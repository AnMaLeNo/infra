"""Discussions reprenant les sessions Claude des runs."""
from __future__ import annotations

import json
import os
import re
import subprocess
import uuid
from datetime import UTC, datetime
from pathlib import Path

from . import compose, config, paths, runner, secrets

READONLY_TOOLS = ",".join([
    "Read", "Glob", "Grep", "mcp__infra__host_info", "mcp__infra__registry_get",
    "mcp__infra__project_status", "mcp__infra__stack_status", "mcp__infra__skills_list",
    "mcp__infra__dns_list", "mcp__infra__secrets_keys", "mcp__infra__traefik_labels",
])
DISALLOWED_TOOLS = "Bash,Edit,Write,NotebookEdit,WebFetch"


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _validate_thread(thread: str) -> str:
    if not isinstance(thread, str) or re.fullmatch(r"[0-9a-f]{12}", thread) is None:
        raise ValueError("identifiant de fil invalide")
    return thread


def _chats_dir() -> Path:
    return paths.runs_dir() / "chats"


def _queue_dir() -> Path:
    return paths.runs_dir() / "chatq"


def _thread_path(thread: str) -> Path:
    return _chats_dir() / f"{_validate_thread(thread)}.json"


def _write_thread(data: dict) -> None:
    d = _chats_dir()
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{data['thread']}.json").write_text(json.dumps(data, indent=2, ensure_ascii=False))


def session_of(run_file: str) -> str | None:
    rec = runner.get_run(run_file)
    session = rec.get("session_id")
    if isinstance(session, str) and session:
        return session
    log = paths.runs_dir() / Path(run_file).with_suffix(".log").name
    if log.exists():
        match = re.search(r'"session_id"\s*:\s*"([^"]+)"', log.read_text(errors="replace"))
        if match:
            return match.group(1)
    return None


def create(run_file: str) -> dict:
    rec = runner.get_run(run_file)
    base_session = session_of(run_file)
    if base_session is None:
        raise ValueError("ce run n'a pas de session Claude reprenable (run dry ou trop ancien)")
    thread = uuid.uuid4().hex[:12]
    data = {
        "thread": thread,
        "run": run_file,
        "project": rec.get("project", ""),
        "base_session": base_session,
        "session": None,
        "act": False,
        "created": _now(),
        "pending": False,
        "messages": [],
    }
    _write_thread(data)
    return data


def list_for_run(run_file: str) -> list[dict]:
    d = _chats_dir()
    if not d.exists():
        return []
    out = []
    for p in sorted(d.glob("*.json")):
        data = json.loads(p.read_text())
        if data.get("run") == run_file:
            out.append({
                "thread": data["thread"],
                "created": data["created"],
                "act": data["act"],
                "pending": data["pending"],
                "n_messages": len(data["messages"]),
            })
    return out


def get(thread: str) -> dict:
    return json.loads(_thread_path(thread).read_text())


def set_act(thread: str, act: bool) -> dict:
    data = get(thread)
    if not isinstance(act, bool):
        raise ValueError("act doit être un booléen")
    data["act"] = act
    _write_thread(data)
    return data


def post(thread: str, text: str) -> dict:
    data = get(thread)
    if data["pending"]:
        raise ValueError("un message est déjà en attente pour ce fil")
    if not isinstance(text, str) or not text.strip():
        raise ValueError("le message ne peut pas être vide")
    data["messages"].append({"role": "user", "text": text, "at": _now()})
    data["pending"] = True
    _write_thread(data)
    d = _queue_dir()
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{thread}.json").write_text(json.dumps({
        "thread": thread, "text": text, "requested": _now(),
    }, ensure_ascii=False))
    return data


def queue_position(thread: str) -> int | None:
    _validate_thread(thread)
    files = sorted(_queue_dir().glob("*.json"), key=lambda p: p.stat().st_mtime)
    for index, p in enumerate(files, start=1):
        if p.stem == thread:
            return index
    chatting = paths.runs_dir() / ".chatting"
    return 0 if chatting.exists() and chatting.read_text().strip() == thread else None


def get_log(thread: str, n: int) -> str:
    _validate_thread(thread)
    if n < 0:
        raise ValueError("index de message invalide")
    return (_chats_dir() / f"{thread}-{n}.log").read_text()


def _system_prompt(project: str, act: bool) -> str:
    if act:
        mode = "Mode ACTION : tu as les mêmes droits que pendant le run ; n'agis que sur demande explicite de l'utilisateur."
    else:
        mode = (
            "Mode ENQUÊTE : tu peux lire fichiers, logs et état (tools MCP de lecture) mais tu ne modifies rien ; "
            "si une action est nécessaire, décris-la précisément et dis à l'utilisateur d'activer « droits du run » "
            "pour ce fil."
        )
    return (
        f"Tu es repris en discussion par l'utilisateur, à propos du run ci-dessus (projet {project}). {mode} "
        "Ne lance pas run_report : ce n'est plus un run. Réponds en français, concis, en distinguant ce que tu as "
        "vérifié de ce que tu supposes."
    )


def _append_assistant(data: dict, text: str, *, ok: bool, log: str, error: str | None = None,
                      cost_usd: float | None = None, duration_ms: int | None = None,
                      num_turns: int | None = None) -> dict:
    message = {
        "role": "assistant",
        "text": text,
        "at": _now(),
        "ok": ok,
        "cost_usd": cost_usd,
        "duration_ms": duration_ms,
        "num_turns": num_turns,
        "log": log,
    }
    if error is not None:
        message["error"] = error
    data["messages"].append(message)
    return message


def process_one() -> dict | None:
    queue = _queue_dir()
    files = sorted(queue.glob("*.json"), key=lambda p: p.stat().st_mtime)
    if not files:
        return None
    req_file = files[0]
    thread_id = req_file.stem
    chatting = paths.runs_dir() / ".chatting"
    chatting.parent.mkdir(parents=True, exist_ok=True)
    chatting.write_text(thread_id)
    data = None
    assistant = None
    secret_values: list[str] = []
    log_name = f"{thread_id}-0.log"
    try:
        req = json.loads(req_file.read_text())
        thread_id = _validate_thread(req.get("thread", thread_id))
        data = get(thread_id)
        n = sum(message.get("role") == "assistant" for message in data["messages"])
        log_name = f"{thread_id}-{n}.log"
        ienv = secrets.infra_env()
        secret_values = list(ienv.values())
        env = {**os.environ, **compose.git_env()}
        if "CLAUDE_CODE_OAUTH_TOKEN" in ienv and "CLAUDE_CODE_OAUTH_TOKEN" not in env:
            env["CLAUDE_CODE_OAUTH_TOKEN"] = ienv["CLAUDE_CODE_OAUTH_TOKEN"]
        llm = config.host().llm
        mcp_cfg = runner._mcp_config()
        cmd = [
            runner.claude_bin(), "-p", req["text"], "--output-format", "stream-json", "--verbose",
            "--model", llm.model, "--effort", llm.effort, "--mcp-config", str(mcp_cfg),
            "--strict-mcp-config", "--append-system-prompt", _system_prompt(data["project"], data["act"]),
        ]
        if data["session"] is None:
            cmd.extend(["--resume", data["base_session"], "--fork-session"])
        else:
            cmd.extend(["--resume", data["session"]])
        if data["act"]:
            cmd.extend(["--allowedTools", runner.ALLOWED_TOOLS, "--permission-mode", "acceptEdits"])
        else:
            cmd.extend(["--allowedTools", READONLY_TOOLS, "--disallowedTools", DISALLOWED_TOOLS])
        cp = subprocess.run(cmd, check=False, cwd=paths.config_dir(), env=env, text=True, capture_output=True)
        out = None
        for line in cp.stdout.splitlines():
            try:
                ev = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(ev, dict) and ev.get("type") == "result":
                out = ev
        log_text = secrets.mask(cp.stdout, secret_values)
        if cp.stderr:
            log_text += "\n\n--- stderr ---\n" + secrets.mask(cp.stderr, secret_values)
        (_chats_dir() / log_name).write_text(log_text)
        if out is None:
            error = secrets.mask(cp.stderr[-4000:], secret_values)
            assistant = _append_assistant(data, "", ok=False, log=log_name, error=error)
        else:
            if data["session"] is None and isinstance(out.get("session_id"), str):
                data["session"] = out["session_id"]
            text = secrets.mask(out.get("result") or "", secret_values)
            assistant = _append_assistant(
                data, text, ok=cp.returncode == 0 and not out.get("is_error", False), log=log_name,
                cost_usd=out.get("total_cost_usd"), duration_ms=out.get("duration_ms"),
                num_turns=out.get("num_turns"),
            )
    except Exception as e:  # noqa: BLE001 — une réponse de chat en échec ne doit pas tuer le runner
        error = secrets.mask(str(e), secret_values)
        if data is not None:
            assistant = _append_assistant(data, "", ok=False, log=log_name, error=error)
    finally:
        if data is not None:
            data["pending"] = False
            _write_thread(data)
        req_file.unlink(missing_ok=True)
        chatting.unlink(missing_ok=True)
    return {"thread": thread_id, "message": assistant} if assistant is not None else None

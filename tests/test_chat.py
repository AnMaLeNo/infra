import json
import subprocess

import pytest
from fastapi.testclient import TestClient

from infra import chat, paths, runner, secrets

TOKEN = "tok-secret-123"


def _save_run(name="run.json", session="BASE"):
    d = paths.runs_dir()
    d.mkdir(parents=True, exist_ok=True)
    rec = {"project": "site-client"}
    if session:
        rec["session_id"] = session
    (d / name).write_text(json.dumps(rec))
    return name


def _stream(session="FORK1", result=f"réponse {TOKEN}"):
    events = [
        {"type": "system", "subtype": "init", "session_id": "S1"},
        {"type": "result", "result": result, "session_id": session, "total_cost_usd": 0.1,
         "num_turns": 1, "duration_ms": 5},
    ]
    return "\n".join(json.dumps(event, ensure_ascii=False) for event in events)


def _fake_claude(monkeypatch, calls, stdout=None, returncode=0, stderr=""):
    monkeypatch.setattr(chat.runner, "claude_bin", lambda: "claude")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": TOKEN})

    def run(cmd, **kwargs):
        calls.append((cmd, kwargs))
        output = stdout if stdout is not None else _stream()
        return subprocess.CompletedProcess(cmd, returncode, output, stderr)

    monkeypatch.setattr(chat.subprocess, "run", run)


def test_session_fallback_and_unresumable_run(env):
    name = _save_run(session=None)
    (paths.runs_dir() / "run.log").write_text('{"type":"system","session_id":"FROMLOG"}\n')
    assert chat.session_of(name) == "FROMLOG"

    no_session = _save_run("old.json", session=None)
    assert chat.session_of(no_session) is None
    with pytest.raises(ValueError, match="ce run n'a pas de session"):
        chat.create(no_session)


def test_first_message_forks_readonly_and_masks_secrets(env, monkeypatch):
    run_file = _save_run()
    thread = chat.create(run_file)
    calls = []
    _fake_claude(monkeypatch, calls)

    chat.post(thread["thread"], "Que s'est-il passé ?")
    result = chat.process_one()

    cmd, kwargs = calls[0]
    assert cmd[cmd.index("--resume") + 1] == "BASE"
    assert "--fork-session" in cmd
    assert cmd[cmd.index("--allowedTools") + 1] == chat.READONLY_TOOLS
    assert cmd[cmd.index("--disallowedTools") + 1] == chat.DISALLOWED_TOOLS
    assert "--permission-mode" not in cmd
    assert cmd[cmd.index("--mcp-config") + 1] == str(paths.runs_dir() / ".mcp.json")
    assert "--strict-mcp-config" in cmd
    assert kwargs["cwd"] == paths.config_dir()
    assert kwargs["env"]["CLAUDE_CODE_OAUTH_TOKEN"] == TOKEN
    assert kwargs["env"]["GIT_TERMINAL_PROMPT"] == "0"
    current = chat.get(thread["thread"])
    assert current["session"] == "FORK1"
    assert current["pending"] is False
    assert current["messages"][-1]["text"] == "réponse ***"
    assert current["messages"][-1]["ok"] is True
    assert result["message"] == current["messages"][-1]
    log_file = paths.runs_dir() / "chats" / current["messages"][-1]["log"]
    assert log_file.exists() and TOKEN not in log_file.read_text() and "***" in log_file.read_text()
    assert list((paths.runs_dir() / "chatq").glob("*.json")) == []


def test_later_message_uses_fork_session_and_action_rights(env, monkeypatch):
    thread = chat.create(_save_run())
    calls = []
    _fake_claude(monkeypatch, calls, stdout=_stream(session="FORK1", result="première réponse"))
    chat.post(thread["thread"], "premier")
    chat.process_one()

    chat.set_act(thread["thread"], True)
    chat.post(thread["thread"], "deuxième")
    chat.process_one()

    cmd = calls[1][0]
    assert cmd[cmd.index("--resume") + 1] == "FORK1"
    assert "--fork-session" not in cmd
    assert cmd[cmd.index("--allowedTools") + 1] == runner.ALLOWED_TOOLS
    assert cmd[cmd.index("--permission-mode") + 1] == "acceptEdits"
    assert chat.get(thread["thread"])["session"] == "FORK1"


def test_post_validation_and_invalid_thread(env):
    thread = chat.create(_save_run())["thread"]
    with pytest.raises(ValueError, match="vide"):
        chat.post(thread, "  ")
    chat.post(thread, "pending")
    assert chat.set_act(thread, True)["act"] is True
    with pytest.raises(ValueError, match="attente"):
        chat.post(thread, "another")
    with pytest.raises(ValueError, match="invalide"):
        chat.get("../invalid")


def test_missing_result_event_records_error_without_changing_session(env, monkeypatch):
    thread = chat.create(_save_run())
    calls = []
    _fake_claude(monkeypatch, calls, stdout=_stream(session="FORK1", result="first"))
    chat.post(thread["thread"], "first")
    chat.process_one()
    _fake_claude(monkeypatch, calls, stdout='{"type":"system","session_id":"OTHER"}', returncode=1,
                 stderr=f"failure {TOKEN}")
    chat.post(thread["thread"], "second")
    chat.process_one()
    current = chat.get(thread["thread"])
    assert current["session"] == "FORK1"
    assert current["pending"] is False
    assert current["messages"][-1]["ok"] is False
    assert current["messages"][-1]["error"] == "failure ***"


def test_claude_exception_closes_pending_message(env, monkeypatch):
    thread = chat.create(_save_run())
    chat.post(thread["thread"], "question")

    def fail_to_resolve_claude():
        raise RuntimeError("claude failure")

    monkeypatch.setattr(chat.runner, "claude_bin", fail_to_resolve_claude)
    monkeypatch.setattr(secrets, "infra_env", lambda: {})

    chat.process_one()

    current = chat.get(thread["thread"])
    assert current["pending"] is False
    assert current["messages"][-1]["ok"] is False
    assert current["messages"][-1]["error"] == "claude failure"
    assert not (paths.runs_dir() / ".chatting").exists()
    assert list((paths.runs_dir() / "chatq").glob("*.json")) == []


def test_watch_processes_chat_before_queued_run(env, monkeypatch):
    thread = chat.create(_save_run())
    chat.post(thread["thread"], "investigate")
    calls = []
    monkeypatch.setattr(chat.runner, "claude_bin", lambda: "claude")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": TOKEN})

    def run_chat(cmd, **kwargs):
        calls.append("chat")
        return subprocess.CompletedProcess(cmd, 0, _stream(), "")

    monkeypatch.setattr(chat.subprocess, "run", run_chat)
    runner.enqueue("unused")
    process_run = runner.process_one

    def process_run_recording(dry=False):
        calls.append("run")
        return process_run(dry=dry)

    monkeypatch.setattr(runner, "process_one", process_run_recording)
    runner.watch(once=True, dry=True)
    assert calls == ["chat", "run"]


def test_run_project_stores_session_id(env, monkeypatch):
    monkeypatch.setattr(runner, "claude_bin", lambda: "claude")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": TOKEN})
    output = _stream(session="RESULT_SESSION", result="done")
    monkeypatch.setattr(
        runner.subprocess, "run",
        lambda cmd, **kwargs: subprocess.CompletedProcess(cmd, 0, output, ""),
    )
    rec = runner.run_project("unused", None, None)
    assert rec["session_id"] == "S1"


def test_chat_api_routes(env):
    from infra.api import app

    _save_run()
    client = TestClient(app)
    response = client.get("/api/runs/run.json/chats")
    assert response.status_code == 200
    assert response.json() == {"resumable": True, "threads": []}
    created = client.post("/api/runs/run.json/chats")
    assert created.status_code == 200
    thread = created.json()["thread"]
    assert client.put(f"/api/chats/{thread}", json={"act": True}).json()["act"] is True
    posted = client.post(f"/api/chats/{thread}/messages", json={"text": "question"})
    assert posted.status_code == 200 and posted.json()["pending"] is True
    detail = client.get(f"/api/chats/{thread}").json()
    assert detail["queue_position"] == 1
    assert detail["runner_busy"] is None
    assert set(detail) == {
        "thread", "run", "project", "base_session", "session", "act", "created",
        "pending", "messages", "queue_position", "runner_busy",
    }
    log = paths.runs_dir() / "chats" / f"{thread}-0.log"
    log.write_text("transcript")
    assert client.get(f"/api/chats/{thread}/log/0").text == "transcript"
    assert client.get("/api/chats/invalid").status_code == 400

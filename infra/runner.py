"""Lance `claude -p` sur un projet avec les bons skills, et journalise dans <config>/runs/.

`infra watch` surveille registry.yaml : à chaque changement d'une entrée (UI, git pull, édition),
un run est lancé pour ce projet. Un seul run à la fois (verrou), file d'attente par projet.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from datetime import UTC, datetime
from pathlib import Path

import yaml

from . import config, paths, secrets, skills
from .models import Project

ALLOWED_TOOLS = "Bash,Read,Edit,Write,Glob,Grep"


def _hash(prj: Project | None) -> str:
    if prj is None:
        return "absent"
    return hashlib.sha256(json.dumps(prj.model_dump(), sort_keys=True).encode()).hexdigest()[:16]


def build_prompt(name: str, prj: Project | None, previous: dict | None) -> str:
    h = config.host()
    sk = skills.paths_for(prj)
    missing = [rel for rel, p in sk.items() if p is None]
    if missing:
        raise RuntimeError(f"skills introuvables : {missing} (ni dans <config>/skills ni dans tool/skills)")
    tool = paths.tool_dir()
    lines = [
        f"Tu es le runner de l'outil `infra` sur l'hôte `{h.name}`. Répertoire courant : {paths.config_dir()} (repo de config).",
        f"Lis d'abord {tool / 'README.md'} et {tool / 'docs/conventions.md'}.",
        f"Projet concerné : `{name}`.",
        "",
        "Entrée du registre AVANT :",
        "```yaml", yaml.safe_dump({name: previous} if previous else {name: None}, sort_keys=False).strip(), "```",
        "Entrée du registre APRÈS (état voulu) :",
        "```yaml", yaml.safe_dump({name: prj.model_dump(exclude_defaults=True)} if prj else {name: None}, sort_keys=False).strip(), "```",
        "",
        "Applique l'état voulu en suivant EXACTEMENT ces skills, dans l'ordre :",
        *[f"- {rel} → {p}" for rel, p in sk.items()],
        "",
        "Règles : commence par `infra status " + name + "` ; n'agis que sur l'écart ; utilise le CLI `infra` pour tout ce qui est mécanique ;",
        "ne modifie jamais un fichier généré ; ne publie jamais de port ; n'affiche jamais la valeur d'un secret ;",
        "si une information manque (port, container, variable d'env), déduis-la depuis le code du projet et fixe-la via `infra set`,",
        "sinon termine en échec avec la question précise à poser à l'humain.",
        "Termine par un résumé : ce qui a été fait, les URLs, ce qu'il reste à faire par un humain.",
    ]
    return "\n".join(lines)


def claude_bin() -> str:
    b = shutil.which("claude")
    if not b:
        raise RuntimeError("`claude` introuvable dans le PATH (npm i -g @anthropic-ai/claude-code)")
    return b


def run_project(name: str, prj: Project | None, previous: dict | None, dry: bool = False) -> dict:
    started = datetime.now(UTC)
    prompt = build_prompt(name, prj, previous)
    rec = {"project": name, "started": started.isoformat(), "skills": list(skills.for_project(prj)),
           "before": previous, "after": prj.model_dump() if prj else None, "prompt": prompt}
    if dry:
        rec.update({"dry": True, "ok": True})
        return rec
    env = {**os.environ}
    ienv = secrets.infra_env()
    if "CLAUDE_CODE_OAUTH_TOKEN" in ienv and "CLAUDE_CODE_OAUTH_TOKEN" not in env:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = ienv["CLAUDE_CODE_OAUTH_TOKEN"]
    cmd = [claude_bin(), "-p", prompt, "--output-format", "json", "--allowedTools", ALLOWED_TOOLS,
           "--permission-mode", "acceptEdits"]
    cp = subprocess.run(cmd, check=False, cwd=paths.config_dir(), env=env, text=True, capture_output=True)
    secret_values = list(ienv.values())
    try:
        out = json.loads(cp.stdout)
        result_text = out.get("result", "")
        rec.update({"cost_usd": out.get("total_cost_usd"), "duration_ms": out.get("duration_ms"),
                    "num_turns": out.get("num_turns"), "is_error": out.get("is_error", False)})
    except json.JSONDecodeError:
        result_text = cp.stdout
        rec["is_error"] = True
    rec["result"] = secrets.mask(result_text, secret_values)
    rec["stderr"] = secrets.mask(cp.stderr[-4000:], secret_values)
    rec["ok"] = cp.returncode == 0 and not rec.get("is_error")
    rec["finished"] = datetime.now(UTC).isoformat()
    _save(rec)
    return rec


def _save(rec: dict) -> Path:
    d = paths.runs_dir()
    d.mkdir(parents=True, exist_ok=True)
    p = d / f"{rec['started'].replace(':', '').replace('+00:00', 'Z')}-{rec['project']}.json"
    p.write_text(json.dumps(rec, indent=2, ensure_ascii=False))
    return p


def list_runs(limit: int = 50) -> list[dict]:
    d = paths.runs_dir()
    if not d.exists():
        return []
    out = []
    for p in sorted(d.glob("*.json"), reverse=True)[:limit]:
        if p.name.startswith("."):
            continue
        r = json.loads(p.read_text())
        out.append({k: r.get(k) for k in ("project", "started", "finished", "ok", "skills", "cost_usd", "duration_ms", "dry")}
                   | {"file": p.name})
    return out


def get_run(file: str) -> dict:
    return json.loads((paths.runs_dir() / file).read_text())


# ---- watcher -------------------------------------------------------------------------------------

def _state_file() -> Path:
    return paths.runs_dir() / ".state.json"


def load_state() -> dict:
    p = _state_file()
    return json.loads(p.read_text()) if p.exists() else {}


def save_state(state: dict) -> None:
    p = _state_file()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(state, indent=2))


def diff(state: dict) -> list[tuple[str, Project | None]]:
    """Projets dont l'entrée diffère de la dernière appliquée (ou disparus)."""
    reg = config.registry()
    changed = []
    for name, prj in reg.projects.items():
        if state.get(name, {}).get("hash") != _hash(prj):
            changed.append((name, prj))
    for name, st in state.items():
        if name not in reg.projects and st.get("hash") != "absent":
            changed.append((name, None))
    return changed


def mark_applied(state: dict, name: str, prj: Project | None, ok: bool) -> None:
    state[name] = {"hash": _hash(prj), "entry": prj.model_dump() if prj else None, "ok": ok,
                   "at": datetime.now(UTC).isoformat()}
    save_state(state)


def watch(interval: float = 5.0, once: bool = False, dry: bool = False) -> None:
    print(f"watch {paths.config_dir() / 'registry.yaml'} toutes les {interval}s", file=sys.stderr)
    while True:
        state = load_state()
        try:
            changes = diff(state)
        except Exception as e:  # registre invalide : on attend une correction
            print(f"registry.yaml invalide : {e}", file=sys.stderr)
            changes = []
        for name, prj in changes:
            previous = state.get(name, {}).get("entry")
            print(f"→ run {name}", file=sys.stderr)
            try:
                rec = run_project(name, prj, previous, dry=dry)
                ok = rec.get("ok", False)
            except Exception as e:
                print(f"run {name} : {e}", file=sys.stderr)
                ok = False
            # on marque même en échec : pas de boucle infinie ; l'UI montre l'échec, une nouvelle édition relance
            if not dry:
                mark_applied(state, name, prj, ok)
        if once:
            return
        time.sleep(interval)

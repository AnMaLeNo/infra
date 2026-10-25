"""Lance `claude -p` sur un projet avec les bons skills, et journalise dans <config>/runs/.

Rien ne se déclenche tout seul : un run est demandé explicitement (bouton « Lancer Claude » du front → `enqueue`,
ou `infra run <P>` en CLI). `infra watch` (service infra-runner) consomme la file `runs/queue/` un projet à la fois.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path

import yaml

from . import compose, config, forge, paths, secrets, skills, tools
from .models import Project

ALLOWED_TOOLS = "Bash,Read,Edit,Write,Glob,Grep,WebFetch,mcp__infra"


def _hash(entry) -> str:
    """Empreinte de l'intention (Project ou Shared, sans les constats du LLM) ; `absent` si retiré."""
    if entry is None:
        return "absent"
    return hashlib.sha256(json.dumps(entry.intent(), sort_keys=True).encode()).hexdigest()[:16]


def build_prompt(name: str, prj: Project | None, previous: dict | None) -> str:
    h = config.host()
    sk = skills.paths_for(prj)
    missing = [rel for rel, p in sk.items() if p is None]
    if missing:
        raise RuntimeError(f"skills introuvables : {missing} (ni dans <config>/skills ni dans tool/skills)")
    tool = paths.tool_dir()
    after = yaml.safe_dump({name: prj.model_dump(exclude_defaults=True)} if prj else {name: None}, sort_keys=False).strip()
    before = yaml.safe_dump({name: previous} if previous else {name: None}, sort_keys=False).strip()
    lines = [
        f"Tu es l'agent de déploiement de l'hôte `{h.name}` (outil `infra`). Répertoire courant : {paths.config_dir()} (repo de config).",
        f"Contexte à lire d'abord : {tool / 'README.md'} puis {tool / 'docs/conventions.md'}.",
        "",
        f"Objectif : amener le projet `{name}` à l'état voulu ci-dessous. Tu as carte blanche sur le chemin ;",
        "les skills décrivent le but et l'état cible à atteindre, pas une procédure.",
        "",
        "Entrée du registre AVANT :", "```yaml", before, "```",
        "Entrée du registre APRÈS (état voulu) :", "```yaml", after, "```",
        "",
        "Skills à satisfaire :",
        *[f"- {rel} → {p}" for rel, p in sk.items()],
        "",
        *([
            "`service` n'est pas fixé dans le registre : regarde le repo et choisis `compose` (il a — ou tu lui écris — un",
            "docker-compose.yml) ou `static` (fichiers/ build servis par nginx), écris ton choix avec registry_set, puis satisfais",
            "le skill service/<choix> correspondant (l'autre est fourni pour que tu puisses comparer).", "",
        ] if prj is not None and prj.enabled and prj.service is None else []),
        "Le *comment* est à toi : `container`, `port`, `compose_file`, `build`, `static_dir` sont des constats que tu",
        "déduis du repo (compose, Dockerfile, package.json…) et que tu écris dans le registre avec registry_set — l'utilisateur",
        "ne les saisit pas et ne les connaît pas.",
        "",
        "Tu disposes du serveur MCP `infra` (tools atomiques : repo_sync, compose_override, traefik_labels, env_decrypt,",
        "dns_ensure/dns_prune/dns_list, registry_get/registry_set, project_status, host_info, secrets_keys…) et de Bash",
        "(docker compose, git, curl, sudo limité). Les tools sont des raccourcis fiables pour les gestes répétitifs ; si l'un",
        "ne convient pas au projet, il te le dira : fais alors autrement, à la main, et dis-le dans ton résumé.",
        "",
        "Règles : commence par constater l'état réel (project_status) et n'agis que sur l'écart ; ne publie jamais de port ;",
        "n'affiche jamais la valeur d'un secret ; n'invente pas de valeur manquante (termine en échec avec la question précise) ;",
        "ne modifie pas le repo du projet sauf cas prévu par un skill (PR). Termine par un résumé : ce qui a été fait,",
        "les URLs, ce que tu as dû faire à la main (candidats à devenir des tools), ce qu'il reste à un humain.",
        "",
        f"Clôture obligatoire : termine par UN appel au tool `run_report(project={name!r}, status=ok|bloqué|échec, summary, needs_human)`.",
        "`ok` seulement si l'état cible est atteint et vérifié ; `bloqué` s'il manque quelque chose à un humain (rien de cassé) ;",
        "`échec` sinon. Sans cet appel, le run compte comme échec.",
    ]
    return "\n".join(lines)


def build_shared_prompt(name: str, svc, previous: dict | None) -> str:
    """Prompt d'un run sur un service partagé (shared.yaml) : même cadre que les projets, skill `shared-service`."""
    h = config.host()
    sk = skills.paths_for_shared(svc)
    missing = [rel for rel, p in sk.items() if p is None]
    if missing:
        raise RuntimeError(f"skills introuvables : {missing}")
    tool = paths.tool_dir()
    key = config.shared_key(name)
    after = yaml.safe_dump({name: svc.model_dump(exclude_defaults=True)} if svc else {name: None}, sort_keys=False).strip()
    before = yaml.safe_dump({name: previous} if previous else {name: None}, sort_keys=False).strip()
    users = config.users_of(name)
    lines = [
        f"Tu es l'agent de déploiement de l'hôte `{h.name}` (outil `infra`). Répertoire courant : {paths.config_dir()} (repo de config).",
        f"Contexte à lire d'abord : {tool / 'README.md'} puis {tool / 'docs/conventions.md'}.",
        "",
        f"Objectif : amener le SERVICE PARTAGÉ `{name}` (shared.yaml, un seul exemplaire sur l'hôte) à l'état voulu ci-dessous.",
        "Tu as carte blanche sur le chemin ; le skill décrit le but et l'état cible, pas une procédure.",
        "",
        "Entrée de shared.yaml AVANT :", "```yaml", before, "```",
        "Entrée de shared.yaml APRÈS (état voulu) :", "```yaml", after, "```",
        f"Projets qui le consomment (`uses:`) : {', '.join(users) if users else 'aucun pour l instant'}.",
        "",
        "Skills à satisfaire :",
        *[f"- {rel} → {p}" for rel, p in sk.items()],
        "",
        "Les choix humains (kind, listen, port, auth, data) sont fixés : tu ne les changes pas ; s'ils sont incohérents ou",
        "incomplets (ex. `listen: [lan]` sans `lan_ip` dans host.yaml), termine `bloqué` avec la question précise.",
        "Le *comment* (image, container, compose_file) est à toi : écris-le avec shared_set.",
        "",
        "Tu disposes du serveur MCP `infra` (shared_get/shared_set/shared_status, host_addresses, host_info, registry_get,",
        "secrets_keys, stack_status…) et de Bash (docker compose, ss, git, curl, sudo limité).",
        "",
        "Règles : constate l'état réel d'abord (shared_status) et n'agis que sur l'écart ; n'écoute jamais sur 0.0.0.0 ;",
        "n'affiche jamais la valeur d'un secret ; n'invente pas de valeur manquante. Termine par un résumé : fait, vérifié,",
        "fait à la main (candidats à devenir des tools), reste à un humain.",
        "",
        f"Clôture obligatoire : termine par UN appel au tool `run_report(project={key!r}, status=ok|bloqué|échec, summary, needs_human)`.",
        "`ok` seulement si l'état cible est atteint et vérifié ; `bloqué` s'il manque quelque chose à un humain (rien de cassé) ;",
        "`échec` sinon. Sans cet appel, le run compte comme échec.",
    ]
    return "\n".join(lines)


def read_report(name: str) -> dict:
    """Rapport déposé par le tool MCP `run_report` pendant le run. Absent ou pour un autre projet → statut `inconnu`."""
    p = tools.report_path()
    if not p.exists():
        return {"status": "inconnu"}
    try:
        rep = json.loads(p.read_text())
    except json.JSONDecodeError:
        return {"status": "inconnu"}
    if rep.get("project") != name or rep.get("status") not in ("ok", "bloqué", "échec"):
        return {"status": "inconnu"}
    return rep


def claude_bin() -> str:
    b = shutil.which("claude")
    if not b:
        raise RuntimeError("`claude` introuvable dans le PATH (npm i -g @anthropic-ai/claude-code)")
    return b


def run_project(name: str, prj: Project | None, previous: dict | None, dry: bool = False) -> dict:
    return _run(name, build_prompt(name, prj, previous), skills.for_project(prj), previous,
                prj.model_dump() if prj else None, dry)


def run_shared(name: str, svc, previous: dict | None, dry: bool = False) -> dict:
    """Run sur un service partagé : enregistré sous la clé `shared.<nom>` (runs, état, historique)."""
    return _run(config.shared_key(name), build_shared_prompt(name, svc, previous), skills.for_shared(svc), previous,
                svc.model_dump() if svc else None, dry)


def _run(name: str, prompt: str, skill_list: list[str], previous: dict | None, after: dict | None, dry: bool) -> dict:
    started = datetime.now(UTC)
    rec = {"project": name, "started": started.isoformat(), "skills": list(skill_list),
           "before": previous, "after": after, "prompt": prompt}
    if dry:
        rec.update({"dry": True, "ok": True})
        return rec
    llm = config.host().llm
    # id de session choisi ici (pas lu dans la sortie) et nom lisible : le front ouvre cette session dans l'éditeur (infra/editor.py)
    rec.update({"model": llm.model, "effort": llm.effort, "session_id": str(uuid.uuid4())})
    ienv = secrets.infra_env()
    # git/gh de Claude : pas de prompt, et GITHUB_TOKEN (repos privés, PR) si présent — jamais écrit sur disque
    env = {**os.environ, **compose.git_env()}
    if "CLAUDE_CODE_OAUTH_TOKEN" in ienv and "CLAUDE_CODE_OAUTH_TOKEN" not in env:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = ienv["CLAUDE_CODE_OAUTH_TOKEN"]
    mcp_cfg = paths.runs_dir() / ".mcp.json"
    mcp_cfg.parent.mkdir(parents=True, exist_ok=True)
    mcp_cfg.write_text(json.dumps({"mcpServers": {"infra": {"command": shutil.which("infra") or "infra", "args": ["mcp"],
                                                             "env": {"INFRA_CONFIG_DIR": str(paths.config_dir())}}}}))
    # stream-json + --verbose : on garde TOUT le déroulé (tool calls, textes, résultat) dans runs/<run>.log
    cmd = [claude_bin(), "-p", prompt, "--output-format", "stream-json", "--verbose",
           "--model", llm.model, "--effort", llm.effort, "--allowedTools", ALLOWED_TOOLS,
           "--permission-mode", "acceptEdits", "--mcp-config", str(mcp_cfg), "--strict-mcp-config",
           "--session-id", rec["session_id"], "--name", f"{name} · {started.astimezone():%d/%m %H:%M}"]
    tools.report_path().unlink(missing_ok=True)  # jamais réutiliser le rapport d'un run précédent
    cp = subprocess.run(cmd, check=False, cwd=paths.config_dir(), env=env, text=True, capture_output=True)
    secret_values = list(ienv.values())
    result_text, out = "", None
    for line in cp.stdout.splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(ev, dict) and ev.get("type") == "result":
            out = ev
    if out is not None:
        result_text = out.get("result", "")
        rec.update({"cost_usd": out.get("total_cost_usd"), "duration_ms": out.get("duration_ms"),
                    "num_turns": out.get("num_turns"), "is_error": out.get("is_error", False)})
    else:
        result_text = cp.stdout[-4000:]
        rec["is_error"] = True
    rec["result"] = secrets.mask(result_text, secret_values)
    rec["stderr"] = secrets.mask(cp.stderr[-4000:], secret_values)
    rep = read_report(name)
    rec["status"] = rep["status"]
    rec["needs_human"] = secrets.mask(rep.get("needs_human") or "", secret_values)
    rec["report"] = secrets.mask(rep.get("summary") or "", secret_values)
    # claude -p sort 0 même quand il s'est déclaré bloqué : c'est son appel à run_report qui fait foi
    rec["ok"] = cp.returncode == 0 and not rec.get("is_error") and rec["status"] == "ok"
    rec["finished"] = datetime.now(UTC).isoformat()
    masked_log = secrets.mask(cp.stdout, secret_values)
    # skillforge (host.yaml → forge.mode) : juge la skill à partir du déroulé masqué ; jamais bloquant, n'affecte pas `ok`
    rec["forge"] = forge.judge(rec, masked_log, episode_id=f"{started:%Y%m%dT%H%M%S}-{name}")
    p = _save(rec)
    p.with_suffix(".log").write_text(masked_log
                                     + ("\n\n--- stderr ---\n" + secrets.mask(cp.stderr, secret_values) if cp.stderr else ""))
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
        out.append(_summary(r, p))
    return out


def _summary(r: dict, p: Path) -> dict:
    keys = ("project", "started", "finished", "ok", "status", "skills", "cost_usd", "duration_ms", "num_turns", "dry", "model", "effort")
    return {k: r.get(k) for k in keys} | {"file": p.name, "log": p.with_suffix(".log").name if p.with_suffix(".log").exists() else None}


def runs_for_project(name: str) -> list[dict]:
    d = paths.runs_dir()
    if not d.exists():
        return []
    return [_summary(json.loads(p.read_text()), p) for p in sorted(d.glob(f"*-{name}.json"))
            if not p.name.startswith(".")]


def get_run(file: str) -> dict:
    return json.loads((paths.runs_dir() / file).read_text())


def session_of(file: str) -> str | None:
    """Session Claude d'un run : enregistrée, sinon (runs antérieurs) lue dans le premier événement de son .log."""
    sid = get_run(file).get("session_id")
    if sid:
        return sid
    log = (paths.runs_dir() / file).with_suffix(".log")
    try:
        return json.loads(log.read_text().split("\n", 1)[0]).get("session_id")
    except (OSError, json.JSONDecodeError):
        return None


def get_log(file: str) -> str:
    p = paths.runs_dir() / file
    return p.read_text() if p.exists() else ""


# ---- file d'attente et état -------------------------------------------------------------------

def _state_file() -> Path:
    return paths.runs_dir() / ".state.json"


def load_state() -> dict:
    p = _state_file()
    return json.loads(p.read_text()) if p.exists() else {}


def save_state(state: dict) -> None:
    p = _state_file()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(state, indent=2))


def changed_since_last_run(state: dict) -> list[str]:
    """Projets dont l'entrée du registre diffère de celle du dernier run (indicateur « modifié » du front)."""
    reg = config.registry()
    sh = config.shared().services
    out = [n for n, p in reg.projects.items() if state.get(n, {}).get("hash") != _hash(p)]
    out += [config.shared_key(n) for n, p in sh.items() if state.get(config.shared_key(n), {}).get("hash") != _hash(p)]
    out += [n for n, st in state.items() if n not in reg.projects and n.removeprefix("shared.") not in sh
            and st.get("hash") != "absent"]
    return out


def mark_applied(state: dict, name: str, entry, ok: bool, status: str | None = None) -> None:
    state[name] = {"hash": _hash(entry), "entry": entry.model_dump() if entry else None, "ok": ok,
                   "status": status, "at": datetime.now(UTC).isoformat()}
    save_state(state)


def queue_dir() -> Path:
    d = paths.runs_dir() / "queue"
    d.mkdir(parents=True, exist_ok=True)
    return d


def enqueue(name: str) -> dict:
    """Demande un run pour un projet, ou un service partagé sous la clé `shared.<nom>` (idempotent : une seule demande en attente par cible)."""
    p = queue_dir() / f"{name}.json"
    if p.exists():
        return json.loads(p.read_text())
    req = {"project": name, "requested": datetime.now(UTC).isoformat()}
    p.write_text(json.dumps(req))
    return req


def queued() -> list[str]:
    return sorted(p.stem for p in queue_dir().glob("*.json"))


def running() -> str | None:
    p = paths.runs_dir() / ".running"
    return p.read_text().strip() if p.exists() else None


def _set_running(name: str | None) -> None:
    p = paths.runs_dir() / ".running"
    if name is None:
        p.unlink(missing_ok=True)
    else:
        p.write_text(name)


def process_one(dry: bool = False) -> dict | None:
    """Exécute la plus ancienne demande de la file. Retourne le run, ou None si la file est vide."""
    files = sorted(queue_dir().glob("*.json"), key=lambda p: p.stat().st_mtime)
    if not files:
        return None
    req = files[0]
    name = req.stem
    shared_name = name.removeprefix("shared.") if name.startswith("shared.") else None
    state = load_state()
    previous = state.get(name, {}).get("entry")
    if shared_name is not None:
        entry = config.shared().services.get(shared_name)
        runner_fn, skill_list = (lambda: run_shared(shared_name, entry, previous, dry=dry)), skills.for_shared(entry)
    else:
        entry = config.registry().projects.get(name)
        runner_fn, skill_list = (lambda: run_project(name, entry, previous, dry=dry)), (skills.for_project(entry) if entry else [])
    _set_running(name)
    try:
        rec = runner_fn()
    except Exception as e:  # noqa: BLE001 — un run qui plante ne doit pas tuer le service
        rec = {"project": name, "started": datetime.now(UTC).isoformat(), "ok": False,
               "result": f"erreur du runner : {e}", "skills": skill_list}
        _save(rec)
    finally:
        _set_running(None)
        req.unlink(missing_ok=True)
    if not dry:
        # relu après le run : le LLM a pu écrire ses constats (service, container, port, image…)
        if entry is None:
            after = None
        elif shared_name is not None:
            after = config.shared().services.get(shared_name)
        else:
            after = config.registry().projects.get(name)
        mark_applied(state, name, after, rec.get("ok", False), rec.get("status"))
        # la config écrite pendant le run (constats, shared/<S>/, host.yaml…) est versionnée tout de suite
        from . import gitsync
        rec["git"] = gitsync.commit_push(f"run {name} : {rec.get('status') or 'erreur'} — {(rec.get('report') or '')[:72]}".rstrip(" —"))
        if "started" in rec and rec.get("skills") is not None:
            _save(rec)
    return rec


def watch(interval: float = 3.0, once: bool = False, dry: bool = False) -> None:
    """Boucle du service infra-runner : consomme la file, un projet à la fois."""
    print(f"runner : file {queue_dir()} (toutes les {interval}s)", file=sys.stderr)
    while True:
        rec = process_one(dry=dry)
        if rec:
            print(f"run {rec['project']} : {rec.get('status') or ('ok' if rec.get('ok') else 'échec')}", file=sys.stderr)
        if once:
            return
        if rec is None:
            time.sleep(interval)

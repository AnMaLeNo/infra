"""API + UI web : édite registry.yaml, montre l'état et les runs. Ne lance pas claude (c'est le runner, via le fichier)."""
from __future__ import annotations

from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import HTMLResponse, PlainTextResponse
from pydantic import ValidationError

from . import __version__, compose, config, editor, gitsync, history, paths, runner, skills
from .models import Llm, Project, Shared
from .render import Target

app = FastAPI(title="infra", version=__version__)
WEB = Path(__file__).parent / "web"


@app.get("/", response_class=HTMLResponse)
def index():
    return (WEB / "index.html").read_text()


@app.get("/api/host")
def host():
    h = config.host()
    return {**h.model_dump(), "version": __version__, "config_dir": str(paths.config_dir())}


@app.get("/api/settings")
def settings():
    """Paramètres LLM (host.yaml → llm), communs à tous les runs."""
    return config.host().llm.model_dump()


@app.put("/api/settings")
def put_settings(body: dict):
    try:
        Llm.model_validate(body)
        llm = config.set_llm(**body).model_dump()
        return llm | {"git": gitsync.commit_push("ui : settings LLM")}
    except (ValidationError, ValueError) as e:
        raise HTTPException(422, str(e)) from e


@app.get("/api/projects")
def projects():
    reg, h = config.registry(), config.host()
    state = runner.load_state()
    queued, running = set(runner.queued()), runner.running()
    out = {}
    for name, prj in reg.projects.items():
        t = Target(name, prj, h)
        st = state.get(name, {})
        out[name] = {"entry": prj.model_dump(), "decided": prj.decided(), "urls": t.urls, "skills": skills.for_project(prj),
                     "changed": st.get("hash") != runner._hash(prj), "never_run": "hash" not in st,
                     "changed_fields": runner.changed_fields(prj, st),
                     "queued": name in queued, "running": running == name,
                     "last_ok": st.get("ok"), "last_status": st.get("status"), "last_at": st.get("at")}
    return out


@app.post("/api/projects/{name}/run")
def request_run(name: str):
    """Bouton « Lancer Claude » : met le projet dans la file du runner. Rien ne se lance sans ça."""
    if name not in config.registry().projects and name not in runner.load_state():
        raise HTTPException(404, "projet inconnu")
    return runner.enqueue(name)


@app.get("/api/projects/{name}/history")
def project_history(name: str):
    """Tout ce qui est arrivé à un projet : versions de sa config (runs/history) + ses runs (runs/*-<P>.json)."""
    return {"config": history.for_project(name), "runs": runner.runs_for_project(name)}


@app.get("/api/shared")
def shared_services():
    """Services partagés (shared.yaml) avec leur état de run, comme /api/projects."""
    reg = config.shared()
    state = runner.load_state()
    queued, running = set(runner.queued()), runner.running()
    out = {}
    for name, svc in reg.services.items():
        key = config.shared_key(name)
        st = state.get(key, {})
        out[name] = {"entry": svc.model_dump(), "decided": svc.decided(), "users": config.users_of(name), "key": key,
                     "changed": st.get("hash") != runner._hash(svc), "never_run": "hash" not in st,
                     "changed_fields": runner.changed_fields(svc, st),
                     "queued": key in queued, "running": running == key,
                     "last_ok": st.get("ok"), "last_status": st.get("status"), "last_at": st.get("at")}
    return out


@app.put("/api/shared/{name}")
def put_shared(name: str, body: dict):
    try:
        Shared.model_validate(body)
        svc = config.set_shared_fields(name, **body)
    except (ValidationError, ValueError) as e:
        raise HTTPException(422, str(e)) from e
    return svc.model_dump() | {"git": gitsync.commit_push(f"ui : service partagé {name} déclaré/modifié")}


@app.delete("/api/shared/{name}")
def delete_shared(name: str):
    try:
        config.remove_shared(name)
    except ValueError as e:
        raise HTTPException(409, str(e)) from e
    return {"ok": True, "git": gitsync.commit_push(f"ui : service partagé {name} retiré")}


@app.post("/api/shared/{name}/run")
def request_shared_run(name: str):
    """Bouton « Lancer Claude » d'un service partagé : file du runner, clé `shared.<nom>`."""
    key = config.shared_key(name)
    if name not in config.shared().services and key not in runner.load_state():
        raise HTTPException(404, "service partagé inconnu")
    return runner.enqueue(key)


@app.get("/api/shared/{name}/history")
def shared_history(name: str):
    key = config.shared_key(name)
    return {"config": history.for_project(key), "runs": runner.runs_for_project(key)}


@app.get("/api/runner")
def runner_state():
    return {"queued": runner.queued(), "running": runner.running()}


@app.get("/api/projects/{name}")
def project(name: str):
    try:
        prj = config.project(name)
    except KeyError:
        raise HTTPException(404, "projet inconnu") from None
    t = Target(name, prj, config.host())
    try:
        status = compose.status(t)
    except Exception as e:  # docker indisponible dans le container ?
        status = {"error": str(e)}
    return {"entry": prj.model_dump(), "urls": t.urls, "status": status, "skills": skills.for_project(prj)}


@app.put("/api/projects/{name}")
def put_project(name: str, body: dict):
    try:
        Project.model_validate(body)
        prj = config.set_project_fields(name, **{k: v for k, v in body.items()})
    except (ValidationError, ValueError) as e:
        raise HTTPException(422, str(e)) from e
    return prj.model_dump() | {"git": gitsync.commit_push(f"ui : projet {name} modifié")}


@app.delete("/api/projects/{name}")
def delete_project(name: str):
    config.remove_project(name)
    return {"ok": True, "git": gitsync.commit_push(f"ui : projet {name} retiré du registre")}


@app.get("/api/runs")
def runs(limit: int = 50):
    return runner.list_runs(limit)


def _check_file(file: str, suffix: str) -> None:
    if "/" in file or file.startswith(".") or not file.endswith(suffix):
        raise HTTPException(400)


@app.get("/api/runs/{file}", response_class=PlainTextResponse)
def run_detail(file: str):
    _check_file(file, ".json")
    r = runner.get_run(file)
    return (r.get("result") or "") + ("\n\n--- stderr ---\n" + r["stderr"] if r.get("stderr") else "")


@app.get("/api/runs/{file}/full")
def run_full(file: str):
    """Le run complet (prompt, entrée avant/après, résultat, coût…)."""
    _check_file(file, ".json")
    return runner.get_run(file)


@app.post("/api/runs/{file}/editor")
def run_in_editor(file: str):
    """Demande à l'éditeur (`infra editor`) d'ouvrir la session Claude de ce run ; le front ouvre ensuite l'éditeur."""
    _check_file(file, ".json")
    try:
        session = runner.session_of(file)
    except FileNotFoundError as e:
        raise HTTPException(404, str(e)) from e
    if not session:
        raise HTTPException(404, "pas de session enregistrée pour ce run")
    editor.request_session(session)
    return {"ok": True}


@app.get("/api/runs/{file}/log", response_class=PlainTextResponse)
def run_log(file: str):
    """Déroulé complet du run (stream-json de claude : textes, appels d'outils, résultat)."""
    _check_file(file, ".log")
    return runner.get_log(file)


@app.get("/api/skills")
def skills_list():
    return skills.list_all()


@app.get("/api/registry", response_class=PlainTextResponse)
def registry_raw():
    return (paths.config_dir() / "registry.yaml").read_text()

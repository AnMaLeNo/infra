"""API + UI web : édite registry.yaml, montre l'état et les runs. Ne lance pas claude (c'est le runner, via le fichier)."""
from __future__ import annotations

from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import HTMLResponse, PlainTextResponse
from pydantic import ValidationError

from . import __version__, compose, config, history, paths, runner, skills
from .models import Llm, Project
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
        return config.set_llm(**body).model_dump()
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
    return prj.model_dump()


@app.delete("/api/projects/{name}")
def delete_project(name: str):
    config.remove_project(name)
    return {"ok": True}


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

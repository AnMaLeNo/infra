"""API + UI web : édite registry.yaml, montre l'état et les runs. Ne lance pas claude (c'est le runner, via le fichier)."""
from __future__ import annotations

from pathlib import Path

from fastapi import FastAPI, HTTPException
from fastapi.responses import HTMLResponse, PlainTextResponse
from pydantic import ValidationError

from . import __version__, compose, config, paths, runner, skills
from .models import Project
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


@app.get("/api/projects")
def projects():
    reg, h = config.registry(), config.host()
    state = runner.load_state()
    out = {}
    for name, prj in reg.projects.items():
        t = Target(name, prj, h)
        st = state.get(name, {})
        out[name] = {"entry": prj.model_dump(), "urls": t.urls, "skills": skills.for_project(prj),
                     "applied": st.get("hash") == runner._hash(prj), "last_ok": st.get("ok"), "last_at": st.get("at")}
    return out


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


@app.get("/api/runs/{file}", response_class=PlainTextResponse)
def run_detail(file: str):
    if "/" in file or not file.endswith(".json"):
        raise HTTPException(400)
    r = runner.get_run(file)
    return (r.get("result") or "") + ("\n\n--- stderr ---\n" + r["stderr"] if r.get("stderr") else "")


@app.get("/api/skills")
def skills_list():
    return skills.list_all()


@app.get("/api/registry", response_class=PlainTextResponse)
def registry_raw():
    return (paths.config_dir() / "registry.yaml").read_text()

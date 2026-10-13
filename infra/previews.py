"""Previews par PR : <P>-pr-<N> dans /srv/apps/<P>/previews/pr-<N>/, verrou flock par PR."""
from __future__ import annotations

import fcntl
import shutil
from contextlib import contextmanager

from . import compose, config, paths
from .render import Target, render


@contextmanager
def _lock(t: Target):
    t.dir.parent.mkdir(parents=True, exist_ok=True)
    lockfile = t.dir.parent / f".pr-{t.pr}.lock"
    with open(lockfile, "w") as f:
        fcntl.flock(f, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def _check(t: Target) -> None:
    if t.project.previews == "none":
        raise RuntimeError(f"{t.name}: previews désactivées dans le registre (previews: none)")
    if not t.project.enabled:
        raise RuntimeError(f"{t.name}: projet désactivé")


def up(t: Target) -> dict:
    _check(t)
    with _lock(t):
        commit = compose.fetch(t)
        render(t)
        compose.up(t)
        s = compose.status(t)
    url = t.urls.get("public") or t.urls.get("private")
    return {"project": t.compose_project, "commit": commit, "url": url, "running": s["running"]}


def down(t: Target) -> None:
    with _lock(t):
        compose.down(t, volumes=True)
        shutil.rmtree(t.dir, ignore_errors=True)
        shutil.rmtree(paths.data_dir() / t.compose_project, ignore_errors=True)


def list_previews(name: str) -> dict[int, dict]:
    prj, h = config.project(name), config.host()
    base = paths.app_dir(name) / "previews"
    out = {}
    if base.exists():
        for d in sorted(base.glob("pr-*")):
            pr = int(d.name[3:])
            t = Target(name, prj, h, pr)
            s = compose.status(t)
            out[pr] = {"running": s["running"], "url": t.urls.get("public") or t.urls.get("private"), "commit": s["commit"]}
    return out


def _pr_open(repo_url: str, pr: int) -> bool | None:
    """True/False via l'API GitHub publique, None si indéterminable."""
    import re

    import httpx
    m = re.search(r"github\.com[:/]([^/]+)/([^/.]+)", repo_url)
    if not m:
        return None
    try:
        r = httpx.get(f"https://api.github.com/repos/{m[1]}/{m[2]}/pulls/{pr}", timeout=15,
                      headers={"Accept": "application/vnd.github+json"})
        if r.status_code != 200:
            return None
        return r.json().get("state") == "open"
    except httpx.HTTPError:
        return None


def prune(name: str, everything: bool = False) -> dict[int, str]:
    prj, h = config.project(name), config.host()
    out = {}
    for pr in list_previews(name):
        if everything or _pr_open(prj.repo, pr) is False:
            down(Target(name, prj, h, pr))
            out[pr] = "supprimée"
    return out


def workflow_snippet(name: str) -> str:
    h = config.host()
    return f"""# .github/workflows/preview.yml  (à committer dans le repo du projet)
name: preview
on:
  pull_request:
    types: [opened, synchronize, reopened, closed]
jobs:
  preview:
    uses: AnMaLeNo/infra/.github/workflows/preview.yml@main
    with:
      project: {name}
    secrets:
      TS_OAUTH_CLIENT_ID: ${{{{ secrets.TS_OAUTH_CLIENT_ID }}}}
      TS_OAUTH_SECRET: ${{{{ secrets.TS_OAUTH_SECRET }}}}
      INFRA_HOST: ${{{{ secrets.INFRA_HOST }}}}
      INFRA_SSH_KEY: ${{{{ secrets.INFRA_SSH_KEY }}}}

# Secrets GitHub à créer sur le repo du projet :
#   TS_OAUTH_CLIENT_ID / TS_OAUTH_SECRET : client OAuth Tailscale (scope auth_keys, tag:ci)
#   INFRA_HOST   = {h.tailscale_ip}
#   INFRA_SSH_KEY = clé privée du compte ai-github (sa clé publique est dans <config>/agents/ai-github.pub)
"""

"""Commit + push automatiques du repo de config après chaque run : tout ce que Claude ou l'outil écrit
(registry.yaml, shared.yaml, shared/<S>/, host.yaml…) est versionné sans geste humain, avec l'historique complet.
Le push passe par https + GITHUB_TOKEN (secrets/infra.env) même si le remote est en ssh (voir compose.git_env)."""
from __future__ import annotations

import os
import subprocess

from . import compose, config, paths, secrets


def _git(args: list[str], env: dict[str, str]) -> subprocess.CompletedProcess:
    # safe.directory explicite : le repo de config appartient à root et l'UI tourne en ai-infra dans un conteneur
    # (sans le /etc/gitconfig de l'hôte) → sinon « dubious ownership » et aucun commit depuis le front.
    cfg = str(paths.config_dir())
    return subprocess.run(["git", "-c", f"safe.directory={cfg}", "-C", cfg, *args], env=env, capture_output=True, text=True, check=False)


def commit_push(message: str) -> dict:
    """`git add -A` + commit + push du repo de config. Idempotent : rien à commiter → {"committed": False}.
    N'échoue jamais bruyamment (le run est déjà fini) : retourne ok/erreur, token masqué."""
    if not (paths.config_dir() / ".git").exists():
        return {"ok": False, "committed": False, "error": "le dossier de config n'est pas un dépôt git"}
    ienv = secrets.infra_env()
    env = {**os.environ, **compose.git_env()}
    host = config.host().name
    identity = ["-c", f"user.name=infra ({host})", "-c", f"user.email=infra@{host}.invalid"]
    out: dict = {"ok": True, "committed": False, "pushed": False}
    r = _git(["add", "-A"], env)
    if r.returncode != 0:
        return out | {"ok": False, "error": secrets.mask(r.stderr.strip(), list(ienv.values()))}
    if _git(["diff", "--cached", "--quiet"], env).returncode == 0:
        return out
    r = _git([*identity, "commit", "-q", "-m", message], env)
    if r.returncode != 0:
        return out | {"ok": False, "error": secrets.mask((r.stderr or r.stdout).strip(), list(ienv.values()))}
    out["committed"] = True
    out["commit"] = _git(["rev-parse", "--short", "HEAD"], env).stdout.strip()
    r = _git(["push", "-q", "origin", "HEAD"], env)
    if r.returncode != 0:
        return out | {"ok": False, "error": "commit local fait mais push refusé : " + secrets.mask(r.stderr.strip()[-300:], list(ienv.values()))}
    out["pushed"] = True
    return out

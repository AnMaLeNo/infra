"""Éditeur web (code-server + extension Claude Code) pour reprendre la session Claude d'un run.

Il tourne sous le même utilisateur (ai-infra) et ouvre le même dossier (config_dir) que les runs : leurs sessions
sont donc dans l'historique de l'extension. Même token Claude et même accès git que le runner. Il n'écoute que sur
l'IP de l'hôte vue depuis traefik-net : Traefik le sert en privé (`editor.<private_zone>`, tailnet), jamais en public.
"""
from __future__ import annotations

import os
import shutil

from . import compose, paths, secrets, stack

PORT = 8443


def command() -> tuple[list[str], dict[str, str]]:
    cs = shutil.which("code-server")
    if not cs:
        raise RuntimeError("`code-server` introuvable dans le PATH (voir install.sh)")
    env = {**os.environ, **compose.git_env()}
    token = secrets.infra_env().get("CLAUDE_CODE_OAUTH_TOKEN")
    if token:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = token
    return [cs, "--bind-addr", f"{stack.network_gateway()}:{PORT}", str(paths.config_dir())], env


def serve() -> None:
    argv, env = command()
    os.chdir(paths.config_dir())
    os.execve(argv[0], argv, env)

"""Éditeur web (VS Code de Microsoft, `code serve-web`) pour reprendre la session Claude d'un run.

Il tourne sous le même utilisateur (ai-infra) et ouvre le même dossier (config_dir) que les runs : leurs sessions
sont donc dans l'historique de l'extension Claude Code. Même token Claude et même accès git que le runner. Il n'écoute que
sur l'IP de l'hôte vue depuis traefik-net : Traefik le sert en privé (`editor.<private_zone>`, tailnet), jamais en public.
Accès protégé par un token de connexion (fichier 0600, généré au premier lancement) : ouvrir une fois `…/?tkn=<token>`.
"""
from __future__ import annotations

import os
import secrets as pysecrets
import shutil
from pathlib import Path

from . import compose, paths, secrets, stack

PORT = 8443
CLI = "vscode-cli"


def token_file() -> Path:
    return Path.home() / ".config" / "infra-editor" / "token"


def _token() -> Path:
    f = token_file()
    if not f.exists():
        f.parent.mkdir(parents=True, exist_ok=True)
        f.touch(mode=0o600)
        f.write_text(pysecrets.token_urlsafe(24))
    return f


def command() -> tuple[list[str], dict[str, str]]:
    cli = shutil.which(CLI)
    if not cli:
        raise RuntimeError(f"`{CLI}` introuvable dans le PATH (CLI VS Code standalone, voir install.sh)")
    env = {**os.environ, **compose.git_env()}
    token = secrets.infra_env().get("CLAUDE_CODE_OAUTH_TOKEN")
    if token:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = token
    return [cli, "serve-web", "--host", stack.network_gateway(), "--port", str(PORT),
            "--connection-token-file", str(_token()), "--default-folder", str(paths.config_dir()),
            "--accept-server-license-terms", "--disable-telemetry"], env


def serve() -> None:
    argv, env = command()
    os.chdir(paths.config_dir())
    os.execve(argv[0], argv, env)

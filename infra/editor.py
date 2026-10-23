"""Éditeur web (VS Code de Microsoft, `code serve-web`) pour reprendre la session Claude d'un run.

Il tourne sous le même utilisateur (ai-infra) et ouvre le même dossier (config_dir) que les runs : leurs sessions
sont donc dans l'historique de l'extension Claude Code, installée au démarrage. Même token Claude et même accès git que
le runner. Il n'écoute que sur l'IP de l'hôte vue depuis traefik-net : Traefik le sert en privé (`editor.<private_zone>`,
tailnet), jamais en public. Accès protégé par un token de connexion (fichier 0600, généré au premier lancement) :
ouvrir une fois `…/?tkn=<token>`.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

import httpx

from . import compose, paths, secrets, stack

PORT = 8443
CLI = "vscode-cli"
EXTENSION = "anthropic.claude-code"


def data_dir() -> Path:
    """Fixé explicitement : le serveur VS Code téléchargé et sa CLI (l'emplacement par défaut dépend de la version)."""
    return Path.home() / ".local" / "share" / "infra-editor"


def token_file() -> Path:
    f = Path.home() / ".config" / "infra-editor" / "token"
    if not f.exists():
        f.parent.mkdir(parents=True, exist_ok=True)
        f.touch(mode=0o600)
        f.write_text(os.urandom(24).hex())
    return f


def command(host: str) -> tuple[list[str], dict[str, str]]:
    cli = shutil.which(CLI)
    if not cli:
        raise RuntimeError(f"`{CLI}` introuvable dans le PATH (CLI VS Code standalone, voir install.sh)")
    env = {**os.environ, **compose.git_env()}
    token = secrets.infra_env().get("CLAUDE_CODE_OAUTH_TOKEN")
    if token:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = token
    return [cli, "serve-web", "--host", host, "--port", str(PORT), "--connection-token-file", str(token_file()),
            "--cli-data-dir", str(data_dir()), "--default-folder", str(paths.config_dir()),
            "--accept-server-license-terms", "--disable-telemetry"], env


def install_extension(host: str, env: dict[str, str], tries: int = 120) -> bool:
    """Le serveur VS Code ne se télécharge qu'à la première requête (202 tant que ça dure) : on la fait, puis on
    installe/met à jour l'extension avec sa CLI. Idempotent."""
    url = f"http://{host}:{PORT}/?tkn={token_file().read_text()}"
    for _ in range(tries):
        try:
            ready = httpx.get(url, timeout=5).status_code != 202
        except httpx.HTTPError:
            ready = False
        servers = sorted((data_dir() / "serve-web").glob("*/bin/code-server"),
                         key=lambda p: p.stat().st_mtime)
        if ready and servers:
            return subprocess.run([str(servers[-1]), "--install-extension", EXTENSION], env=env, check=False).returncode == 0
        time.sleep(5)
    return False


def serve() -> None:
    host = stack.network_gateway()
    argv, env = command(host)
    server = subprocess.Popen(argv, env=env)
    threading.Thread(target=lambda: install_extension(host, env) or print("extension non installée", file=sys.stderr),
                     daemon=True).start()
    raise SystemExit(server.wait())

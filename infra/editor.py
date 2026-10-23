"""Éditeur web (VS Code de Microsoft, `code serve-web`) pour reprendre la session Claude d'un run.

Il tourne sous le même utilisateur (ai-infra) et ouvre le même dossier (config_dir) que les runs : leurs sessions
sont donc dans l'historique de l'extension Claude Code, installée au démarrage avec une petite extension à nous
(`editor_ext/`) qui ouvre la session demandée par le front. Même token Claude et même accès git que
le runner. Il n'écoute que sur l'IP de l'hôte vue depuis traefik-net : Traefik le sert en privé (`editor.<private_zone>`,
tailnet), jamais en public. Accès protégé par un token de connexion (fichier 0600, généré au premier lancement) :
ouvrir une fois `…/?tkn=<token>`.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import threading
import time
import uuid
import zipfile
from pathlib import Path

import httpx

from . import compose, paths, secrets, stack

PORT = 8443
CLI = "vscode-cli"
EXTENSION = "anthropic.claude-code"
# L'extension n'affiche dans « Session history » les sessions lancées par `claude -p` que si `claudeProcessWrapper` est
# défini (sinon elle ne montre que celles créées dans VS Code). `env` exécute simplement le binaire embarqué qu'on lui passe.
SETTINGS = {"claudeCode.claudeProcessWrapper": "/usr/bin/env"}
# Petite extension à nous (editor_ext/) : au démarrage de VS Code, ouvre la session déposée par `request_session`.
OPEN_FILE = ".editor-open"


def data_dir() -> Path:
    """Fixé explicitement : le serveur VS Code téléchargé et sa CLI (l'emplacement par défaut dépend de la version)."""
    return Path.home() / ".local" / "share" / "infra-editor"


def server_dir() -> Path:
    return data_dir() / "server"


def configure() -> None:
    f = server_dir() / "data" / "Machine" / "settings.json"
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps({**(json.loads(f.read_text()) if f.exists() else {}), **SETTINGS}, indent=2))


def request_session(session_id: str) -> None:
    """Demande à l'éditeur d'ouvrir cette session Claude au prochain démarrage d'une fenêtre (bouton du front)."""
    f = paths.runs_dir() / OPEN_FILE
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(str(uuid.UUID(session_id)))


def build_vsix() -> Path:
    src = Path(__file__).parent / "editor_ext"
    out = data_dir() / "infra-open-session.vsix"
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w") as z:
        for f in sorted(src.rglob("*")):
            if f.is_file():
                z.write(f, f.relative_to(src).as_posix())
    return out


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
            "--cli-data-dir", str(data_dir()), "--server-data-dir", str(server_dir()), "--default-folder", str(paths.config_dir()),
            "--accept-server-license-terms", "--disable-telemetry"], env


def install_extension(host: str, env: dict[str, str], tries: int = 120) -> bool:
    """Le serveur VS Code ne se télécharge qu'à la première requête (202 tant que ça dure) : on la fait, puis on
    installe/met à jour l'extension Claude Code et la nôtre avec sa CLI. Idempotent."""
    url = f"http://{host}:{PORT}/?tkn={token_file().read_text()}"
    for _ in range(tries):
        try:
            ready = httpx.get(url, timeout=5).status_code != 202
        except httpx.HTTPError:
            ready = False
        servers = sorted((data_dir() / "serve-web").glob("*/bin/code-server"),
                         key=lambda p: p.stat().st_mtime)
        if ready and servers:
            cli = [str(servers[-1]), "--server-data-dir", str(server_dir())]
            return all(subprocess.run([*cli, *args], env=env, check=False).returncode == 0
                       for args in (["--install-extension", EXTENSION],
                                    ["--install-extension", str(build_vsix()), "--force"]))   # --force : même version, code à jour
        time.sleep(5)
    return False


def serve() -> None:
    host = stack.network_gateway()
    argv, env = command(host)
    configure()
    server = subprocess.Popen(argv, env=env)
    threading.Thread(target=lambda: install_extension(host, env) or print("extension non installée", file=sys.stderr),
                     daemon=True).start()
    raise SystemExit(server.wait())

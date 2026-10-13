"""Stacks d'infra partagées (tool/stack/<nom>) : traefik, cloudflared, ui, mosquitto…"""
from __future__ import annotations

import os
from pathlib import Path

from . import __version__, config, paths, secrets
from .util import dump_dotenv, run, run_json, write_private

REQUIRED = {"traefik", "ui"}


def stack_dir(name: str) -> Path:
    d = paths.tool_dir() / "stack" / name
    if not d.exists():
        raise FileNotFoundError(f"stack inconnue : {name}")
    return d


def state_dir(name: str) -> Path:
    d = paths.stack_state_dir() / name
    d.mkdir(parents=True, exist_ok=True)
    return d


def ensure_network() -> None:
    nets = run(["docker", "network", "ls", "--format", "{{.Name}}"], quiet=True).stdout.split()
    if "traefik-net" not in nets:
        run(["docker", "network", "create", "traefik-net"], quiet=True)


def stack_env(name: str) -> dict[str, str]:
    h = config.host()
    ienv = secrets.infra_env()
    env = {
        "TAILSCALE_IP": h.tailscale_ip,
        "PRIVATE_ZONE": h.private_zone,
        "PUBLIC_ZONE": h.public_zone,
        "PUBLIC_INGRESS": h.public_ingress,
        "STACK_STATE": str(state_dir(name)),
        "ACME_EMAIL": ienv.get("ACME_EMAIL", ""),
        "CF_API_TOKEN": ienv.get("CF_API_TOKEN", ""),
        "TUNNEL_ID": h.cloudflare.tunnel_id,
        "INFRA_VERSION": os.environ.get("INFRA_VERSION", __version__),
    }
    return env


def prepare(name: str) -> None:
    sd = state_dir(name)
    if name == "traefik":
        acme = sd / "acme.json"
        if not acme.exists():
            acme.write_text("{}")
        os.chmod(acme, 0o600)
    if name == "cloudflared":
        if not secrets.exists("cloudflared.json"):
            raise RuntimeError("secrets/cloudflared.json absent : lance `infra tunnel create`")
        write_private(sd / "credentials.json", secrets.decrypt("cloudflared.json"))


def compose_cmd(name: str) -> list[str]:
    d = stack_dir(name)
    env_file = state_dir(name) / ".env"
    write_private(env_file, dump_dotenv(stack_env(name)))
    cmd = ["docker", "compose", "-p", f"infra-{name}", "--env-file", str(env_file), "-f", str(d / "docker-compose.yml")]
    if name == "traefik" and config.host().public_ingress == "direct":
        cmd += ["-f", str(d / "docker-compose.direct.yml")]
    return cmd


def up(name: str) -> None:
    ensure_network()
    prepare(name)
    run([*compose_cmd(name), "up", "-d", "--remove-orphans", "--quiet-pull"], capture=False)


def down(name: str) -> None:
    run([*compose_cmd(name), "down", "--remove-orphans"], capture=False)


def status(name: str) -> dict:
    rows = run_json(["docker", "compose", "-p", f"infra-{name}", "ps", "-a", "--format", "json"])
    rows = rows if isinstance(rows, list) else [rows]
    return {c["Service"]: (c.get("Health") or c.get("State")) for c in rows}


def all_names() -> list[str]:
    h = config.host()
    return sorted(set(h.stacks) | REQUIRED, key=lambda n: ["traefik", "cloudflared", "ui"].index(n) if n in ("traefik", "cloudflared", "ui") else 9)

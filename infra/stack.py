"""Stacks d'infra partagées (tool/stack/<nom>) : traefik, cloudflared, ui, mosquitto…"""
from __future__ import annotations

import grp
import os
import pwd
import re
from pathlib import Path

from . import __version__, config, paths, secrets
from .util import dump_dotenv, run, run_json, write_private

REQUIRED = {"traefik", "ui"}


NETWORK = "traefik-net"
NETWORK_SUBNET = "172.30.0.0/24"  # fixe à la création : la passerelle sert aux ponts vers l'hôte


def stack_dir(name: str) -> Path:
    """Stack de l'hôte (<config>/stack/<nom>) prioritaire sur celle de l'outil (tool/stack/<nom>)."""
    for d in (paths.config_dir() / "stack" / name, paths.tool_dir() / "stack" / name):
        if (d / "docker-compose.yml").exists():
            return d
    raise FileNotFoundError(f"stack inconnue : {name}")


def state_dir(name: str) -> Path:
    d = paths.stack_state_dir() / name
    d.mkdir(parents=True, exist_ok=True)
    return d


def ensure_network() -> None:
    nets = run(["docker", "network", "ls", "--format", "{{.Name}}"], quiet=True).stdout.split()
    if NETWORK not in nets:
        run(["docker", "network", "create", "--subnet", NETWORK_SUBNET, NETWORK], quiet=True)


def network_gateway() -> str:
    """IP de l'hôte vue depuis traefik-net (cible des services legacy qui n'écoutent que sur l'hôte)."""
    ensure_network()
    out = run(["docker", "network", "inspect", NETWORK, "-f",
               "{{(index .IPAM.Config 0).Gateway}}"], quiet=True).stdout.strip()
    return out or NETWORK_SUBNET.rsplit(".", 1)[0] + ".1"


def _uid(user: str) -> str:
    try:
        return str(pwd.getpwnam(user).pw_uid)
    except KeyError:
        return "0"


def _gid(group: str) -> str:
    try:
        return str(grp.getgrnam(group).gr_gid)
    except KeyError:
        return "0"


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
        "TRAEFIK_NET_GATEWAY": network_gateway(),
        "INFRA_VERSION": os.environ.get("INFRA_VERSION", __version__),
        # l'UI écrit registry.yaml et runs/queue : même identité que le runner, sinon les fichiers
        # créés par root dans le conteneur sont illisibles/insupprimables pour ai-infra
        "INFRA_UID": _uid("ai-infra"),
        "INFRA_GID": _gid("infra"),
        "DOCKER_GID": _gid("docker"),
    }
    return env


def prepare(name: str) -> bool:
    """Rend les fichiers de la stack dans STACK_STATE. Retourne True si un fichier a changé : les conteneurs
    ne voient pas un fichier remplacé (rename) derrière un bind mount, il faut alors les recréer."""
    changed = False
    sd = state_dir(name)
    if name == "traefik":
        acme = sd / "acme.json"
        if not acme.exists():
            acme.write_text("{}")
        os.chmod(acme, 0o600)
        (sd / "certs").mkdir(exist_ok=True)
        _sync_dynamic(sd / "dynamic")
        src = stack_dir("traefik") / f"traefik.{config.host().public_ingress}.yml"
        changed |= write_private(sd / "traefik.yml", render_static(src.read_text(), stack_env("traefik")), mode=0o644)
    if name == "cloudflared":
        if not secrets.exists("cloudflared.json"):
            raise RuntimeError("secrets/cloudflared.json absent : lance `infra tunnel create`")
        creds = sd / "credentials.json"
        changed |= write_private(creds, secrets.decrypt("cloudflared.json"), mode=0o400)
        os.chown(creds, CLOUDFLARED_UID, 0)
        src = stack_dir("cloudflared") / "config.yml"
        changed |= write_private(sd / "config.yml", render_static(src.read_text(), stack_env("cloudflared")), mode=0o644)
    return changed


# l'image officielle cloudflared tourne en `nonroot` (uid 65532) : le fichier de credentials doit lui appartenir
CLOUDFLARED_UID = 65532

_ENV_TPL = re.compile(r"\{\{\s*env\s+`([A-Z0-9_]+)`\s*\}\}")


def render_static(text: str, env: dict[str, str]) -> str:
    """Config statique Traefik et config.yml de cloudflared ne sont templatées ni par Traefik (seule la
    dynamique l'est) ni par cloudflared (`${X}` est pris littéralement) : on remplace nous-mêmes
    les `{{ env `X` }}` avant d'écrire le fichier dans STACK_STATE."""
    return _ENV_TPL.sub(lambda m: env.get(m.group(1), ""), text)


def _sync_dynamic(dst: Path) -> None:
    """Config dynamique Traefik servie = fichiers de l'outil + ceux de l'hôte (<config>/stack/traefik/dynamic)."""
    dst.mkdir(exist_ok=True)
    srcs = [paths.tool_dir() / "stack" / "traefik" / "dynamic",
            paths.config_dir() / "stack" / "traefik" / "dynamic"]
    wanted = {}
    for src in srcs:
        if src.is_dir():
            for f in sorted(src.glob("*.yml")):
                wanted[f.name] = f.read_text()
    for old in dst.glob("*.yml"):
        if old.name not in wanted:
            old.unlink()
    for fname, content in wanted.items():
        write_private(dst / fname, content, mode=0o644)


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
    recreate = ["--force-recreate"] if prepare(name) else []
    run([*compose_cmd(name), "up", "-d", "--remove-orphans", "--quiet-pull", *recreate], capture=False)


def down(name: str) -> None:
    run([*compose_cmd(name), "down", "--remove-orphans"], capture=False)


def status(name: str) -> dict:
    rows = run_json(["docker", "compose", "-p", f"infra-{name}", "ps", "-a", "--format", "json"])
    rows = rows if isinstance(rows, list) else [rows]
    return {c["Service"]: (c.get("Health") or c.get("State")) for c in rows}


def all_names() -> list[str]:
    h = config.host()
    return sorted(set(h.stacks) | REQUIRED, key=lambda n: ["traefik", "cloudflared", "ui"].index(n) if n in ("traefik", "cloudflared", "ui") else 9)

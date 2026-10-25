"""Génère compose.override.yml (+ compose complet pour `static`) et le .env déchiffré d'un projet ou d'une preview."""
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

import yaml

from . import config, paths, secrets
from .models import Host, Project
from .util import dump_dotenv, parse_dotenv, run, write_private


class ResetList(list):
    """Sérialisé en `!reset []` : compose v2.24+ vide la liste héritée (ports publiés)."""


class _Dumper(yaml.SafeDumper):
    pass


_Dumper.add_representer(ResetList, lambda d, data: d.represent_sequence("!reset", []))


@dataclass
class Target:
    """Un déploiement concret : le projet lui-même ou une de ses previews."""
    name: str
    project: Project
    host: Host
    pr: int | None = None

    @property
    def compose_project(self) -> str:
        return self.name if self.pr is None else f"{self.name}-pr-{self.pr}"

    @property
    def dir(self) -> Path:
        return paths.app_dir(self.name) if self.pr is None else paths.preview_dir(self.name, self.pr)

    @property
    def repo_dir(self) -> Path:
        return self.dir / "repo"

    @property
    def domain(self) -> str:
        return config.domain_of(self.name, self.project)

    def _fqdn(self, zone: str) -> str:
        base = f"{self.domain}.{zone}"
        return base if self.pr is None else f"pr-{self.pr}.{base}"

    @property
    def expose(self) -> str:
        return self.project.expose if self.pr is None else self.project.previews

    @property
    def private_fqdn(self) -> str:
        return self._fqdn(self.host.private_zone)

    @property
    def public_fqdn(self) -> str:
        return self._fqdn(self.host.public_zone)

    @property
    def urls(self) -> dict[str, str]:
        out = {}
        if self.expose in ("private", "public"):
            out["private"] = f"https://{self.private_fqdn}"
        if self.expose == "public":
            out["public"] = f"https://{self.public_fqdn}"
        return out

    @property
    def compose_file(self) -> Path:
        if self.project.service == "static":
            return self.dir / "compose.static.yml"
        return self.repo_dir / self.project.compose_file

    @property
    def override_file(self) -> Path:
        return self.dir / "compose.override.yml"

    @property
    def env_file(self) -> Path:
        return self.dir / ".env"


def load_compose(t: Target) -> dict:
    if not t.compose_file.exists():
        raise FileNotFoundError(f"compose introuvable : {t.compose_file}")
    return yaml.safe_load(t.compose_file.read_text()) or {}


def traefik_labels(t: Target) -> list[str]:
    if t.expose == "none":
        return []
    rn = t.compose_project
    labels = [
        "traefik.enable=true",
        "traefik.docker.network=traefik-net",
        f"traefik.http.routers.{rn}-private.rule=Host(`{t.private_fqdn}`)",
        f"traefik.http.routers.{rn}-private.entrypoints=tailnet",
        f"traefik.http.routers.{rn}-private.tls=true",
        f"traefik.http.routers.{rn}-private.tls.certresolver=cf",
        f"traefik.http.routers.{rn}-private.service={rn}",
    ]
    if t.expose == "public":
        if t.host.public_ingress == "cloudflare-tunnel":
            labels += [
                f"traefik.http.routers.{rn}-public.rule=Host(`{t.public_fqdn}`)",
                f"traefik.http.routers.{rn}-public.entrypoints=tunnel",
                f"traefik.http.routers.{rn}-public.service={rn}",
            ]
        else:
            labels += [
                f"traefik.http.routers.{rn}-public.rule=Host(`{t.public_fqdn}`)",
                f"traefik.http.routers.{rn}-public.entrypoints=websecure",
                f"traefik.http.routers.{rn}-public.tls=true",
                f"traefik.http.routers.{rn}-public.tls.certresolver=cf",
                f"traefik.http.routers.{rn}-public.service={rn}",
            ]
    port = 80 if t.project.service == "static" else t.project.port
    if port is None:
        raise ValueError(f"{t.name}: `port` manquant dans le registre (port d'écoute interne du container)")
    labels.append(f"traefik.http.services.{rn}.loadbalancer.server.port={port}")
    return labels


def _with_traefik_net(orig_nets):
    if isinstance(orig_nets, dict):
        return {**orig_nets, "traefik-net": None}
    if isinstance(orig_nets, list):
        return [*orig_nets, "traefik-net"] if "traefik-net" not in orig_nets else orig_nets
    return ["default", "traefik-net"]


def build_override(t: Target, compose: dict) -> dict:
    services = compose.get("services") or {}
    if not services:
        raise ValueError(f"{t.name}: aucun service dans {t.compose_file}")
    container = "web" if t.project.service == "static" else t.project.container
    if t.expose != "none":
        if container is None:
            if len(services) == 1:
                container = next(iter(services))
            else:
                raise ValueError(f"{t.name}: `container` manquant ; services disponibles : {sorted(services)}")
        if container not in services:
            raise ValueError(f"{t.name}: container `{container}` absent du compose ; services : {sorted(services)}")

    ov: dict = {"services": {}}
    for svc in services:
        ov["services"][svc] = {"ports": ResetList(), "restart": "unless-stopped"}
        if t.project.env and t.env_file.exists():
            ov["services"][svc]["env_file"] = [str(t.env_file)]

    for spec in t.project.data:
        svc, _, cpath = spec.partition(":")
        if svc not in services or not cpath.startswith("/"):
            raise ValueError(f"{t.name}: data `{spec}` invalide (attendu <service>:/chemin)")
        hpath = paths.data_dir() / t.compose_project / svc / cpath.strip("/").replace("/", "_")
        ov["services"][svc].setdefault("volumes", []).append(f"{hpath}:{cpath}")

    if t.expose != "none":
        s = ov["services"][container]
        s["labels"] = traefik_labels(t)
        s["networks"] = _with_traefik_net(services[container].get("networks"))
        ov["networks"] = {"traefik-net": {"external": True}}

    if t.project.uses:
        declared = config.shared().services
        unknown = [u for u in t.project.uses if u not in declared]
        if unknown:
            raise ValueError(f"{t.name}: `uses` référence des services partagés non déclarés dans shared.yaml : {unknown}. "
                             "Ne les crée pas dans ce projet : l'utilisateur doit les déclarer (front → Services partagés).")
        if not t.project.consumers:
            raise ValueError(f"{t.name}: `uses: {t.project.uses}` mais `consumers` est vide : lis le code du projet et écris via "
                             "registry_set quel(s) service(s) compose consomme(nt) le service partagé et avec quelles variables "
                             "d'env, ex. {\"consumers\": {\"api\": {\"MQTT_HOST\": \"mqtt\", \"MQTT_PORT\": \"1883\"}}}.")
    for svc, env in t.project.consumers.items():
        if svc not in services:
            raise ValueError(f"{t.name}: consumers `{svc}` absent du compose ; services : {sorted(services)}")
        s = ov["services"][svc]
        s["environment"] = {**s.get("environment", {}), **env}
        s["networks"] = _with_traefik_net(services[svc].get("networks"))
        ov["networks"] = {"traefik-net": {"external": True}}

    for spec in t.project.expose_ports:
        port, _, proto = spec.partition("/")
        svc = container or next(iter(services))
        ov["services"][svc]["ports"] = [f"{t.host.tailscale_ip}:{port}:{port}" + (f"/{proto}" if proto else "")]
    return ov


def static_compose(t: Target) -> dict:
    serve_dir = (t.repo_dir / t.project.static_dir).resolve()
    return {
        "services": {
            "web": {
                "image": "nginx:1.27-alpine",
                "restart": "unless-stopped",
                "volumes": [f"{serve_dir}:/usr/share/nginx/html:ro"],
            }
        }
    }


def run_static_build(t: Target) -> None:
    if not t.project.build:
        return
    stamp = t.dir / ".build-commit"
    head = run(["git", "rev-parse", "HEAD"], cwd=t.repo_dir, quiet=True).stdout.strip()
    if stamp.exists() and stamp.read_text().strip() == head and (t.repo_dir / t.project.static_dir).exists():
        return
    run(["docker", "run", "--rm", "-v", f"{t.repo_dir}:/app", "-w", "/app", "node:22-bookworm",
         "sh", "-lc", t.project.build], capture=False)
    stamp.write_text(head + "\n")


def render_env(t: Target) -> list[str]:
    """Déchiffre le .env ; retourne les valeurs (pour masquage). Supprime le fichier si env: false."""
    if not t.project.env:
        t.env_file.unlink(missing_ok=True)
        return []
    env = secrets.dotenv(secrets.project_env_path(t.name))
    if t.pr is not None:
        prev = secrets.project_env_path(t.name, preview=True)
        if secrets.exists(prev):
            env.update(secrets.dotenv(prev))
        env.update({"PR_NUMBER": str(t.pr), "PR_DOMAIN": t.public_fqdn if t.expose == "public" else t.private_fqdn})
    write_private(t.env_file, dump_dotenv(env))
    return list(env.values())


def render(t: Target) -> bool:
    """Écrit .env + override (+ compose static). Retourne True si quelque chose a changé."""
    t.dir.mkdir(parents=True, exist_ok=True)
    changed = False
    before = t.env_file.read_text() if t.env_file.exists() else None
    render_env(t)
    after = t.env_file.read_text() if t.env_file.exists() else None
    changed |= before != after
    if t.project.service == "static":
        run_static_build(t)
        changed |= write_private(t.compose_file, yaml.safe_dump(static_compose(t), sort_keys=False), 0o644)
    compose = load_compose(t)
    ov = build_override(t, compose)
    text = "# Généré par `infra render` — ne pas éditer.\n" + yaml.dump(ov, Dumper=_Dumper, sort_keys=False)
    changed |= write_private(t.override_file, text, 0o644)
    return changed


def missing_env_vars(t: Target) -> list[str]:
    """Variables ${VAR} référencées par le compose sans valeur dans .env ni défaut."""
    import re
    text = t.compose_file.read_text()
    env = parse_dotenv(t.env_file.read_text()) if t.env_file.exists() else {}
    missing = []
    for m in re.finditer(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:?[-?+][^}]*)?\}", text):
        var, default = m.group(1), m.group(2)
        if var not in env and not (default and default.startswith((":-", "-"))):
            missing.append(var)
    return sorted(set(missing))

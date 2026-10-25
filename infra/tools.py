"""Tools atomiques exposés au LLM (MCP) et au CLI. Chaque fonction fait UNE chose, est idempotente,
et lève une erreur explicite quand elle ne s'applique pas (le LLM fait alors autrement)."""
from __future__ import annotations

import json
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from . import compose, config, dns, paths, secrets, stack
from .render import Target, missing_env_vars, render_env, traefik_labels
from .render import render as _render


def _target(project: str, pr: int | None = None) -> Target:
    return Target(project, config.project(project), config.host(), pr)


def host_info() -> dict:
    """Décrit l'hôte : host.yaml (IP Tailscale, zones DNS, mode d'ingress public, tunnel, stacks, exceptions), chemins conventionnels."""
    h = config.host()
    return {
        "host": h.model_dump(),
        "paths": {"config": str(paths.config_dir()), "tool": str(paths.tool_dir()), "apps": str(paths.apps_dir()),
                  "data": str(paths.data_dir()), "stack_state": str(paths.stack_state_dir())},
        "conventions": str(paths.tool_dir() / "docs" / "conventions.md"),
    }


def registry_get(project: str | None = None) -> dict:
    """Lit registry.yaml : l'entrée d'un projet, ou toutes."""
    reg = config.registry()
    if project:
        return {project: config.project(project).model_dump()}
    return {n: p.model_dump() for n, p in reg.projects.items()}


def registry_set(project: str, fields: dict) -> dict:
    """Modifie des champs d'un projet dans registry.yaml. C'est ainsi que tu consignes tes constats (`service` si absent, `container`, `port`, `compose_file`, `build`, `static_dir`), ex. {"container": "web", "port": 8420}. Valeur null = supprimer le champ."""
    return {project: config.set_project_fields(project, **fields).model_dump(exclude_defaults=True)}


def repo_sync(project: str, pr: int | None = None) -> dict:
    """Clone (première fois) ou met à jour le repo du projet au bon endroit (/srv/apps/<P>/repo, ou previews/pr-<N>/repo), sur `ref` du registre (ou la tête de la PR). Retourne le chemin et le commit."""
    t = _target(project, pr)
    commit = compose.fetch(t)
    files = sorted(p.name for p in t.repo_dir.iterdir() if not p.name.startswith(".git"))
    return {"path": str(t.repo_dir), "commit": commit, "ref": t.project.ref if pr is None else f"refs/pull/{pr}/head",
            "has_compose": (t.repo_dir / t.project.compose_file).exists(), "has_dockerfile": (t.repo_dir / "Dockerfile").exists(),
            "top_level": files[:50]}


def compose_override(project: str, pr: int | None = None) -> dict:
    """Génère /srv/apps/<P>/compose.override.yml (ports publiés neutralisés, restart, labels Traefik + réseau traefik-net sur `container`, bind mounts `data:`, env_file si env:true) et le .env déchiffré. Pour `service: static`, génère aussi compose.static.yml (nginx) après le `build:`. Échoue si le repo n'a pas de compose, si `container`/`port` manquent (lis le compose/Dockerfile et fixe-les d'abord via registry_set) ou si le container n'existe pas. Retourne le contenu généré, la commande `docker compose` à utiliser et les variables manquantes."""
    t = _target(project, pr)
    if not t.repo_dir.exists():
        raise FileNotFoundError(f"repo non synchronisé : appelle repo_sync({project!r}) d'abord")
    if t.project.service != "static" and not t.compose_file.exists():
        raise FileNotFoundError(f"pas de {t.project.compose_file} dans {t.repo_dir} : ce projet n'est pas un `service: compose`. "
                                "Écris un Dockerfile + docker-compose.yml (PR sur le repo, voir migrate-legacy) ou passe en `service: static`.")
    changed = _render(t)
    return {"override": str(t.override_file), "content": t.override_file.read_text(), "changed": changed,
            "env_file": str(t.env_file) if t.env_file.exists() else None, "missing_env": missing_env_vars(t),
            "compose_cmd": " ".join(compose.compose_cmd(t))}


def traefik_labels_tool(project: str, pr: int | None = None) -> dict:
    """Calcule les labels Traefik voulus pour le projet (routeurs private/public selon `expose`, port) sans rien écrire. Utile pour écrire un override à la main."""
    t = _target(project, pr)
    return {"labels": traefik_labels(t), "networks": ["traefik-net"], "urls": t.urls}


def env_decrypt(project: str, pr: int | None = None) -> dict:
    """Déchiffre secrets/projects/<P>.env vers /srv/apps/<P>/.env (0600). Retourne uniquement les NOMS de variables. Supprime le fichier si env: false."""
    t = _target(project, pr)
    t.dir.mkdir(parents=True, exist_ok=True)
    if not t.project.env:
        render_env(t)
        return {"env_file": None, "note": "env: false dans le registre ; aucun .env écrit"}
    render_env(t)
    return {"env_file": str(t.env_file), "keys": secrets.keys(secrets.project_env_path(project))}


def secrets_keys_tool(rel: str) -> dict:
    """Noms des variables d'un secret chiffré (ex. 'projects/catacombes.env', 'infra.env'). Jamais les valeurs."""
    return {"file": f"secrets/{rel}", "keys": secrets.keys(rel)}


def project_status(project: str) -> dict:
    """État réel d'un projet : containers et santé, ports publiés, présence dans traefik-net, URLs attendues, commit déployé, et l'état voulu du registre."""
    t = _target(project)
    s = compose.status(t)
    s["wanted"] = t.project.model_dump(exclude_defaults=True)
    s["paths"] = {"dir": str(t.dir), "repo": str(t.repo_dir), "override": str(t.override_file), "data": str(paths.data_dir() / project)}
    return s


def dns_ensure(project: str, force: bool = False) -> dict:
    """Crée/corrige les enregistrements DNS voulus pour le projet d'après `expose`/`previews` (CNAME vers le tunnel ou A vers l'IP publique ; wildcards previews) et supprime ceux devenus inutiles. Retourne ok|created|updated|deleted|conflict par nom. `conflict` = un enregistrement existe avec une autre cible : vérifie avant de relancer avec force=True."""
    return dns.ensure_project(project, config.project(project), config.host(), force=force)


def dns_prune(project: str, public_only: bool = False) -> dict:
    """Supprime les enregistrements DNS du projet (tous, ou seulement le CNAME public)."""
    return dns.prune_project(project, config.project(project), config.host(), public_only=public_only)


def dns_list() -> list[dict]:
    """Tous les enregistrements de la zone publique (type, nom, cible, proxied)."""
    return dns.list_records(config.host())


def stack_status() -> dict:
    """État des stacks d'infra (traefik, cloudflared, ui, …)."""
    return {n: stack.status(n) for n in stack.all_names()}


def host_addresses() -> dict:
    """Adresses de l'hôte pour décider où un service écoute : IP Tailscale, IP LAN déclarée (host.yaml: lan_ip) et détectée (route par défaut), passerelle traefik-net. Un service partagé n'écoute jamais sur 0.0.0.0 : il lie explicitement ces IPs."""
    h = config.host()
    detected = None
    try:
        out = subprocess.run(["ip", "-4", "route", "get", "1.1.1.1"], capture_output=True, text=True, check=False).stdout.split()
        detected = out[out.index("src") + 1] if "src" in out else None
    except (OSError, ValueError):
        pass
    return {"tailscale_ip": h.tailscale_ip, "lan_ip": h.lan_ip, "lan_ip_detected": detected,
            "traefik_net_gateway": stack.network_gateway(),
            "warning": None if not h.lan_ip or h.lan_ip == detected else
            f"host.yaml lan_ip={h.lan_ip} mais l'IP LAN détectée est {detected} : signale-le, ne corrige pas tout seul"}


def shared_get(name: str | None = None) -> dict:
    """Lit shared.yaml : un service partagé (avec `users` = projets qui le déclarent dans `uses:`, calculé) ou tous. Un service partagé n'existe qu'en un exemplaire par hôte ; s'il n'est pas déclaré ici, il ne doit pas être créé (ni dans un projet)."""
    reg = config.shared()
    names = [name] if name else list(reg.services)
    return {n: config.shared_service(n).model_dump() | {"users": config.users_of(n), "dir": str(paths.shared_dir(n)),
                                                         "state": str(paths.shared_state_dir(n))} for n in names}


def shared_set(name: str, fields: dict) -> dict:
    """Consigne tes constats sur un service partagé déjà déclaré (`image`, `container`, `compose_file`, `notes`). Les choix humains (kind, listen, port, auth, data) ne se changent pas ici : termine `bloqué` avec la question. Valeur null = supprimer."""
    config.shared_service(name)
    human = {"kind", "listen", "port", "auth", "data", "enabled"}
    bad = human & set(fields)
    if bad:
        raise ValueError(f"{sorted(bad)} sont des choix humains : pas via shared_set (demande-les dans needs_human)")
    return {name: config.set_shared_fields(name, "llm", **fields).model_dump(exclude_defaults=True)}


def shared_status(name: str) -> dict:
    """État réel d'un service partagé : conteneurs du projet compose `shared-<nom>`, sockets réellement écoutés sur le port déclaré (ss), alias `<nom>` sur traefik-net, clients TCP connectés, projets consommateurs attendus."""
    svc = config.shared_service(name)
    proj = f"shared-{name}"
    ps = subprocess.run(["docker", "compose", "-p", proj, "ps", "-a", "--format", "json"], capture_output=True, text=True, check=False)
    containers = []
    for line in ps.stdout.splitlines():
        try:
            c = json.loads(line)
        except json.JSONDecodeError:
            continue
        containers.append({"name": c.get("Name"), "service": c.get("Service"), "state": c.get("State"),
                           "health": c.get("Health"), "image": c.get("Image"), "ports": c.get("Publishers") or []})
    aliases = []
    for c in containers:
        ins = subprocess.run(["docker", "inspect", c["name"], "--format", "{{json .NetworkSettings.Networks}}"],
                             capture_output=True, text=True, check=False)
        try:
            nets = json.loads(ins.stdout or "{}")
        except json.JSONDecodeError:
            nets = {}
        aliases += (nets.get(stack.NETWORK) or {}).get("Aliases") or []
    listening, clients = [], None
    if svc.port:
        ss = subprocess.run(["ss", "-Hltn", f"( sport = :{svc.port} )"], capture_output=True, text=True, check=False)
        listening = [line.split()[3] for line in ss.stdout.splitlines() if len(line.split()) > 3]
        est = subprocess.run(["ss", "-Htn", "state", "established", f"( sport = :{svc.port} )"], capture_output=True, text=True, check=False)
        clients = len(est.stdout.splitlines())
    h = config.host()
    wanted = [{"lan": h.lan_ip, "tailnet": h.tailscale_ip}.get(x) for x in svc.listen]
    return {"wanted": svc.model_dump(exclude_defaults=True), "users": config.users_of(name), "containers": containers,
            "alias_ok": name in aliases, "listening": listening,
            "listen_wanted": [f"{ip}:{svc.port}" for ip in wanted if ip] if svc.port else [],
            "on_all_interfaces": any(a.startswith(("0.0.0.0:", "*:", "[::]:")) for a in listening),
            "tcp_clients": clients, "paths": {"dir": str(paths.shared_dir(name)), "state": str(paths.shared_state_dir(name))}}


def skills_list() -> dict:
    """Skills disponibles (chemin → origine tool|custom)."""
    from . import skills
    return skills.list_all()





def report_path() -> Path:
    return paths.runs_dir() / ".report.json"


def run_report(project: str, status: Literal["ok", "bloqué", "échec"], summary: str, needs_human: str = "") -> dict:
    """Clôture ton run : à appeler UNE fois, en dernier. `ok` = état cible atteint et vérifié ; `bloqué` = il manque quelque
    chose qu'un humain doit fournir (dis quoi dans `needs_human`), rien n'est cassé ; `échec` = tu n'as pas réussi ou tu as
    laissé quelque chose en mauvais état. Sans cet appel, le run compte comme échec. `summary` = ce qui a été fait, les URLs,
    ce que tu as dû faire à la main (candidats à devenir des tools)."""
    rep = {"project": project, "status": status, "summary": summary, "needs_human": needs_human,
           "at": datetime.now(UTC).isoformat()}
    p = report_path()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(json.dumps(rep, ensure_ascii=False))
    return rep

"""Tools atomiques exposés au LLM (MCP) et au CLI. Chaque fonction fait UNE chose, est idempotente,
et lève une erreur explicite quand elle ne s'applique pas (le LLM fait alors autrement)."""
from __future__ import annotations

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
    """Modifie des champs d'un projet dans registry.yaml (ex. {"container": "web", "port": 8420}). Valeur null = supprimer le champ."""
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
    """Génère /srv/apps/<P>/compose.override.yml (ports publiés neutralisés, restart, labels Traefik + réseau traefik-net sur `container`, bind mounts `data:`, env_file si env:true) et le .env déchiffré. Pour `service: static`, génère aussi compose.static.yml (nginx) après le `build:`. Échoue si le repo n'a pas de compose, si `container`/`port` manquent ou si le container n'existe pas. Retourne le contenu généré, la commande `docker compose` à utiliser et les variables manquantes."""
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


def skills_list() -> dict:
    """Skills disponibles (chemin → origine tool|custom)."""
    from . import skills
    return skills.list_all()




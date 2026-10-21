"""Opérations git + docker compose sur une cible (projet ou preview)."""
from __future__ import annotations

from . import secrets
from .render import Target
from .util import run, run_json


def git_env() -> dict[str, str]:
    """Env pour git/gh : jamais de prompt interactif, et si `GITHUB_TOKEN` est dans secrets/infra.env,
    authentification https sur github.com via la config git passée en variables (rien n'est écrit sur disque)."""
    env = {"GIT_TERMINAL_PROMPT": "0"}
    token = secrets.infra_env().get("GITHUB_TOKEN")
    if token:
        env.update({
            "GIT_CONFIG_COUNT": "1",
            "GIT_CONFIG_KEY_0": f"url.https://x-access-token:{token}@github.com/.insteadOf",
            "GIT_CONFIG_VALUE_0": "https://github.com/",
            "GH_TOKEN": token,
        })
    return env


def fetch(t: Target) -> str:
    """Clone ou met à jour le repo sur `ref`. Retourne le commit."""
    repo = t.repo_dir
    ref = t.project.ref if t.pr is None else f"refs/pull/{t.pr}/head"
    env = git_env()
    if not (repo / ".git").exists():
        repo.parent.mkdir(parents=True, exist_ok=True)
        try:
            run(["git", "clone", "--quiet", t.project.repo, str(repo)], env=env)
        except RuntimeError as e:
            hint = ("GITHUB_TOKEN est défini mais refusé : vérifie ses droits (Contents) sur ce repo" if "GH_TOKEN" in env
                    else "repo privé ou URL fausse ; pour un repo privé : `infra secrets set infra.env GITHUB_TOKEN` "
                         "(token fine-grained, Contents read)")
            msg = secrets.mask(str(e), [env.get("GH_TOKEN", "")])  # git peut citer l'URL réécrite (token inclus)
            raise RuntimeError(f"clone de {t.project.repo} impossible — {hint}. {msg}") from e
    run(["git", "-C", str(repo), "fetch", "--quiet", "--prune", "origin", ref], quiet=True, env=env)
    run(["git", "-C", str(repo), "checkout", "--quiet", "--detach", "FETCH_HEAD"], quiet=True)
    run(["git", "-C", str(repo), "clean", "-fdq", "-e", ".env"], quiet=True)
    return run(["git", "-C", str(repo), "rev-parse", "--short", "HEAD"], quiet=True).stdout.strip()


def compose_cmd(t: Target) -> list[str]:
    cmd = ["docker", "compose", "-p", t.compose_project, "--project-directory", str(t.repo_dir),
           "-f", str(t.compose_file)]
    if t.override_file.exists():
        cmd += ["-f", str(t.override_file)]
    if t.env_file.exists():
        cmd += ["--env-file", str(t.env_file)]
    return cmd


def up(t: Target) -> None:
    run([*compose_cmd(t), "up", "-d", "--build", "--remove-orphans", "--quiet-pull"], capture=False)


def down(t: Target, volumes: bool = False) -> None:
    if not t.compose_file.exists():
        # plus de code : on arrête par nom de projet
        ids = run(["docker", "ps", "-aq", "--filter", f"label=com.docker.compose.project={t.compose_project}"],
                  quiet=True).stdout.split()
        if ids:
            run(["docker", "rm", "-f", *ids], quiet=True)
        return
    cmd = [*compose_cmd(t), "down", "--remove-orphans"]
    if volumes:
        cmd.append("-v")
    run(cmd, capture=False)


def ps(t: Target) -> list[dict]:
    rows = run_json(["docker", "compose", "-p", t.compose_project, "ps", "-a", "--format", "json"])
    return rows if isinstance(rows, list) else [rows]


def published_ports(t: Target) -> list[str]:
    out = []
    for c in ps(t):
        for p in c.get("Publishers") or []:
            if p.get("PublishedPort"):
                out.append(f"{p.get('URL') or ''}:{p['PublishedPort']}->{p.get('TargetPort')}/{p.get('Protocol')}")
    return out


def status(t: Target) -> dict:
    containers = ps(t)
    states = {c["Service"]: (c.get("Health") or c.get("State")) for c in containers}
    nets = []
    if t.expose != "none" and containers:
        container = "web" if t.project.service == "static" else (t.project.container or next(iter(states), None))
        for c in containers:
            if c["Service"] == container:
                info = run_json(["docker", "inspect", c["ID"]])
                nets = list(info[0]["NetworkSettings"]["Networks"].keys())
    return {
        "project": t.compose_project,
        "running": bool(containers) and all(s in ("running", "healthy") for s in states.values()),
        "containers": states,
        "published_ports": published_ports(t),
        "traefik_net": "traefik-net" in nets,
        "urls": t.urls if containers else {},
        "commit": run(["git", "-C", str(t.repo_dir), "rev-parse", "--short", "HEAD"], quiet=True, check=False).stdout.strip()
        if t.repo_dir.exists() else None,
    }


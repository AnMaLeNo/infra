"""CLI `infra` : les opérations mécaniques et idempotentes. Voir skills/ pour l'usage."""
from __future__ import annotations

import functools
import json
import sys

import typer
import yaml

from . import __version__, compose, config, forge, paths, runner, secrets, skills
from . import dns as dnsmod
from . import stack as stackmod
from . import tunnel as tunnelmod
from .render import Target, missing_env_vars, render
from .util import CmdError

app = typer.Typer(no_args_is_help=True, add_completion=False, help="Déploiement déclaratif piloté par registry.yaml.")
dns_app = typer.Typer(no_args_is_help=True, help="Enregistrements DNS Cloudflare")
stack_app = typer.Typer(no_args_is_help=True, help="Stacks d'infra (traefik, cloudflared, ui…)")
secrets_app = typer.Typer(no_args_is_help=True, help="Secrets sops+age")
preview_app = typer.Typer(no_args_is_help=True, help="Previews par PR")
tunnel_app = typer.Typer(no_args_is_help=True, help="Cloudflare Tunnel")
agents_app = typer.Typer(no_args_is_help=True, help="Comptes agents IA")
forge_app = typer.Typer(no_args_is_help=True, help="skillforge : juge des skills à partir des runs (host.yaml → forge)")


@forge_app.command("mode")
def forge_mode(mode: str = typer.Argument(..., help="off | shadow | ask")):
    """Règle host.yaml → forge.mode (shadow = verdict écrit dans chaque run, rien affiché ; ask = questions dans le front)."""
    from . import gitsync
    f = config.set_forge(mode=mode).model_dump()
    typer.echo(json.dumps(f | {"git": gitsync.commit_push(f"cli : forge {mode}")}, ensure_ascii=False))


@forge_app.command("replay")
def forge_replay(file: str = typer.Argument(None, help="runs/<fichier>.json ; sans argument : tous les runs")):
    """Rejoue skillforge sur des runs passés et écrit le verdict dans leur .json (preuve n°1 de l'intégration)."""
    if file:
        typer.echo(json.dumps(forge.replay(file), ensure_ascii=False, indent=1))
    else:
        for name, d in forge.replay_all():
            typer.echo(f"{d:12s} {name}")


@forge_app.command("answer")
def forge_answer(file: str, answer: str,
                 apply: bool = typer.Option(False, help="si la réponse est générale : l'ajouter à la skill custom <config>/skills/…")):
    """Réponse de l'auteur à la question skillforge d'un run (historique de la skill, portée, patch)."""
    from . import gitsync
    r = forge.answer(file, answer, apply=apply)
    typer.echo(json.dumps(r | {"git": gitsync.commit_push(f"forge : réponse de l'auteur ({r['scope']})")}, ensure_ascii=False, indent=1))

for name, sub in (("dns", dns_app), ("stack", stack_app), ("secrets", secrets_app), ("preview", preview_app),
                  ("tunnel", tunnel_app), ("agents", agents_app), ("forge", forge_app)):
    app.add_typer(sub, name=name)


def _out(data) -> None:
    typer.echo(yaml.safe_dump(data, sort_keys=False, allow_unicode=True).rstrip())


def _target(name: str, pr: int | None = None) -> Target:
    try:
        return Target(name, config.project(name), config.host(), pr)
    except KeyError as e:
        raise typer.BadParameter(str(e)) from e


def _main_guard(fn):
    @functools.wraps(fn)
    def wrapper(*a, **kw):
        try:
            return fn(*a, **kw)
        except (CmdError, RuntimeError, FileNotFoundError, ValueError) as e:
            typer.secho(f"erreur : {e}", fg="red", err=True)
            raise typer.Exit(1) from e
    return wrapper


@app.callback(invoke_without_command=True)
def _version_cb(ctx: typer.Context, version: bool = typer.Option(False, "--version", is_eager=True)):
    if version:
        typer.echo(f"infra {__version__}")
        raise typer.Exit()
    if ctx.invoked_subcommand is None:
        typer.echo(ctx.get_help())
        raise typer.Exit()


@app.command()
@_main_guard
def validate():
    """Valide host.yaml et registry.yaml, et la présence des skills."""
    h = config.host()
    reg = config.registry()
    problems = []
    for name, prj in reg.projects.items():
        for rel, p in skills.paths_for(prj).items():
            if p is None:
                problems.append(f"{name}: skill {rel} introuvable")
    _out({"host": h.name, "projects": len(reg.projects), "problems": problems or "aucun"})
    if any("introuvable" in p for p in problems):
        raise typer.Exit(1)


@app.command()
@_main_guard
def status(name: str | None = typer.Argument(None), as_json: bool = typer.Option(False, "--json")):
    """État réel d'un projet (ou de tous) : containers, ports publiés, routes."""
    reg = config.registry()
    h = config.host()
    names = [name] if name else list(reg.projects)
    out = {}
    for n in names:
        t = Target(n, config.project(n), h)
        s = compose.status(t)
        s["wanted"] = {"enabled": t.project.enabled, "expose": t.project.expose, "previews": t.project.previews}
        out[n] = s
    typer.echo(json.dumps(out, indent=2) if as_json else yaml.safe_dump(out, sort_keys=False, allow_unicode=True).rstrip())


@app.command()
@_main_guard
def fetch(name: str):
    """Clone / met à jour le repo du projet sur `ref`."""
    typer.echo(compose.fetch(_target(name)))


@app.command("render")
@_main_guard
def render_cmd(name: str):
    """Génère compose.override.yml et .env (déchiffré). Ne lance rien."""
    t = _target(name)
    changed = render(t)
    missing = missing_env_vars(t)
    _out({"override": str(t.override_file), "changed": changed, "missing_env": missing or "aucune"})
    if missing:
        raise typer.Exit(2)


@app.command()
@_main_guard
def up(name: str):
    """docker compose up -d --build avec l'override généré (aide humaine ; le LLM lance docker compose lui-même)."""
    t = _target(name)
    compose.up(t)
    _out(compose.status(t))


@app.command()
@_main_guard
def down(name: str, volumes: bool = typer.Option(False, help="supprime aussi les volumes nommés")):
    """Arrête le projet (données conservées sauf --volumes)."""
    compose.down(_target(name), volumes=volumes)


@app.command("set")
@_main_guard
def set_cmd(name: str, fields: list[str] = typer.Argument(..., help="cle=valeur …  (valeur vide = supprimer)")):
    """Modifie une entrée de registry.yaml (ex. infra set catacombes port=8420 container=web)."""
    kv = {}
    for f in fields:
        k, _, v = f.partition("=")
        if v == "":
            kv[k] = None
        elif v in ("true", "false"):
            kv[k] = v == "true"
        elif v.isdigit():
            kv[k] = int(v)
        elif v.startswith(("[", "{")):
            kv[k] = yaml.safe_load(v)
        else:
            kv[k] = v
    from . import gitsync
    _out({name: config.set_project_fields(name, source="cli", **kv).model_dump(exclude_defaults=True),
          "git": gitsync.commit_push(f"cli : projet {name} modifié")})


@app.command()
@_main_guard
def purge(name: str, yes: bool = typer.Option(False, "--yes")):
    """Supprime containers, volumes, clone, .env et /srv/data/<P>. Humain uniquement."""
    import shutil
    if not yes:
        typer.confirm(f"Supprimer DÉFINITIVEMENT données et clone de {name} ?", abort=True)
    t = _target(name)
    compose.down(t, volumes=True)
    shutil.rmtree(t.dir, ignore_errors=True)
    shutil.rmtree(paths.data_dir() / name, ignore_errors=True)
    typer.echo("purgé")


@app.command()
@_main_guard
def run(name: str, dry: bool = typer.Option(False, help="affiche le prompt sans lancer claude")):
    """Lance immédiatement un run LLM (claude -p) pour un projet, avec ses skills (sans passer par la file)."""
    reg = config.registry()
    prj = reg.projects.get(name)
    state = runner.load_state()
    rec = runner.run_project(name, prj, state.get(name, {}).get("entry"), dry=dry)
    if dry:
        typer.echo(rec["prompt"])
    else:
        runner.mark_applied(state, name, prj, rec["ok"])
        typer.echo(rec["result"])
        raise typer.Exit(0 if rec["ok"] else 1)


@app.command()
@_main_guard
def watch(interval: float = 3.0, once: bool = False, dry: bool = False):
    """Service infra-runner : exécute les runs demandés (front / `infra enqueue`), un à la fois. Rien d'automatique."""
    runner.watch(interval=interval, once=once, dry=dry)


@app.command()
@_main_guard
def editor():
    """Service infra-editor : VS Code web (`code serve-web`) sur le dossier des runs, pour reprendre la session Claude d'un run (bouton du front)."""
    from . import editor as ed
    ed.serve()


@app.command()
@_main_guard
def enqueue(name: str):
    """Demande un run pour un projet (équivalent du bouton « Lancer Claude » du front)."""
    config.project(name) if name in config.registry().projects else None
    _out(runner.enqueue(name))


@app.command()
@_main_guard
def plan():
    """Projets modifiés depuis leur dernier run, demandes en attente, run en cours."""
    _out({"modifiés": runner.changed_since_last_run(runner.load_state()) or "aucun",
          "en attente": runner.queued() or "aucune", "en cours": runner.running() or "aucun"})


@app.command()
@_main_guard
def runs(limit: int = 20):
    """Derniers runs LLM."""
    _out(runner.list_runs(limit))


@app.command()
@_main_guard
def history(name: str):
    """Historique d'un projet : versions de sa config + ses runs (ce que montre le bouton « Historique » du front)."""
    from . import history as hist
    _out({"config": hist.for_project(name), "runs": runner.runs_for_project(name)})


@app.command("llm")
@_main_guard
def llm_cmd(model: str | None = typer.Option(None), effort: str | None = typer.Option(None)):
    """Affiche / règle le modèle et l'effort de claude -p pour tous les runs (host.yaml → llm)."""
    if model or effort:
        from . import gitsync
        _out(config.set_llm(model=model, effort=effort).model_dump() | {"git": gitsync.commit_push("cli : settings LLM")})
    else:
        _out(config.host().llm.model_dump())


@app.command("skills")
@_main_guard
def skills_cmd(name: str | None = typer.Argument(None)):
    """Skills disponibles, ou ceux qu'un projet utilise."""
    if name:
        _out({k: str(v) for k, v in skills.paths_for(config.project(name)).items()})
    else:
        _out(skills.list_all())


@app.command()
@_main_guard
def mcp():
    """Serveur MCP (stdio) : les tools atomiques de l'outil, pour Claude Code (branché par le runner)."""
    from .mcp_server import main
    main()


@app.command()
@_main_guard
def serve(host: str = "127.0.0.1", port: int = 8000):
    """API + UI web."""
    import uvicorn
    uvicorn.run("infra.api:app", host=host, port=port, log_level="info")


@app.command()
@_main_guard
def doctor(legacy: bool = typer.Option(False, "--legacy", help="liste aussi ce qui tourne hors modèle")):
    """Vérifie l'hôte : docker, tailscale, traefik, tunnel, dns, age, runner."""
    from .doctor import doctor as _doctor
    rep = _doctor(legacy=legacy)
    _out(rep)
    if any(v.startswith("KO") for v in rep.values() if isinstance(v, str)):
        raise typer.Exit(1)


# ---- dns ---------------------------------------------------------------------------------------

@dns_app.command("list")
@_main_guard
def dns_list():
    _out(dnsmod.list_records(config.host()))


@dns_app.command("ensure")
@_main_guard
def dns_ensure(name: str, force: bool = typer.Option(False, "--force", help="écrase un enregistrement existant divergent")):
    """Crée les enregistrements voulus pour un projet (CNAME public, wildcards previews)."""
    res = dnsmod.ensure_project(name, config.project(name), config.host(), force=force)
    _out(res)
    if "conflict" in res.values():
        typer.secho("conflit : un enregistrement existe avec une autre cible ; vérifie puis --force", fg="yellow", err=True)
        raise typer.Exit(2)


@dns_app.command("prune")
@_main_guard
def dns_prune(name: str, public_only: bool = typer.Option(False, "--public", help="ne supprime que le CNAME public")):
    """Supprime les enregistrements d'un projet."""
    _out(dnsmod.prune_project(name, config.project(name), config.host(), public_only=public_only) or "rien")


@dns_app.command("bootstrap")
@_main_guard
def dns_bootstrap():
    """A *.<private_zone> → tailscale_ip."""
    _out(dnsmod.bootstrap(config.host()))


@dns_app.command("check")
@_main_guard
def dns_check():
    _out(dnsmod.check(config.host()))


# ---- stack -------------------------------------------------------------------------------------

@stack_app.command("up")
@_main_guard
def stack_up(name: str | None = typer.Argument(None), all_: bool = typer.Option(False, "--all")):
    for n in (stackmod.all_names() if all_ else [name]):
        stackmod.up(n)
        _out({n: stackmod.status(n)})


@stack_app.command("down")
@_main_guard
def stack_down(name: str):
    stackmod.down(name)


@stack_app.command("status")
@_main_guard
def stack_status(name: str | None = typer.Argument(None)):
    _out({n: stackmod.status(n) for n in ([name] if name else stackmod.all_names())})


# ---- tunnel ------------------------------------------------------------------------------------

@tunnel_app.command("create")
@_main_guard
def tunnel_create(name: str | None = typer.Option(None)):
    """Crée le tunnel (API), chiffre les credentials, écrit tunnel_id dans host.yaml."""
    typer.echo(tunnelmod.create(name))


@tunnel_app.command("info")
@_main_guard
def tunnel_info():
    _out(tunnelmod.info())


# ---- secrets -----------------------------------------------------------------------------------

@secrets_app.command("edit")
@_main_guard
def secrets_edit(rel: str):
    secrets.edit(rel)


@secrets_app.command("set")
@_main_guard
def secrets_set(rel: str, pairs: list[str]):
    """infra secrets set projects/x.env CLE=valeur  (ou CLE seule : valeur lue sur stdin)."""
    values = {}
    for p in pairs:
        k, sep, v = p.partition("=")
        values[k] = v if sep else sys.stdin.readline().rstrip("\n")
    secrets.set_values(rel, values)
    typer.echo(f"{len(values)} variable(s) écrite(s) dans secrets/{rel}")


@secrets_app.command("keys")
@_main_guard
def secrets_keys(rel: str):
    _out(secrets.keys(rel))


@secrets_app.command("check")
@_main_guard
def secrets_check():
    res = secrets.check()
    _out(res or "aucun secret")
    if any(v != "ok" for v in res.values()):
        raise typer.Exit(1)


@secrets_app.command("rekey")
@_main_guard
def secrets_rekey():
    secrets.rekey()


# ---- previews ----------------------------------------------------------------------------------

@preview_app.command("up")
@_main_guard
def preview_up(name: str, pr: int, url: bool = typer.Option(False, "--url", help="n'affiche que l'URL")):
    from .previews import up as _up
    res = _up(_target(name, pr))
    typer.echo(res["url"] if url else yaml.safe_dump(res, sort_keys=False).rstrip())


@preview_app.command("down")
@_main_guard
def preview_down(name: str, pr: int, url: bool = typer.Option(False, "--url")):
    from .previews import down as _down
    _down(_target(name, pr))
    typer.echo("" if url else "supprimée")


@preview_app.command("list")
@_main_guard
def preview_list(name: str):
    from .previews import list_previews
    _out(list_previews(name) or "aucune")


@preview_app.command("prune")
@_main_guard
def preview_prune(name: str, all_: bool = typer.Option(False, "--all", help="toutes, pas seulement les PR fermées")):
    from .previews import prune
    _out(prune(name, everything=all_) or "rien")


@preview_app.command("install")
@_main_guard
def preview_install(name: str):
    """Affiche le workflow à ajouter dans le repo du projet et les secrets GitHub à créer."""
    from .previews import workflow_snippet
    typer.echo(workflow_snippet(name))


# ---- agents ------------------------------------------------------------------------------------

@agents_app.command("sync")
@_main_guard
def agents_sync(prune: bool = typer.Option(False, "--prune", help="verrouille les comptes ai-* sans clé")):
    from .agents import sync
    _out(sync(prune=prune))


@agents_app.command("retire")
@_main_guard
def agents_retire(names: list[str]):
    from .agents import retire
    _out(retire(names))


if __name__ == "__main__":
    app()

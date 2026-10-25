from __future__ import annotations

from pathlib import Path

import yaml

from . import history, paths
from .models import Forge, Host, Llm, Project, Registry, Shared, SharedRegistry


def load_yaml(p: Path) -> dict:
    if not p.exists():
        return {}
    return yaml.safe_load(p.read_text()) or {}


def dump_yaml(data: dict, p: Path) -> None:
    p.write_text(yaml.safe_dump(data, sort_keys=False, allow_unicode=True))


def host() -> Host:
    return Host.model_validate(load_yaml(paths.config_dir() / "host.yaml"))


def registry() -> Registry:
    return Registry.model_validate(load_yaml(paths.config_dir() / "registry.yaml"))


def project(name: str) -> Project:
    reg = registry()
    if name not in reg.projects:
        raise KeyError(f"projet inconnu : {name}")
    return reg.projects[name]


def set_llm(**fields) -> Llm:
    """Modifie host.yaml → llm (modèle / effort de claude -p), depuis le front ou `infra set-llm`."""
    p = paths.config_dir() / "host.yaml"
    raw = load_yaml(p)
    raw["llm"] = {**raw.get("llm", {}), **{k: v for k, v in fields.items() if v is not None}}
    h = Host.model_validate(raw)
    dump_yaml(raw, p)
    return h.llm


def set_forge(**fields) -> Forge:
    """Modifie host.yaml → forge (mode off/shadow/ask, budget), depuis le front ou `infra forge mode`."""
    p = paths.config_dir() / "host.yaml"
    raw = load_yaml(p)
    raw["forge"] = {**raw.get("forge", {}), **{k: v for k, v in fields.items() if v is not None}}
    h = Host.model_validate(raw)
    dump_yaml(raw, p)
    return h.forge


def set_project_fields(name: str, source: str = "ui", **fields) -> Project:
    """Modifie registry.yaml (utilisé par `infra set` et l'UI). Préserve l'ordre et les autres clés.
    Chaque écriture est historisée dans runs/history/<P>.jsonl."""
    p = paths.config_dir() / "registry.yaml"
    raw = load_yaml(p)
    raw.setdefault("projects", {})
    raw["projects"].setdefault(name, {})
    for k, v in fields.items():
        if v is None:
            raw["projects"][name].pop(k, None)
        else:
            raw["projects"][name][k] = v
    reg = Registry.model_validate(raw)  # valide avant d'écrire
    dump_yaml(raw, p)
    history.record(name, reg.projects[name].model_dump(), source)
    return reg.projects[name]


def remove_project(name: str, source: str = "ui") -> None:
    p = paths.config_dir() / "registry.yaml"
    raw = load_yaml(p)
    if raw.get("projects", {}).pop(name, None) is not None:
        dump_yaml(raw, p)
        history.record(name, None, source)


def domain_of(name: str, prj: Project) -> str:
    return prj.domain or name


def private_host(name: str, prj: Project, h: Host) -> str:
    return f"{domain_of(name, prj)}.{h.private_zone}"


def public_host(name: str, prj: Project, h: Host) -> str:
    return f"{domain_of(name, prj)}.{h.public_zone}"


# ---- services partagés (shared.yaml) ----

def shared_file() -> Path:
    return paths.config_dir() / "shared.yaml"


def shared() -> SharedRegistry:
    p = shared_file()
    return SharedRegistry.model_validate(load_yaml(p) if p.exists() else {})


def shared_service(name: str) -> Shared:
    reg = shared()
    if name not in reg.services:
        raise KeyError(f"service partagé inconnu : {name} (à déclarer dans shared.yaml)")
    return reg.services[name]


def users_of(name: str) -> list[str]:
    """Projets du registre qui déclarent `uses: [<name>]` — calculé, jamais stocké."""
    return sorted(n for n, p in registry().projects.items() if name in p.uses)


def set_shared_fields(name: str, source: str = "ui", **fields) -> Shared:
    """Modifie shared.yaml ; chaque écriture est historisée (runs/history/shared.<nom>.jsonl)."""
    p = shared_file()
    raw = load_yaml(p) if p.exists() else {}
    raw.setdefault("services", {})
    raw["services"].setdefault(name, {})
    for k, v in fields.items():
        if v is None:
            raw["services"][name].pop(k, None)
        else:
            raw["services"][name][k] = v
    reg = SharedRegistry.model_validate(raw)
    dump_yaml(raw, p)
    history.record(shared_key(name), reg.services[name].model_dump(), source)
    return reg.services[name]


def remove_shared(name: str, source: str = "ui") -> None:
    users = users_of(name)
    if users:
        raise ValueError(f"{name} est encore utilisé par : {', '.join(users)} (retire-le de leur `uses:` d'abord)")
    p = shared_file()
    raw = load_yaml(p) if p.exists() else {}
    if raw.get("services", {}).pop(name, None) is not None:
        dump_yaml(raw, p)
        history.record(shared_key(name), None, source)


def shared_key(name: str) -> str:
    """Clé d'un service partagé dans la file, l'état et les runs (`shared.<nom>` : un nom de projet ne contient pas de point)."""
    return f"shared.{name}"

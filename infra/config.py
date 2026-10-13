from __future__ import annotations

from pathlib import Path

import yaml

from . import paths
from .models import Host, Project, Registry


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


def set_project_fields(name: str, **fields) -> Project:
    """Modifie registry.yaml (utilisé par `infra set` et l'UI). Préserve l'ordre et les autres clés."""
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
    return reg.projects[name]


def remove_project(name: str) -> None:
    p = paths.config_dir() / "registry.yaml"
    raw = load_yaml(p)
    raw.get("projects", {}).pop(name, None)
    dump_yaml(raw, p)


def domain_of(name: str, prj: Project) -> str:
    return prj.domain or name


def private_host(name: str, prj: Project, h: Host) -> str:
    return f"{domain_of(name, prj)}.{h.private_zone}"


def public_host(name: str, prj: Project, h: Host) -> str:
    return f"{domain_of(name, prj)}.{h.public_zone}"

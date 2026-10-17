from __future__ import annotations

from pathlib import Path

import yaml

from . import history, paths
from .models import Host, Llm, Project, Registry


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

"""Résolution des skills pour un projet (skills/README.md)."""
from __future__ import annotations

from pathlib import Path

from . import paths
from .models import Project


def resolve(rel: str) -> Path | None:
    for base in (paths.config_dir() / "skills", paths.tool_dir() / "skills"):
        p = base / rel / "SKILL.md"
        if p.exists():
            return p
    return None


def for_project(prj: Project | None) -> list[str]:
    if prj is None or not prj.enabled:
        return ["undeploy"]
    rels = [f"service/{prj.service}", f"expose/{prj.expose}"]
    if prj.previews != "none":
        rels.append("previews")
    return rels


def paths_for(prj: Project | None) -> dict[str, Path | None]:
    return {rel: resolve(rel) for rel in for_project(prj)}


def list_all() -> dict[str, str]:
    out: dict[str, str] = {}
    for base, origin in ((paths.tool_dir() / "skills", "tool"), (paths.config_dir() / "skills", "custom")):
        if base.exists():
            for p in sorted(base.rglob("SKILL.md")):
                out[str(p.parent.relative_to(base))] = origin
    return out

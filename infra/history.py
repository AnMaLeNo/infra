"""Historique de la config par projet : runs/history/<P>.jsonl, une ligne par écriture du registre.

Avec les runs (runs/<date>-<P>.json), ça donne la vue « de A à Z » d'un projet dans le front.
"""
from __future__ import annotations

import json
from datetime import UTC, datetime

from . import paths


def _file(name: str):
    d = paths.runs_dir() / "history"
    d.mkdir(parents=True, exist_ok=True)
    return d / f"{name}.jsonl"


def record(name: str, entry: dict | None, source: str = "ui") -> dict:
    """Ajoute une version de l'entrée (None = retirée du registre). Ignore si identique à la précédente."""
    prev = for_project(name)
    if prev and prev[-1]["entry"] == entry:
        return prev[-1]
    ev = {"at": datetime.now(UTC).isoformat(), "source": source, "entry": entry}
    with _file(name).open("a") as f:
        f.write(json.dumps(ev, ensure_ascii=False) + "\n")
    return ev


def for_project(name: str) -> list[dict]:
    p = _file(name)
    if not p.exists():
        return []
    return [json.loads(line) for line in p.read_text().splitlines() if line.strip()]

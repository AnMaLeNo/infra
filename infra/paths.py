"""Chemins conventionnels sur l'hôte (docs/conventions.md). Surchargeables par variables d'env pour les tests."""
from __future__ import annotations

import os
from pathlib import Path


def config_dir() -> Path:
    return Path(os.environ.get("INFRA_CONFIG_DIR", "/srv/infra"))


def tool_dir() -> Path:
    env = os.environ.get("INFRA_TOOL_DIR")
    if env:
        return Path(env)
    sub = config_dir() / "tool"
    return sub if sub.exists() else Path(__file__).resolve().parent.parent


def apps_dir() -> Path:
    return Path(os.environ.get("INFRA_APPS_DIR", "/srv/apps"))


def data_dir() -> Path:
    return Path(os.environ.get("INFRA_DATA_DIR", "/srv/data"))


def stack_state_dir() -> Path:
    return Path(os.environ.get("INFRA_STACK_DIR", "/srv/stack"))


def runs_dir() -> Path:
    return config_dir() / "runs"


def age_key_file() -> Path:
    return Path(os.environ.get("SOPS_AGE_KEY_FILE", str(config_dir() / ".age-key")))


def app_dir(project: str) -> Path:
    return apps_dir() / project


def preview_dir(project: str, pr: int) -> Path:
    return app_dir(project) / "previews" / f"pr-{pr}"


def shared_dir(name: str) -> Path:
    """Définition d'un service partagé (compose + fichiers de conf), versionnée dans le repo de config."""
    return config_dir() / "shared" / name


def shared_state_dir(name: str) -> Path:
    """Données/état d'un service partagé sur l'hôte (bind mounts) — ce qu'on sauvegarde."""
    return Path(os.environ.get("INFRA_SHARED_DIR", "/srv/shared")) / name

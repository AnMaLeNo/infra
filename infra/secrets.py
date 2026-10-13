"""Secrets chiffrés sops+age dans <config>/secrets/. Jamais de valeur dans les logs."""
from __future__ import annotations

import os
from pathlib import Path

from . import paths
from .util import CmdError, dump_dotenv, parse_dotenv, run


def secrets_dir() -> Path:
    return paths.config_dir() / "secrets"


def _env() -> dict:
    return {"SOPS_AGE_KEY_FILE": str(paths.age_key_file())}


def _fmt(rel: str) -> list[str]:
    if rel.endswith(".env"):
        return ["--input-type", "dotenv", "--output-type", "dotenv"]
    if rel.endswith(".json"):
        return ["--input-type", "json", "--output-type", "json"]
    return []


def exists(rel: str) -> bool:
    return (secrets_dir() / rel).exists()


def decrypt(rel: str) -> str:
    p = secrets_dir() / rel
    if not p.exists():
        raise FileNotFoundError(f"secret absent : secrets/{rel}")
    cp = run(["sops", *_fmt(rel), "-d", str(p)], env=_env(), quiet=True)
    return cp.stdout


def encrypt_write(rel: str, plaintext: str) -> None:
    """Chiffre `plaintext` dans secrets/<rel> (crée ou remplace) selon .sops.yaml du repo de config."""
    p = secrets_dir() / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    tmp = p.with_name(p.name + ".plain")
    tmp.write_text(plaintext)
    os.chmod(tmp, 0o600)
    try:
        # sops -e lit les creation_rules relativement au fichier : on chiffre en place puis on renomme
        cp = run(["sops", *_fmt(rel), "--config", str(paths.config_dir() / ".sops.yaml"),
                  "--filename-override", str(p), "-e", str(tmp)], env=_env(), quiet=True,
                 cwd=paths.config_dir())
        p.write_text(cp.stdout)
    finally:
        tmp.unlink(missing_ok=True)


def dotenv(rel: str) -> dict[str, str]:
    return parse_dotenv(decrypt(rel))


def infra_env() -> dict[str, str]:
    return dotenv("infra.env") if exists("infra.env") else {}


def keys(rel: str) -> list[str]:
    return list(dotenv(rel).keys())


def set_values(rel: str, values: dict[str, str]) -> None:
    env = dotenv(rel) if exists(rel) else {}
    env.update(values)
    encrypt_write(rel, dump_dotenv(env))


def edit(rel: str) -> None:
    p = secrets_dir() / rel
    if not p.exists():
        encrypt_write(rel, "" if not rel.endswith(".json") else "{}\n")
    run(["sops", *_fmt(rel), str(p)], env=_env(), capture=False)


def rekey() -> None:
    for p in secrets_dir().rglob("*"):
        if p.is_file():
            run(["sops", "updatekeys", "-y", str(p)], env=_env(), cwd=paths.config_dir())


def check() -> dict[str, str]:
    """Retourne {fichier: 'ok'|'erreur ...'} pour tous les secrets."""
    out = {}
    for p in sorted(secrets_dir().rglob("*")):
        if not p.is_file():
            continue
        rel = str(p.relative_to(secrets_dir()))
        try:
            decrypt(rel)
            out[rel] = "ok"
        except (CmdError, FileNotFoundError) as e:
            out[rel] = f"erreur : {str(e).splitlines()[-1]}"
    return out


def project_env_path(project: str, preview: bool = False) -> str:
    return f"projects/{project}.preview.env" if preview else f"projects/{project}.env"


def mask(text: str, values: list[str]) -> str:
    for v in sorted({v for v in values if len(v) >= 4}, key=len, reverse=True):
        text = text.replace(v, "***")
    return text

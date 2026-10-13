from __future__ import annotations

import json
import os
import shlex
import subprocess
import sys
from pathlib import Path


class CmdError(RuntimeError):
    pass


def run(cmd: list[str], *, cwd: Path | None = None, env: dict | None = None, check: bool = True,
        capture: bool = True, input: str | None = None, quiet: bool = False) -> subprocess.CompletedProcess:
    if not quiet:
        print(f"$ {' '.join(shlex.quote(c) for c in cmd)}", file=sys.stderr)
    full_env = {**os.environ, **(env or {})}
    cp = subprocess.run(cmd, check=False, cwd=cwd, env=full_env, text=True, input=input,
                        stdout=subprocess.PIPE if capture else None,
                        stderr=subprocess.PIPE if capture else None)
    if check and cp.returncode != 0:
        raise CmdError(f"commande échouée ({cp.returncode}) : {' '.join(cmd)}\n{(cp.stderr or '').strip()}")
    return cp


def run_json(cmd: list[str], **kw) -> list | dict:
    out = run(cmd, quiet=True, **kw).stdout.strip()
    if not out:
        return []
    # `docker compose ps --format json` renvoie parfois un objet par ligne
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return [json.loads(line) for line in out.splitlines() if line.strip()]


def write_private(p: Path, content: str, mode: int = 0o600) -> bool:
    """Écrit si différent. Retourne True si le fichier a changé."""
    p.parent.mkdir(parents=True, exist_ok=True)
    if p.exists() and p.read_text() == content:
        os.chmod(p, mode)
        return False
    tmp = p.with_suffix(p.suffix + ".tmp")
    tmp.write_text(content)
    os.chmod(tmp, mode)
    tmp.replace(p)
    return True


def parse_dotenv(text: str) -> dict[str, str]:
    env: dict[str, str] = {}
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        k, v = line.split("=", 1)
        k = k.strip().removeprefix("export ").strip()
        v = v.strip()
        if len(v) >= 2 and v[0] == v[-1] and v[0] in "\"'":
            v = v[1:-1]
        env[k] = v
    return env


def dump_dotenv(env: dict[str, str]) -> str:
    return "".join(f"{k}={v}\n" for k, v in env.items())

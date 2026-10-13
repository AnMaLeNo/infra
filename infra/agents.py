"""Comptes ai-* : créés depuis <config>/agents/*.pub (skills/agent-account)."""
from __future__ import annotations

import os
import pwd
from pathlib import Path

from . import paths
from .util import run, write_private

SUDOERS_FULL = """Defaults:{u} log_input, log_output
{u} ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra
"""
# profil restreint : GitHub Actions ne peut lancer que `infra preview …`
FORCE_CMD = 'command="/usr/local/bin/infra-preview-only",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty '


def _sudo(cmd: list[str]) -> None:
    run(cmd if os.geteuid() == 0 else ["sudo", "-n", *cmd], quiet=True)


def sync(prune: bool = False) -> dict[str, str]:
    d = paths.config_dir() / "agents"
    out: dict[str, str] = {}
    keys = {p.stem: p.read_text().strip() for p in sorted(d.glob("ai-*.pub"))} if d.exists() else {}
    for user, key in keys.items():
        try:
            pwd.getpwnam(user)
            created = False
        except KeyError:
            _sudo(["useradd", "-m", "-s", "/bin/bash", "-G", "docker,infra", "-c", f"Agent IA ({user})", user])
            created = True
        home = Path(pwd.getpwnam(user).pw_dir)
        line = (FORCE_CMD + key) if user == "ai-github" else key
        ak = home / ".ssh" / "authorized_keys"
        _sudo(["mkdir", "-p", str(ak.parent)])
        tmp = Path(f"/tmp/.ak-{user}")
        write_private(tmp, line + "\n")
        _sudo(["install", "-o", user, "-g", user, "-m", "0600", str(tmp), str(ak)])
        _sudo(["chmod", "0700", str(ak.parent)])
        _sudo(["chown", f"{user}:{user}", str(ak.parent)])
        tmp.unlink(missing_ok=True)
        if user != "ai-github":
            tmp_s = Path(f"/tmp/.sudoers-{user}")
            write_private(tmp_s, SUDOERS_FULL.format(u=user), 0o440)
            _sudo(["install", "-o", "root", "-g", "root", "-m", "0440", str(tmp_s), f"/etc/sudoers.d/{user}"])
            tmp_s.unlink(missing_ok=True)
        _sudo(["usermod", "-L", "-aG", "docker,infra", user]) if created else None
        out[user] = "créé" if created else "ok"
    if prune:
        for p in pwd.getpwall():
            if p.pw_name.startswith("ai-") and p.pw_name != "ai-infra" and p.pw_name not in keys:
                _sudo(["usermod", "-L", "-s", "/usr/sbin/nologin", p.pw_name])
                _sudo(["rm", "-f", f"/etc/sudoers.d/{p.pw_name}", f"{p.pw_dir}/.ssh/authorized_keys"])
                out[p.pw_name] = "verrouillé (plus de clé)"
    return out


def retire(names: list[str]) -> dict[str, str]:
    """Verrouille d'anciens comptes partagés (ne supprime pas le home)."""
    out = {}
    for n in names:
        _sudo(["usermod", "-L", "-s", "/usr/sbin/nologin", n])
        _sudo(["rm", "-f", f"/etc/sudoers.d/{n}"])
        _sudo(["bash", "-c", f"[ -f ~{n}/.ssh/authorized_keys ] && mv ~{n}/.ssh/authorized_keys ~{n}/.ssh/authorized_keys.retired || true"])
        out[n] = "verrouillé"
    return out

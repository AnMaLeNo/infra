"""infra doctor : état de l'hôte, et (--legacy) ce qui tourne hors modèle."""
from __future__ import annotations

import shutil

from . import config, paths, secrets, stack
from .util import run

WHITELIST_LISTEN = {"22", "41641"}  # ssh, tailscale


def _ok(cond: bool, ok: str = "ok", ko: str = "KO") -> str:
    return ok if cond else ko


def doctor(legacy: bool = False) -> dict:
    rep: dict = {}
    h = None
    try:
        h = config.host()
        rep["host.yaml"] = f"ok ({h.name})"
    except Exception as e:
        rep["host.yaml"] = f"KO {e}"
    try:
        reg = config.registry()
        rep["registry.yaml"] = f"ok ({len(reg.projects)} projets)"
    except Exception as e:
        rep["registry.yaml"] = f"KO {e}"
    for b in ("docker", "sops", "age", "tailscale", "git"):
        rep[b] = _ok(shutil.which(b) is not None, ko="KO absent")
    rep["claude"] = _ok(shutil.which("claude") is not None, ko="KO absent (runner inutilisable)")
    rep["age-key"] = _ok(paths.age_key_file().exists(), ko=f"KO {paths.age_key_file()} absent")
    if paths.age_key_file().exists():
        chk = secrets.check()
        rep["secrets"] = _ok(all(v == "ok" for v in chk.values()), ok=f"ok ({len(chk)})", ko=f"KO {chk}")
    if h:
        ts = run(["tailscale", "ip", "-4"], check=False, quiet=True).stdout.strip()
        rep["tailscale_ip"] = _ok(ts == h.tailscale_ip, ok=f"ok {ts}", ko=f"KO réel={ts or 'aucune'} attendu={h.tailscale_ip}")
        nets = run(["docker", "network", "ls", "--format", "{{.Name}}"], check=False, quiet=True).stdout.split()
        rep["traefik-net"] = _ok("traefik-net" in nets)
        for s in stack.all_names():
            try:
                st = stack.status(s)
                rep[f"stack/{s}"] = _ok(bool(st) and all(v in ("running", "healthy") for v in st.values()), ko=f"KO {st or 'absent'}")
            except Exception as e:
                rep[f"stack/{s}"] = f"KO {e}"
        if h.public_ingress == "cloudflare-tunnel":
            rep["tunnel_id"] = _ok(bool(h.cloudflare.tunnel_id), ko="KO (infra tunnel create)")
    active = run(["systemctl", "is-active", "infra-runner"], check=False, quiet=True).stdout.strip()
    rep["infra-runner"] = _ok(active == "active", ko=f"KO {active}")
    # sockets exposés hors liste blanche
    ss = run(["ss", "-ltnH"], check=False, quiet=True).stdout
    exposed = []
    for line in ss.splitlines():
        parts = line.split()
        if len(parts) < 4:
            continue
        local = parts[3]
        ip, _, port = local.rpartition(":")
        if ip in ("0.0.0.0", "*", "[::]") and port not in WHITELIST_LISTEN:
            exposed.append(local)
    rep["listen_0.0.0.0"] = _ok(not exposed, ko=f"KO {exposed}")
    if legacy:
        rep["legacy"] = _legacy()
    return rep


def _legacy() -> dict:
    out: dict = {}
    units = run(["systemctl", "list-units", "--type=service", "--state=running", "--no-legend", "--plain"],
                check=False, quiet=True).stdout
    std = {"ssh", "sshd", "docker", "containerd", "tailscaled", "infra-runner", "cron", "dbus", "systemd-", "getty@",
           "user@", "rsyslog", "polkit", "avahi", "ModemManager", "wpa_supplicant", "NetworkManager", "bluetooth",
           "smartd", "netdata", "unattended-upgrades", "earlyoom", "zramswap", "rpi-eeprom", "udisks2", "fail2ban",
           "accounts-daemon", "lightdm", "gdm", "cups", "colord", "upower", "rtkit", "triggerhappy", "thd"}
    out["systemd_units"] = sorted(u.split()[0] for u in units.splitlines()
                                  if u.split() and not any(u.split()[0].startswith(s) for s in std))
    projects = run(["docker", "ps", "--format", "{{.Label \"com.docker.compose.project\"}}|{{.Label \"com.docker.compose.project.working_dir\"}}"],
                   check=False, quiet=True).stdout
    outside = set()
    for line in projects.splitlines():
        proj, _, wd = line.partition("|")
        if wd and not (wd.startswith(str(paths.apps_dir())) or proj.startswith("infra-")):
            outside.add(f"{proj or '(docker run)'} @ {wd}")
        if not proj:
            outside.add("(docker run sans compose)")
    out["compose_hors_srv_apps"] = sorted(outside)
    return out

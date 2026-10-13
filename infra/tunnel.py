"""Création du Cloudflare Tunnel géré localement (credentials chiffrés dans secrets/cloudflared.json)."""
from __future__ import annotations

import base64
import json
import secrets as pysecrets

from . import config, paths, secrets
from .dns import CF


def create(name: str | None = None) -> str:
    h = config.host()
    if secrets.exists("cloudflared.json") and h.cloudflare.tunnel_id:
        return h.cloudflare.tunnel_id
    cf = CF()
    zid = h.cloudflare.zone_id or cf.zone_id(h.public_zone)
    acct = h.cloudflare.account_id or cf.account_id(zid)
    tunnel_name = name or f"infra-{h.name}"
    existing = cf._ok(cf.c.get(f"/accounts/{acct}/cfd_tunnel", params={"name": tunnel_name, "is_deleted": "false"}))
    if existing:
        raise RuntimeError(f"un tunnel `{tunnel_name}` existe déjà ({existing[0]['id']}) : supprime-le ou choisis un autre nom "
                           f"(ses credentials ne sont pas récupérables)")
    secret = base64.b64encode(pysecrets.token_bytes(32)).decode()
    t = cf._ok(cf.c.post(f"/accounts/{acct}/cfd_tunnel",
                         json={"name": tunnel_name, "tunnel_secret": secret, "config_src": "local"}))
    creds = {"AccountTag": acct, "TunnelSecret": secret, "TunnelID": t["id"], "TunnelName": tunnel_name}
    secrets.encrypt_write("cloudflared.json", json.dumps(creds, indent=2) + "\n")
    raw = config.load_yaml(paths.config_dir() / "host.yaml")
    raw.setdefault("cloudflare", {}).update({"zone_id": zid, "account_id": acct, "tunnel_id": t["id"]})
    config.dump_yaml(raw, paths.config_dir() / "host.yaml")
    return t["id"]


def info() -> dict:
    h = config.host()
    if not h.cloudflare.tunnel_id:
        return {"tunnel_id": None}
    cf = CF()
    zid = h.cloudflare.zone_id or cf.zone_id(h.public_zone)
    acct = h.cloudflare.account_id or cf.account_id(zid)
    t = cf._ok(cf.c.get(f"/accounts/{acct}/cfd_tunnel/{h.cloudflare.tunnel_id}"))
    return {"tunnel_id": t["id"], "name": t["name"], "status": t.get("status"),
            "connections": len(t.get("connections") or [])}

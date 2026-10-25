"""Enregistrements DNS Cloudflare de la zone publique (docs/conventions.md § DNS)."""
from __future__ import annotations

import httpx

from . import config, secrets
from .models import Host, Project

API = "https://api.cloudflare.com/client/v4"


class CF:
    def __init__(self, token: str | None = None):
        token = token or secrets.infra_env().get("CF_API_TOKEN")
        if not token:
            raise RuntimeError("CF_API_TOKEN absent de secrets/infra.env")
        self.c = httpx.Client(base_url=API, headers={"Authorization": f"Bearer {token}"}, timeout=30)

    def _ok(self, r: httpx.Response) -> dict:
        d = r.json()
        if not d.get("success"):
            raise RuntimeError(f"Cloudflare : {d.get('errors')}")
        return d["result"]

    def zone_id(self, zone: str) -> str:
        res = self._ok(self.c.get("/zones", params={"name": zone}))
        if not res:
            raise RuntimeError(f"zone {zone} introuvable pour ce token")
        return res[0]["id"]

    def account_id(self, zone_id: str) -> str:
        return self._ok(self.c.get(f"/zones/{zone_id}"))["account"]["id"]

    def records(self, zone_id: str) -> list[dict]:
        return self._ok(self.c.get(f"/zones/{zone_id}/dns_records", params={"per_page": 500}))

    def ensure(self, zone_id: str, name: str, type_: str, content: str, proxied: bool, force: bool = False) -> str:
        """Crée/corrige l'enregistrement. Retourne 'ok' | 'created' | 'updated' | 'conflict'."""
        existing = [r for r in self.records(zone_id) if r["name"] == name and r["type"] in ("A", "AAAA", "CNAME")]
        body = {"type": type_, "name": name, "content": content, "proxied": proxied, "ttl": 1}
        if not existing:
            self._ok(self.c.post(f"/zones/{zone_id}/dns_records", json=body))
            return "created"
        r = existing[0]
        if r["type"] == type_ and r["content"] == content and bool(r["proxied"]) == proxied and len(existing) == 1:
            return "ok"
        if not force:
            return "conflict"
        for extra in existing[1:]:
            self._ok(self.c.delete(f"/zones/{zone_id}/dns_records/{extra['id']}"))
        self._ok(self.c.put(f"/zones/{zone_id}/dns_records/{r['id']}", json=body))
        return "updated"

    def delete_name(self, zone_id: str, name: str) -> int:
        n = 0
        for r in self.records(zone_id):
            if r["name"] == name:
                self._ok(self.c.delete(f"/zones/{zone_id}/dns_records/{r['id']}"))
                n += 1
        return n


def _zone(cf: CF, h: Host) -> str:
    return h.cloudflare.zone_id or cf.zone_id(h.public_zone)


def wanted_records(name: str, prj: Project, h: Host) -> list[tuple[str, str, str, bool]]:
    """(name, type, content, proxied) voulus pour un projet."""
    d = config.domain_of(name, prj)
    out = []
    if h.public_ingress == "cloudflare-tunnel":
        if not h.cloudflare.tunnel_id:
            raise RuntimeError("host.yaml: cloudflare.tunnel_id manquant (infra tunnel create)")
        pub = ("CNAME", f"{h.cloudflare.tunnel_id}.cfargotunnel.com", True)
    else:
        pub = ("A", h.public_ip, False)
    if prj.expose == "public":
        out.append((f"{d}.{h.public_zone}", *pub))
    if prj.previews == "public":
        out.append((f"*.{d}.{h.public_zone}", *pub))
    if prj.previews == "private":
        out.append((f"*.{d}.{h.private_zone}", "A", h.tailscale_ip, False))
    for label, exp in prj.entries.items():
        if exp == "public":
            out.append((f"{label}.{h.public_zone}", *pub))
    return out


def project_names(name: str, prj: Project, h: Host) -> list[str]:
    d = config.domain_of(name, prj)
    return [f"{d}.{h.public_zone}", f"*.{d}.{h.public_zone}", f"*.{d}.{h.private_zone}",
            *(f"{label}.{h.public_zone}" for label in prj.entries)]


def ensure_project(name: str, prj: Project, h: Host, force: bool = False) -> dict[str, str]:
    cf = CF()
    zid = _zone(cf, h)
    wanted = wanted_records(name, prj, h)
    result = {}
    for rec_name, type_, content, proxied in wanted:
        result[rec_name] = cf.ensure(zid, rec_name, type_, content, proxied, force=force)
    # supprime ce qui n'est plus voulu (ex. previews désactivées)
    wanted_names = {w[0] for w in wanted}
    for n in project_names(name, prj, h):
        if n not in wanted_names and cf.delete_name(zid, n):
            result[n] = "deleted"
    return result


def prune_project(name: str, prj: Project, h: Host, public_only: bool = False) -> dict[str, str]:
    cf = CF()
    zid = _zone(cf, h)
    names = project_names(name, prj, h)
    if public_only:
        names = [n for n in names if n.endswith(h.public_zone) and not n.startswith("*.")]
    return {n: "deleted" for n in names if cf.delete_name(zid, n)}


def bootstrap(h: Host) -> dict[str, str]:
    cf = CF()
    zid = _zone(cf, h)
    return {f"*.{h.private_zone}": cf.ensure(zid, f"*.{h.private_zone}", "A", h.tailscale_ip, False, force=True)}


def list_records(h: Host) -> list[dict]:
    cf = CF()
    zid = _zone(cf, h)
    return [{"type": r["type"], "name": r["name"], "content": r["content"], "proxied": r.get("proxied", False)}
            for r in sorted(cf.records(zid), key=lambda r: r["name"])]


def check(h: Host) -> dict[str, bool]:
    recs = list_records(h)
    want = f"*.{h.private_zone}"
    return {want: any(r["name"] == want and r["content"] == h.tailscale_ip for r in recs)}

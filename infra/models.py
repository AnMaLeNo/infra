"""Schéma de host.yaml et registry.yaml."""
from __future__ import annotations

import re
from typing import Literal

from pydantic import BaseModel, Field, field_validator, model_validator

NAME_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")

Expose = Literal["none", "private", "public"]
Previews = Literal["none", "private", "public"]
Ingress = Literal["cloudflare-tunnel", "direct"]


class Cloudflare(BaseModel):
    zone_id: str = ""
    account_id: str = ""
    tunnel_id: str = ""


class Exception_(BaseModel):
    name: str
    reason: str


class Host(BaseModel):
    name: str
    tailscale_ip: str
    public_zone: str
    private_zone: str
    public_ingress: Ingress = "cloudflare-tunnel"
    public_ip: str | None = None
    cloudflare: Cloudflare = Field(default_factory=Cloudflare)
    stacks: list[str] = Field(default_factory=lambda: ["traefik", "cloudflared", "ui"])
    exceptions: list[Exception_] = Field(default_factory=list)
    notes: str = ""

    @model_validator(mode="after")
    def _direct_needs_ip(self):
        if self.public_ingress == "direct" and not self.public_ip:
            raise ValueError("public_ingress: direct requiert public_ip")
        return self


class Project(BaseModel):
    repo: str
    ref: str = "main"
    enabled: bool = True
    service: str = "compose"           # → skills/service/<service>
    expose: str = "private"            # → skills/expose/<expose>  (none|private|public|custom)
    domain: str | None = None          # défaut : nom du projet
    container: str | None = None       # service compose qui reçoit le trafic
    port: int | None = None            # port d'écoute interne
    previews: str = "none"             # none|private|public
    needs: list[str] = Field(default_factory=list)
    env: bool = False                  # secrets/projects/<P>.env
    expose_ports: list[str] = Field(default_factory=list)   # "1883/tcp" → lié à tailscale_ip
    data: list[str] = Field(default_factory=list)           # "<svc>:<chemin container>" → /srv/data/<P>/<svc>/...
    compose_file: str = "docker-compose.yml"
    build: str | None = None           # service static : commande de build
    static_dir: str = "."              # service static : dossier à servir
    legacy: str | None = None
    notes: str = ""

    @field_validator("domain")
    @classmethod
    def _one_label(cls, v):
        if v is not None and ("." in v or not NAME_RE.match(v)):
            raise ValueError("domain doit être un seul label [a-z0-9-]")
        return v

    @model_validator(mode="after")
    def _http_needs_port(self):
        if self.expose != "none" and self.service == "compose" and self.enabled and self.port is None:
            # toléré : le LLM le devine puis le fixe via `infra set`
            pass
        return self


class Registry(BaseModel):
    projects: dict[str, Project] = Field(default_factory=dict)

    @field_validator("projects")
    @classmethod
    def _names(cls, v):
        bad = [n for n in v if not NAME_RE.match(n)]
        if bad:
            raise ValueError(f"noms de projet invalides : {bad}")
        return v

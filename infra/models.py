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


Effort = Literal["low", "medium", "high", "xhigh", "max"]


class Llm(BaseModel):
    """Paramètres de `claude -p`, communs à tous les runs de l'hôte (réglables dans le front)."""
    model: str = "sonnet"              # alias (sonnet, opus…) ou nom complet
    effort: Effort = "high"


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
    llm: Llm = Field(default_factory=Llm)
    stacks: list[str] = Field(default_factory=lambda: ["traefik", "cloudflared", "ui"])
    exceptions: list[Exception_] = Field(default_factory=list)
    notes: str = ""

    @model_validator(mode="after")
    def _direct_needs_ip(self):
        if self.public_ingress == "direct" and not self.public_ip:
            raise ValueError("public_ingress: direct requiert public_ip")
        return self


# Champs du *comment*, déterminés et écrits par le LLM pendant le run (registry_set), jamais saisis
# par l'utilisateur ; ils n'entrent pas dans le hash « modifié depuis le dernier run ».
LLM_DECIDED = ("container", "port", "compose_file", "build", "static_dir")


class Project(BaseModel):
    repo: str
    ref: str = "main"
    enabled: bool = True
    service: str | None = None         # → skills/service/<service> ; None = le LLM choisit (compose|static) et l'écrit
    expose: str = "private"            # → skills/expose/<expose>  (none|private|public|custom)
    domain: str | None = None          # défaut : nom du projet
    container: str | None = None       # (LLM) service compose qui reçoit le trafic
    port: int | None = None            # (LLM) port d'écoute interne
    previews: str = "none"             # none|private|public
    needs: list[str] = Field(default_factory=list)
    env: bool = False                  # secrets/projects/<P>.env
    expose_ports: list[str] = Field(default_factory=list)   # "1883/tcp" → lié à tailscale_ip
    data: list[str] = Field(default_factory=list)           # "<svc>:<chemin container>" → /srv/data/<P>/<svc>/...
    compose_file: str = "docker-compose.yml"   # (LLM)
    build: str | None = None           # (LLM) service static : commande de build
    static_dir: str = "."              # (LLM) service static : dossier à servir
    legacy: str | None = None
    notes: str = ""

    @field_validator("domain")
    @classmethod
    def _one_label(cls, v):
        if v is not None and ("." in v or not NAME_RE.match(v)):
            raise ValueError("domain doit être un seul label [a-z0-9-]")
        return v

    def intent(self) -> dict:
        """L'entrée sans les champs décidés par le LLM : ce que l'utilisateur a voulu."""
        return self.model_dump(exclude=set(LLM_DECIDED))

    def decided(self) -> dict:
        """Les constats écrits par le LLM (valeurs non par défaut seulement)."""
        d = self.model_dump(exclude_defaults=True)
        return {k: d[k] for k in LLM_DECIDED if k in d}


class Registry(BaseModel):
    projects: dict[str, Project] = Field(default_factory=dict)

    @field_validator("projects")
    @classmethod
    def _names(cls, v):
        bad = [n for n in v if not NAME_RE.match(n)]
        if bad:
            raise ValueError(f"noms de projet invalides : {bad}")
        return v

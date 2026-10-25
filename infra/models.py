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
    lan_ip: str | None = None          # IP LAN de l'hôte (clients sans Tailscale : ESP, autres machines) ; requis par `listen: [lan]`
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
LLM_DECIDED = ("container", "port", "compose_file", "build", "static_dir", "consumers", "entry_ports", "pr_proposed")


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
    uses: list[str] = Field(default_factory=list)           # services partagés consommés (shared.yaml)
    env: bool = False                  # secrets/projects/<P>.env
    expose_ports: list[str] = Field(default_factory=list)   # "1883/tcp" → lié à tailscale_ip
    data: list[str] = Field(default_factory=list)           # "<svc>:<chemin container>" → /srv/data/<P>/<svc>/...
    compose_file: str = "docker-compose.yml"   # (LLM)
    build: str | None = None           # (LLM) service static : commande de build
    static_dir: str = "."              # (LLM) service static : dossier à servir
    # (LLM) service compose → variables d'env vers les `uses`, ex. {"api": {"MQTT_HOST": "mqtt", "MQTT_PORT": "1883"}}
    consumers: dict[str, dict[str, str]] = Field(default_factory=dict)
    # Points d'entrée supplémentaires du même `container` : label → expose (none|private|public), ex. {"naturopathe-dashboard": "public"}
    # → hostname <label>.<zone>, port constaté par le LLM dans `entry_ports`
    entries: dict[str, str] = Field(default_factory=dict)
    entry_ports: dict[str, int] = Field(default_factory=dict)   # (LLM) label → port interne
    # (LLM) PR ouverte sur le repo du projet pour le dockeriser (il n'avait ni Dockerfile ni compose) ; `ref` pointe sur sa
    # branche tant qu'elle n'est pas mergée, puis le run suivant repasse `ref: main` et efface ce champ
    pr_proposed: str | None = None
    legacy: str | None = None
    notes: str = ""

    @field_validator("entries")
    @classmethod
    def _entries(cls, v):
        for label, exp in v.items():
            if "." in label or not NAME_RE.match(label):
                raise ValueError(f"entries: `{label}` doit être un seul label [a-z0-9-]")
            if exp not in ("none", "private", "public"):
                raise ValueError(f"entries.{label}: expose doit être none|private|public (reçu {exp!r})")
        return v

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


Listen = Literal["lan", "tailnet"]
SHARED_LLM_DECIDED = ("image", "container", "compose_file")


class Shared(BaseModel):
    """Service partagé entre projets (shared.yaml) : broker MQTT, base de données… Un seul exemplaire par hôte,
    joignable par son nom sur traefik-net ; l'intention (kind, listen, port, auth) est humaine, le reste est constaté."""
    kind: str                          # ce que c'est : mqtt-broker, postgres… (libre, sert au LLM et à l'humain)
    enabled: bool = True
    listen: list[Listen] = Field(default_factory=lambda: ["tailnet"])   # IPs d'écoute sur l'hôte ; jamais 0.0.0.0
    port: int | None = None            # port publié sur ces IPs ; None = joignable seulement par les conteneurs (traefik-net)
    auth: str = "anonymous"            # anonymous | users (identifiants dans secrets/shared/<nom>.env)
    data: bool = True                  # données persistantes sous /srv/shared/<nom>/data
    image: str | None = None           # (LLM) image:tag retenue
    container: str | None = None       # (LLM) nom du conteneur
    compose_file: str = "docker-compose.yml"   # (LLM) dans <config>/shared/<nom>/
    notes: str = ""

    @field_validator("listen")
    @classmethod
    def _unique(cls, v):
        return list(dict.fromkeys(v))

    def intent(self) -> dict:
        return self.model_dump(exclude=set(SHARED_LLM_DECIDED))

    def decided(self) -> dict:
        d = self.model_dump(exclude_defaults=True)
        return {k: d[k] for k in SHARED_LLM_DECIDED if k in d}


class SharedRegistry(BaseModel):
    services: dict[str, Shared] = Field(default_factory=dict)

    @field_validator("services")
    @classmethod
    def _names(cls, v):
        bad = [n for n in v if not NAME_RE.match(n)]
        if bad:
            raise ValueError(f"noms de service partagé invalides : {bad}")
        return v


class Registry(BaseModel):
    projects: dict[str, Project] = Field(default_factory=dict)

    @field_validator("projects")
    @classmethod
    def _names(cls, v):
        bad = [n for n in v if not NAME_RE.match(n)]
        if bad:
            raise ValueError(f"noms de projet invalides : {bad}")
        return v

# Stacks d'infra

Services partagés, un dossier par stack, lancés par `infra stack up <nom>` (= `docker compose -p infra-<nom> --env-file /srv/stack/<nom>/.env up -d`). Le `.env` de chaque stack est généré par l'outil depuis `host.yaml` + `secrets/infra.env` ; l'état runtime (acme.json, credentials) vit dans `/srv/stack/<nom>/`.

| Stack | Rôle | Obligatoire |
|---|---|---|
| `traefik` | reverse proxy unique, certs Let's Encrypt DNS-01 | oui |
| `cloudflared` | tunnel Cloudflare → Traefik (`public_ingress: cloudflare-tunnel`) | si tunnel |
| `ui` | front + API de l'outil (`https://infra.<private_zone>`) | oui |
| `mosquitto` | broker MQTT partagé (projets avec `needs: [mosquitto]`) | non |

## Personnalisation par hôte (repo de config)

- `<config>/stack/<nom>/docker-compose.yml` : stack propre à l'hôte (prioritaire sur `tool/stack/<nom>`), lancée comme les autres si elle est dans `host.yaml: stacks`. Ex. un pont `socat` vers un service legacy qui n'écoute que sur `127.0.0.1`.
- `<config>/stack/traefik/dynamic/*.yml` : routeurs/services/certs Traefik supplémentaires, fusionnés avec ceux de l'outil dans `/srv/stack/traefik/dynamic/` à chaque `infra stack up traefik`.
- `/srv/stack/traefik/certs/` : certificats fournis par l'hôte (ex. `tailscale cert`), montés en `/etc/traefik/certs`.
- `TRAEFIK_NET_GATEWAY` (env des stacks et de Traefik, go-template `{{ env "TRAEFIK_NET_GATEWAY" }}`) : IP de l'hôte vue depuis `traefik-net` (réseau créé en `172.30.0.0/24`).

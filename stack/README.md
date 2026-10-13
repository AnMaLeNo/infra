# Stacks d'infra

Services partagés, un dossier par stack, lancés par `infra stack up <nom>` (= `docker compose -p infra-<nom> --env-file /srv/stack/<nom>/.env up -d`). Le `.env` de chaque stack est généré par l'outil depuis `host.yaml` + `secrets/infra.env` ; l'état runtime (acme.json, credentials) vit dans `/srv/stack/<nom>/`.

| Stack | Rôle | Obligatoire |
|---|---|---|
| `traefik` | reverse proxy unique, certs Let's Encrypt DNS-01 | oui |
| `cloudflared` | tunnel Cloudflare → Traefik (`public_ingress: cloudflare-tunnel`) | si tunnel |
| `ui` | front + API de l'outil (`https://infra.<private_zone>`) | oui |
| `mosquitto` | broker MQTT partagé (projets avec `needs: [mosquitto]`) | non |

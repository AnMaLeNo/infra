---
name: bootstrap-host
description: Amener un serveur (OS vierge ou existant) à l'état où l'outil peut y déployer des projets. Idempotent.
---
## But
L'hôte a Docker, Tailscale, les outils (sops, age, uv, claude), le compte `ai-infra`, les dossiers `/srv/*`, le CLI `infra`, le runner, et les stacks d'infra (Traefik, tunnel, UI, optionnelles) qui tournent ; `infra doctor` est tout vert.

## Préalables humains
Repo de config cloné avec `--recursive` en `/srv/infra` ; clé age privée disponible ; token Cloudflare (Zone.DNS Edit + Account.Cloudflare Tunnel Edit) et token Claude dans `secrets/infra.env` ; accès au compte Tailscale pour approuver le nœud (ou `TS_AUTHKEY`).

## État cible (dans l'ordre naturel des dépendances)
1. `sudo /srv/infra/tool/install.sh` passé : `infra --version`, `id ai-infra`, `systemctl is-enabled infra-runner`.
2. `/srv/infra/.age-key` présent (0400, `ai-infra`) et `infra secrets check` tout `ok`.
3. `tailscale ip -4` = `host.yaml: tailscale_ip` (sinon corrige host.yaml).
4. `infra stack up traefik` : réseau `traefik-net`, Traefik up, `curl -sk https://<tailscale_ip>/` → 404 Traefik.
5. `infra dns bootstrap` : `A *.<private_zone> → <tailscale_ip>` ; `dig +short x.<private_zone>` répond.
6. Si `public_ingress: cloudflare-tunnel` : `infra tunnel create` (une fois ; écrit `tunnel_id` dans host.yaml et `secrets/cloudflared.json`) puis `infra stack up cloudflared` ; `docker logs infra-cloudflared` montre 4 `Registered tunnel connection`.
7. `infra stack up ui` et `sudo systemctl enable --now infra-runner` ; `https://infra.<private_zone>` s'affiche depuis le tailnet.
8. Stacks optionnelles de `host.yaml: stacks` (`infra stack up --all`).
9. Commit de `host.yaml` + `secrets/cloudflared.json` dans le repo de config. Les projets du registre ne sont **pas** déployés à cette étape : rien ne se lance sans un clic sur « Lancer Claude » (ou `infra enqueue`), projet par projet — l'outil peut donc être installé à côté de services existants.
10. `infra doctor` : tout `ok`.

## Pièges connus
- Un wildcard DNS plus large (`*.<public_zone>`) qui pointe ailleurs rend les sous-domaines imprévisibles : signale-le.
- Tailscale : si l'hôte publie déjà des ports sur `0.0.0.0` (legacy), `infra doctor` les liste — ce n'est pas bloquant, c'est le travail de `migrate-legacy`.

---
name: bootstrap-host
description: Préparer un serveur (OS vierge ou existant) à faire tourner l'outil : dépendances, comptes, Tailscale, Traefik, tunnel, UI. Idempotent.
---
# Bootstrap d'un hôte

## Quand
Nouveau serveur, réinstallation de l'OS, ou vérification qu'un hôte est conforme.

## Préconditions (faites par un humain)
- OS Debian/Ubuntu 64 bits, accès SSH avec sudo.
- Le repo de config existe (`host.yaml`, `secrets/` chiffrés, sous-module `tool/`).
- La clé privée age est disponible (gestionnaire de mots de passe).
- Dans Cloudflare : zone `<public_zone>` ; un token API (Zone.DNS Edit + Account.Cloudflare Tunnel Edit) dans `secrets/infra.env`.
- Dans Tailscale : l'hôte sera ajouté au tailnet pendant l'étape 2 (login interactif ou auth key dans `secrets/infra.env: TS_AUTHKEY`).

## Étapes
1. **Cloner** : `sudo git clone --recursive <repo-config> /srv/infra && sudo /srv/infra/tool/install.sh`. Le script : installe docker, git, age, sops, python3/uv, tailscale ; crée `ai-infra` (groupe `docker`, sudo limité, voir `agent-account`), `/srv/{apps,data,stack}` ; installe le CLI `infra` ; installe `infra-runner.service`. Vérifie : `infra --version`, `id ai-infra`.
2. **Clé age** : colle la clé dans `/srv/infra/.age-key` (`chmod 0400`, propriétaire `ai-infra`). Vérifie : `infra secrets check` déchiffre tout.
3. **Tailscale** : `sudo tailscale up --ssh=false` (ou `--auth-key`). Vérifie `tailscale ip -4` = `host.yaml: tailscale_ip` ; sinon mets à jour `host.yaml`.
4. **Réseau + Traefik** : `infra stack up traefik`. Crée `traefik-net`, démarre Traefik avec la config correspondant à `public_ingress`. Vérifie : `docker ps | grep traefik`, `curl -sk https://<tailscale_ip>/` → 404 Traefik.
5. **DNS privé** : `infra dns bootstrap` → `A *.<private_zone> → <tailscale_ip>`. Vérifie `dig +short x.<private_zone>`.
6. **Tunnel** (si `cloudflare-tunnel`) : s'il n'y a pas encore `secrets/cloudflared.json` : `infra tunnel create` (crée le tunnel nommé `<host.name>` via l'API, chiffre les credentials dans `secrets/cloudflared.json`, écrit `tunnel_id` dans `host.yaml`, commit à faire). Puis `infra stack up cloudflared`. Vérifie : `docker logs cloudflared --tail 20` → `Registered tunnel connection` ×4.
7. **UI + runner** : `infra stack up ui` ; `sudo systemctl enable --now infra-runner`. L'UI est un projet interne `expose: private` sur `https://infra.<private_zone>`. Vérifie dans un navigateur du tailnet.
8. **Stacks optionnelles** (`host.yaml: stacks:` : `mosquitto`, `netdata`…) : `infra stack up --all`.
9. **Projets** : `infra apply --all` applique le registre (ou laisse le runner le faire projet par projet).
10. **Commit** : `host.yaml` (tunnel_id, tailscale_ip) et `secrets/cloudflared.json` dans le repo de config.

## Vérification finale
`infra doctor` : tout vert (docker, tailscale, traefik, tunnel, dns, age, runner).

## Idempotence
Chaque étape teste avant d'agir. `install.sh` et `stack up` peuvent être relancés à volonté.

## Sécurité hôte (fait par install.sh, à connaître)
- SSH : clés uniquement, `PasswordAuthentication no`.
- Pas de pare-feu requis sur le Pi (rien n'écoute sur 0.0.0.0 sauf SSH et Tailscale) ; `infra doctor` liste tout socket sur `0.0.0.0`/`[::]` hors liste blanche.
- `unattended-upgrades` activé.

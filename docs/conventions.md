# Conventions

Tout ce qui est écrit ici est **la** façon de faire. Si un skill ou un projet s'en écarte, c'est une erreur à corriger, pas une variante.

## Chemins sur l'hôte

| Chemin | Contenu | Versionné |
|---|---|---|
| `/srv/infra/` | clone du repo de config (`host.yaml`, `registry.yaml`, `secrets/`, `skills/`) | oui (repo privé) |
| `/srv/infra/tool/` | sous-module : cet outil, épinglé sur une version | oui (repo public) |
| `/srv/infra/.age-key` | clé privée age (0400, `ai-infra`) | **non** — gestionnaire de mots de passe |
| `/srv/infra/runs/` | journaux des runs du LLM (`<date>-<projet>.json`) | non |
| `/srv/apps/<projet>/repo/` | clone du repo du projet, sur `ref` | non (c'est un clone) |
| `/srv/apps/<projet>/compose.override.yml` | généré par `infra render` — **ne jamais éditer à la main** | non |
| `/srv/apps/<projet>/.env` | déchiffré depuis `secrets/projects/<projet>.env` (0600) | non |
| `/srv/apps/<projet>/previews/pr-<N>/` | idem pour une preview | non |
| `/srv/data/<projet>/` | données persistantes (bind mounts) — c'est ce qu'on sauvegarde | non |
| `/srv/stack/<nom>/` | état runtime des stacks d'infra (acme.json, credentials tunnel) | non |

`/home/*` ne contient plus aucun service.

## Docker

- Tout tourne en Docker Compose. Exceptions autorisées, listées dans `host.yaml: exceptions:` avec une raison (matériel, process qui lance `claude`…).
- Nom de projet compose = nom du projet dans le registre (`docker compose -p <projet>`). Previews : `<projet>-pr-<N>`.
- Réseau partagé : `traefik-net` (externe, créé au bootstrap). Seul le service `container` du projet le rejoint.
- **Aucun `ports:` publié** par les projets. `infra render` les neutralise (`ports: !reset []`). Si un projet a vraiment besoin d'un port (UDP, protocole non HTTP), il le déclare dans `expose_ports:` et le port est lié à l'IP Tailscale uniquement.
- Les volumes nommés sont remplacés par des bind mounts sous `/srv/data/<projet>/` quand les données doivent survivre à un `down -v`.
- `restart: unless-stopped` partout.
- Images : tag explicite ou build local ; jamais `latest` dans les stacks d'infra.

## Traefik

- Entrypoints : `tailnet` (`<tailscale_ip>:443`, TLS), `tailnet-http` (`<tailscale_ip>:80`, redirection), `tunnel` (`:8080`, HTTP, **non publié** — joignable uniquement par cloudflared dans `traefik-net`). Hôtes en `public_ingress: direct` (VPS) : `web` (`:80`) et `websecure` (`:443`).
- Certificats : resolver `cf` = ACME DNS-01 Cloudflare, wildcard `*.<private_zone>` demandé une fois.
- Labels générés (jamais écrits à la main dans les repos projets) :
  - routeur `<projet>-private` : `Host(\`<domain>.<private_zone>\`)`, entrypoint `tailnet`, tls
  - routeur `<projet>-public` (si `expose: public`) : `Host(\`<domain>.<public_zone>\`)`, entrypoint `tunnel` (ou `websecure` en direct)
  - service `<projet>` : `loadbalancer.server.port=<port>`
- Une app `private` n'a **aucun** routeur sur l'entrypoint `tunnel` : même si un CNAME traînait, elle n'est pas servie.

## DNS (Cloudflare, zone `<public_zone>`)

| Enregistrement | Valeur | Qui le crée |
|---|---|---|
| `A *.<private_zone>` | `<tailscale_ip>` (non proxied) | bootstrap |
| `CNAME <domain>.<public_zone>` | `<tunnel_id>.cfargotunnel.com` (proxied) | `infra dns ensure <projet>` |
| `CNAME *.<domain>.<public_zone>` | idem | `infra dns ensure` si `previews: public` |
| `A *.<domain>.<private_zone>` | `<tailscale_ip>` | `infra dns ensure` si `previews: private` |

Rendre un projet public = créer **un** CNAME. Le tunnel a une seule règle d'ingress : tout → `http://traefik:8080`.

## Noms

- Projet : `[a-z0-9-]+`, en général le nom du repo en minuscules.
- `domain` : un seul label (`catacombes`, pas `catacombes.antoine42.com`).
- Compte agent IA : `ai-<outil>` (`ai-infra` pour le runner, `ai-devin`, `ai-claude`…). Un compte par agent, jamais partagé.

## Secrets

- Chiffrés avec sops + age dans le repo de config : `secrets/infra.env` (tokens Cloudflare, email ACME…), `secrets/cloudflared.json` (credentials du tunnel), `secrets/projects/<projet>.env`.
- Déchiffrés à la demande par `infra render` vers l'hôte (0600, propriétaire `ai-infra`). Jamais dans les logs, jamais dans les runs du LLM (le runner les masque), jamais en clair dans git.
- Édition : `infra secrets edit projects/<projet>.env` (= `sops`).

## Idempotence

Chaque commande `infra` et chaque skill doivent pouvoir être rejoués sans effet si l'état est déjà le bon. Toujours : lire l'état réel (`infra status <projet>`), comparer à l'état voulu, n'agir que sur l'écart.

## Journal

Chaque run du LLM écrit `/srv/infra/runs/<horodatage>-<projet>.json` (entrée du registre avant/après, skills utilisés, sortie, succès/échec). L'UI les affiche.

# Inventaire (13 oct 2026)

## Raspberry Pi `raspberrypi` (Tailscale 100.72.6.43, raspberrypi.tail145c71.ts.net)
- Pi 5, Debian 12 bookworm aarch64, 8 Go RAM + zram 4 Go, NVMe 931 Go (227 utilisés), uptime 90 j. Docker 29.6 + compose v5.3, node 22, python 3.11, netdata (127.0.0.1:19999), samba (partage Stockage=/home/amonot/storage), earlyoom, smartd, unattended-upgrades. Pas de pare-feu (ufw absent). SSH port 22 (clé), sudo NOPASSWD pour amonot/agent/alpage.
- Comptes : amonot (humain, uid 1000, home 0700), agent (uid 1001, « Agent autonome (devin-ibkr) », sudo total enregistré sudoreplay), alpage (uid 1002, « Agent autonome (devin-alpage-blanche) », idem), unused (uid 995, service).
- Tailscale : `tailscale serve` → https://raspberrypi.tail145c71.ts.net/ → 127.0.0.1:8799 (IBKR notify PWA, tailnet only). Funnel essayé dans l'historique (`tailscale funnel 8021`, `5174`) mais plus actif.
- Cloudflare Tunnel : container `cloudflared` (compose /home/amonot/cloudflared, image cloudflare/cloudflared:latest, network_mode host, token CF_TUNNEL_TOKEN dans .env, tunnel remote-managed → hostnames configurés dans le dashboard Zero Trust). Au moins catacombes.antoine42.com → 127.0.0.1:8420.
- Quick tunnels ad hoc (trycloudflare.com) lancés à la main (`cloudflared tunnel --url http://localhost:8787/8788`) pour naturopathe-questionnaire (apps/server, `npm run start`, logs ~/npq-*.log) — process orphelins (ppid 1), non persistants.

### Services qui tournent sur le Pi
| Projet | Mode de lancement | Exposition | Port |
|---|---|---|---|
| catacombes | systemd `catacombes.service` + timer data (python nu, cwd /home/amonot/catacombes/app) | public via Cloudflare Tunnel (catacombes.antoine42.com) | 127.0.0.1:8420 |
| IBKR (ibgateway, ibcollector, ibnotify, ibbackup/ibcache timers, notify-failure@) | systemd, venv python, cwd /home/amonot/IBKR | PWA notify via tailscale serve | 127.0.0.1:8799, 4002 (API IB, *:4002 !) |
| unused | systemd `unused.service` (user unused, /opt/unused, node) | aucune | — |
| piproxy | systemd (Windows-to-Mac-Low-Latency-Vision-Pipeline/pi-agent) | matériel | — |
| sous-la-barre | systemd --user (deploy/sous-la-barre.service), uvicorn | LAN/tailnet 0.0.0.0 | 8310 |
| notes-app | uvicorn lancé à la main (ppid 1) | 0.0.0.0 | 8300 |
| naturopathe-questionnaire | `npm run start` à la main + 2 quick tunnels | trycloudflare (admin 8787, patient 8788) | 127.0.0.1:8787/8788 |
| ApplyBoard | docker compose (/home/amonot/ApplyBoard) nginx reverse-proxy + api + front + postgres | port publié | 0.0.0.0:8042 |
| fan_controller | docker compose | port publié | 0.0.0.0:18081 |
| pc-power-controller | docker compose (+override dev !) | port publié | 0.0.0.0:18080, api 0.0.0.0:3000 |
| mosquitto | docker compose (/home/amonot/mosquitto), infra partagée MQTT | LAN | 0.0.0.0:1883 |
| Naturopathe-website | docker compose (nginx static) | port publié (+ tunnel ?) | 0.0.0.0:8021 |
| projet-web (mon-serveur-nginx) | docker compose | port publié | 0.0.0.0:8080 |
| camofox-browser | docker run manuel | port publié | 0.0.0.0:9377 |
| alpage-blanche-gestion | docker compose, containers Exited depuis 3 mois (prod déplacée sur le VPS) | — | — |
| immich-app | compose présent, arrêté | — | — |
| chromium (--remote-debugging 9222), obsidian (27123/27124), vscode-server | session desktop amonot | — | — |
- Dépôts clonés dans /home/amonot (≈ 20) + /home/amonot/projects (8, dont site-alpage-blanche, site-naturopathe-fb = sites clients).

## VPS `vps-77c98128` (37.187.219.65:52336, Tailscale 100.99.13.85)
- Ubuntu 26.04, 2 vCPU, 3,7 Go RAM + swap 2 Go, 38 Go disque. Docker 29.5 / compose 5.1. Tailscale installé (membre du tailnet). SSH durci (clé seulement, fail2ban). Pas de ufw. Clés autorisées : macbook, github-action-vps, devin-alpage-blanche.
- Traefik v3 (~/traefik : docker-compose.yml, traefik.yml, acme.json, .env CLOUDFLARE_DNS_API_TOKEN) : entrypoints 80→443, provider docker exposedByDefault=false, certresolver `cf` = ACME DNS-01 Cloudflare (email placeholder « ton@email.com »). Réseau externe `traefik-net` (+ `web` vide, inutilisé).
- Prod alpage : /opt/alpage (git main), docker-compose.yml + override non versionné (labels traefik Host alpage.antoine42.com → frontend:5173 = Vite dev server), service backup (pg_dump horaire, rétention 72), déployé par GitHub Actions `deploy-production.yml` (ssh appleboy → scripts/.github/scripts/deploy-production.sh : git reset --hard origin/main ; compose up -d --build ; image prune).
- Previews alpage : workflow `preview-deploy.yml` sur PR → ssh → `deploy-preview.sh` (clone /opt/alpage-previews/pr-N, .env généré, compose -p alpage-pr-N -f docker-compose.yml -f docker-compose.preview.yml, seed depuis dernier dump prod, labels traefik Host pr-N.alpage.antoine42.com, lock flock par PR, vérifie état PR via API), `preview-teardown.yml` à la fermeture. Commentaire sticky avec l'URL. Previews actives : pr-102, 103, 108, 112 (102/103 probablement zombies : PR fermées ?).
- ~/ia-bastion : image sshd Alpine (port 2222, /home monté ro) — non lancée. Expérience « bastion pour agents IA ».
- DNS alpage.antoine42.com → A 37.187.219.65 direct (pas proxied), ports 80/443 ouverts sur le VPS (normal).

## DNS antoine42.com (Cloudflare, NS renan/lorna)
- catacombes.antoine42.com → proxied Cloudflare (104.21.69.207/172.67.213.24) = Tunnel → Pi.
- alpage.antoine42.com, pr-N.alpage.antoine42.com → 37.187.219.65 (VPS, direct).
- Wildcard *.antoine42.com (et www, catacombe, maptracer…) → A 100.69.80.65 = `hermes-server` (nœud Tailscale hors ligne depuis 97 j). Stale.

## Constat : 7 façons de « déployer »
1. systemd + process nu (catacombes, IBKR, unused, piproxy, sous-la-barre user unit)
2. process lancé à la main / orphelin (notes-app, naturopathe-questionnaire)
3. docker compose + port publié sur 0.0.0.0 (ApplyBoard, fan, power, mosquitto, naturo-site, projet-web, camofox)
4. tailscale serve (IBKR notify) / tailscale funnel (essais)
5. Cloudflare Tunnel remote-managed (catacombes)
6. Cloudflare quick tunnel trycloudflare (questionnaire)
7. VPS : Traefik + labels + GitHub Actions ssh (alpage prod & previews)
- Autres incohérences : ports publiés en 0.0.0.0 donc visibles sur le LAN et le tailnet sans auth ; override de dev actif en prod (pc-power) ; API IB sur *:4002 ; repos et données mélangés dans /home/amonot ; comptes agents créés à la main (agent, alpage) sans règle ; token tunnel dans un .env non sauvegardé ; secrets non centralisés.

## DNS antoine42.com — liste complète via API (token Traefik, lecture seule)
- A      *.alpage.antoine42.com  → 37.187.219.65 (VPS, non proxied)
- A      *.antoine42.com         → 100.69.80.65 (hermes-server, mort) — STALE
- A      alpage.antoine42.com    → 37.187.219.65
- CNAME  catacombes.antoine42.com → f2c8d9f4-….cfargotunnel.com (proxied) = seul hostname du tunnel
- Le token Traefik n'a pas les droits Tunnel (10000) ; login dashboard bloqué par Turnstile dans mon navigateur.

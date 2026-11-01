# Architecture cible — Raspberry Pi + VPS

> **Décisions prises (13 oct 2026)** : Traefik ; privé `*.pi.antoine42.com` → IP Tailscale + certs DNS-01 ; public = un Cloudflare Tunnel → Traefik, CNAME par app ; tunnel géré localement ; secrets sops+age ; LLM = `claude -p`.
> **Changement par rapport à la proposition initiale** : l'outil est **générique** (ce repo, public) et **ne gère que le serveur où il est installé**. Chaque serveur a son propre repo de config privé (`infra-raspberry-pi`, `infra-vps`) qui embarque l'outil en sous-module épinglé. Les sections 3, 4 et 8 ci-dessous sont donc remplacées par `README.md`, `example-config/` et `docs/decisions/`. Le reste (constat, principes, exposition, secrets, comptes agents, previews, plan de migration) reste valable.

*Proposition v1 validée. L'inventaire détaillé de l'existant est dans `docs/inventaire-legacy.md`.*

## 0. Ce qu'on a aujourd'hui (résumé)

7 façons de déployer coexistent : systemd + process nu (catacombes, IBKR, unused, piproxy, sous-la-barre), process lancés à la main (notes, questionnaire), docker compose + port publié sur `0.0.0.0` (ApplyBoard, fan, power, mosquitto, site naturo, projet-web, camofox), `tailscale serve` (IBKR), Cloudflare Tunnel remote-managed (catacombes), quick tunnels `trycloudflare.com` (questionnaire), et sur le VPS Traefik + labels + GitHub Actions SSH (alpage prod + previews).

Problèmes concrets : ports ouverts sur tout le LAN et le tailnet sans auth, override de dev actif en prod (pc-power), API IB Gateway sur `*:4002`, repos et données mélangés dans `/home/amonot`, token du tunnel dans un `.env` non sauvegardé, 2 comptes agents créés à la main (`agent`, `alpage`) partagés entre plusieurs IA, DNS wildcard `*.antoine42.com` qui pointe sur une machine morte (`hermes-server`), containers zombies (alpage sur le Pi, previews pr-102/103 sur le VPS), email ACME placeholder sur le VPS.

Ce qui est déjà bien et qu'on garde comme modèle : **la mécanique alpage sur le VPS** (Traefik + réseau `traefik-net` + labels, certs DNS-01 Cloudflare, previews par PR avec lock, seed de la DB, teardown à la fermeture).

## 1. Principes

1. **Une seule façon de faire par besoin**, écrite dans un skill. Zéro commande « à la main » sur le serveur.
2. **Tout en Docker Compose**, y compris l'infra (Traefik, cloudflared, Mosquitto). Sur l'hôte il ne reste que : OS, Tailscale, Docker, SSH, netdata/smartd. Exceptions explicitement listées (matériel : piproxy/GPIO ; IB Gateway tant qu'il n'est pas conteneurisé).
3. **Aucun port publié par les apps.** Un seul point d'entrée : Traefik. Les apps rejoignent le réseau `traefik-net` ; Traefik route par nom d'hôte. (Infra partagée type MQTT : port lié à l'IP Tailscale uniquement.)
4. **Jamais de port ouvert sur la box.** Public = Cloudflare Tunnel (sortant). Privé = Tailscale.
5. **Même modèle sur le Pi et le VPS** : même Traefik, mêmes skills, mêmes conventions. Seul le mode d'entrée publique diffère (tunnel sur le Pi, ports 80/443 sur le VPS).
6. **Le repo projet reste générique** : il fournit juste un `docker-compose.yml` qui marche avec `docker compose up`. Les labels Traefik, le domaine, les `.env` sont générés par le repo infra à partir du registre. Rien de spécifique à « mon serveur » dans les repos projets.
7. **Déclaratif et idempotent** : `registry.yaml` décrit l'état voulu ; appliquer deux fois ne change rien. C'est ce qui permet le pilotage par LLM (et plus tard par un CLI).
8. **Reconstruction depuis zéro** : OS vierge + ce repo + les secrets (coffre) ⇒ tout remonte.

## 2. Noms de domaine et exposition

| Exposition | Qui y accède | URL | Mécanisme |
|---|---|---|---|
| `none` | personne (daemon, collecteur, broker) | — | tourne, pas de route HTTP |
| `private` | moi, via Tailscale (Mac, téléphone, VM Devin…) | `https://<app>.pi.antoine42.com` | DNS `*.pi.antoine42.com → 100.72.6.43` (IP Tailscale, injoignable depuis internet). Traefik écoute sur l'IP Tailscale, certificat Let's Encrypt wildcard via DNS-01 Cloudflare ⇒ HTTPS valide, pas de warning |
| `public` | tout le monde | `https://<app>.antoine42.com` | CNAME `<app>` → tunnel Cloudflare (proxied). cloudflared dans `traefik-net`, ingress unique « tout → `http://traefik:80` ». Rendre une app publique = **créer un CNAME** (API Cloudflare), rien d'autre |

Previews (par PR) : `https://pr-<N>.<app>.antoine42.com` (public, pour un client) ou `https://pr-<N>.<app>.pi.antoine42.com` (privé). Un enregistrement wildcard `*.<app>...` par projet suffit pour toutes ses PR.

Sur le VPS c'est pareil avec `*.vps.antoine42.com → 100.99.13.85` pour le privé et des A records vers `37.187.219.65` pour le public (Traefik y écoute déjà sur 80/443). Pas de tunnel nécessaire mais possible pour masquer l'IP.

Sécurité Traefik : deux entrypoints séparés — `tailnet` (IP Tailscale:443, TLS) et `tunnel` (HTTP, joignable uniquement par cloudflared dans le réseau Docker). Une app `private` n'a de routeur que sur `tailnet` ⇒ même si un CNAME traînait, elle n'est pas servie en public.

À faire côté DNS : supprimer/repointer `*.antoine42.com → 100.69.80.65` (hermes-server). Tant qu'il existe, tout sous-domaine inconnu tombe dans le vide. 

## 3. Le registre : `registry.yaml`

```yaml
hosts:
  pi:  { tailscale_ip: 100.72.6.43,  private_zone: pi.antoine42.com,  public: cloudflare-tunnel }
  vps: { tailscale_ip: 100.99.13.85, private_zone: vps.antoine42.com, public: direct }

projects:
  catacombes:
    repo: https://github.com/AnMaLeNo/catacombes
    host: pi
    enabled: true
    expose: public            # none | private | public
    domain: catacombes        # ⇒ catacombes.antoine42.com (défaut = nom du projet)
    service: web              # service compose qui reçoit le trafic HTTP
    port: 8420
    previews: off             # off | private | public
    env: vault                # les variables viennent du coffre (voir §5) ; ou "none"

  fan_controller:
    repo: https://github.com/AnMaLeNo/fan_controller
    host: pi
    enabled: true
    expose: private           # ⇒ https://fan-controller.pi.antoine42.com
    service: frontend
    port: 80
    depends_on_infra: [mosquitto]

  alpage-blanche-gestion:
    repo: https://github.com/AnMaLeNo/alpage-blanche-gestion
    host: vps
    enabled: true
    expose: public
    domain: alpage
    service: frontend
    port: 5173
    previews: public          # ⇒ pr-N.alpage.antoine42.com (= comportement actuel)

  Unused:
    repo: https://github.com/AnMaLeNo/Unused
    host: pi
    enabled: true
    expose: none              # daemon sans HTTP

  immich:
    repo: https://github.com/immich-app/immich
    host: pi
    enabled: false            # décrit mais pas déployé
```

Minimum à renseigner : `repo` + `expose`. Tout le reste a une valeur par défaut (host=pi, domain=nom, service/port devinés depuis le compose ou demandés par le LLM). L'interface web de plus tard ne fait qu'éditer ce fichier.

## 4. Layout du repo infra (nom proposé : `AnMaLeNo/infra`)

```
infra/
├── README.md                 # point d'entrée : « tu es un LLM, lis ça d'abord »
├── registry.yaml             # état voulu (§3)
├── skills/                   # 1 SKILL.md par opération, écrits pour un LLM
│   ├── deploy/SKILL.md       # appliquer un projet du registre (none/private/public), idempotent
│   ├── previews/SKILL.md     # activer les previews par PR d'un projet
│   ├── undeploy/SKILL.md     # enabled:false ⇒ arrêter, garder ou purger les données
│   ├── bootstrap-host/SKILL.md   # OS vierge → hôte prêt (Tailscale, Docker, Traefik, cloudflared…)
│   ├── agent-account/SKILL.md    # créer un compte pour un agent IA (§6)
│   └── secrets/SKILL.md      # ajouter/lire un secret
├── hosts/
│   ├── pi/                   # compose de l'infra partagée : traefik/, cloudflared/, mosquitto/, netdata/
│   │   ├── bootstrap.sh      # idempotent
│   │   └── agents/*.pub      # clés SSH des agents (source de vérité des authorized_keys)
│   └── vps/                  # idem (traefik/ seulement)
├── apps/<projet>/            # uniquement si le repo projet ne suffit pas : compose.override.yml, notes
├── docs/
│   ├── conventions.md        # noms, chemins, ports, réseaux, labels, logs, backups
│   ├── decisions/            # mini ADR : pourquoi Traefik, pourquoi tunnel vs funnel, etc.
│   └── inventaire-legacy.md  # état avant migration (celui que je viens de faire)
└── tools/                    # plus tard : CLI `infra plan|apply`, web UI, appel LLM
```

Layout sur l'hôte (Pi et VPS identiques) :
```
/srv/infra/                 clone de ce repo
/srv/apps/<projet>/         clone du repo projet + .env + compose.override.yml généré
/srv/apps/<projet>/previews/pr-N/
/srv/data/<projet>/         volumes / bind mounts (ce qu'on sauvegarde)
/srv/infra/hosts/pi/*/      traefik, cloudflared, mosquitto (compose + config, versionnés)
```
`/home/amonot` redevient un home personnel.

## 5. Secrets

Les `.env` ne sont jamais dans git en clair. Deux options :
- **A (recommandée)** : `sops` + `age`. Les `.env` chiffrés sont dans le repo (`apps/<projet>/env.sops`), la clé `age` privée est sur l'hôte (`/srv/infra/.age-key`, hors git) et dans ton gestionnaire de mots de passe. Reconstruction complète possible depuis GitHub + 1 clé. L'UI web de plus tard chiffre à l'écriture.
- **B** : `.env` uniquement sur l'hôte dans `/srv/apps/<projet>/.env`, sauvegardés via le backup de `/srv`. Plus simple, mais la reconstruction dépend du backup.

Secrets d'infra concernés : token/credentials du tunnel Cloudflare, token API Cloudflare (DNS edit + Tunnel edit — à créer, celui du VPS ne fait que DNS), clé Tailscale OAuth pour GitHub Actions, clé `age`.

## 6. Comptes agents IA

Règle : **un compte Unix par agent / par outil**, jamais partagé. Nom `ai-<outil>` (`ai-devin`, `ai-claude`…). Création par le skill `agent-account` :
- user sans mot de passe, clé SSH dans `hosts/<host>/agents/<nom>.pub` (le bootstrap régénère `authorized_keys` depuis le repo ⇒ révoquer = supprimer le fichier + PR),
- groupe `docker` + groupe `deploy` (écriture sur `/srv/apps`, `/srv/data`),
- `sudo` NOPASSWD **limité** à une liste (`systemctl`, `tailscale`, `apt`) + `log_output` sudoreplay comme aujourd'hui,
- `/srv/infra` cloné en lecture + droit de push sur une branche (PR obligatoire pour `main`).
Les comptes `agent` et `alpage` actuels sont migrés puis supprimés.

## 7. Previews par PR (généralisation d'alpage)

- Le repo projet ajoute **un** workflow de 5 lignes : `uses: AnMaLeNo/infra/.github/workflows/preview.yml@main` (workflow réutilisable, dans le repo infra).
- Le workflow rejoint le tailnet avec l'action Tailscale (nœud éphémère, clé OAuth) puis `ssh ai-github@<host> infra preview up <projet> <N>` ; teardown à la fermeture de la PR ; commentaire sticky avec l'URL. Plus besoin de SSH exposé sur internet (⇒ on peut fermer le 52336 du VPS).
- Côté hôte : les scripts actuels d'alpage (`deploy-preview.sh`, lock flock, seed DB, teardown) deviennent génériques dans `tools/` et lisent le registre.

## 8. Pilotage par LLM (phase suivante)

1. Court terme : toi (ou moi) modifie `registry.yaml` → un LLM lit `skills/deploy/SKILL.md` et applique. Tout est déjà écrit pour ça.
2. Moyen terme : CLI `infra plan` (diff état voulu/état réel) + `infra apply` (idempotent). Le LLM n'intervient que pour ce qui n'est pas mécanique (deviner service/port, écrire un override, débloquer un build).
3. Long terme : web UI (sur le Pi, `private`) qui édite le registre, commit, et déclenche une session LLM (API Devin ou Claude Code sur l'hôte) avec l'entrée modifiée + le skill.

## 9. Plan de migration (sans casser ce qui tourne)

0. Créer le repo, docs, skills, `registry.yaml` reflétant l'existant.
1. Pi : Traefik + nouveau cloudflared (tunnel recréé en mode « config dans git », ingress → traefik) **à côté** de l'existant ; DNS `*.pi.antoine42.com` ; suppression du wildcard mort. Coupure de catacombes ≈ 1 min lors du basculement du CNAME.
2. Apps Docker existantes (fan, power, ApplyBoard, site naturo, projet-web) : retirer les ports publiés, rejoindre `traefik-net` ⇒ `private`. Mosquitto lié à l'IP Tailscale. Supprimer les zombies (alpage sur le Pi, camofox si inutile).
3. Conteneuriser les services systemd : catacombes (→ `public`), sous-la-barre, notes, questionnaire (tunnel persistant au lieu de trycloudflare). IBKR / Unused / piproxy : conteneurisation étudiée au cas par cas, sinon documentés comme exceptions `expose: none`.
4. VPS : reprendre le Traefik du repo (email ACME réel, config identique), previews via Tailscale, nettoyage (pr-102/103, réseau `web`, ia-bastion).
5. `tools/` : CLI puis web UI + LLM.

## 10. Décisions à prendre (je recommande en gras)

1. Reverse proxy sur le Pi : **Traefik** (identique au VPS, labels, certs auto) vs Caddy vs garder `tailscale serve`.
2. Privé : **`*.pi.antoine42.com` → IP Tailscale + certs DNS-01** vs `*.ts.net` (Tailscale serve, un seul hostname, pas de sous-domaines) vs ports.
3. Public : **Cloudflare Tunnel unique → Traefik, CNAME par app** vs Tailscale Funnel (limité : 3 ports, hostname `.ts.net` seulement, pas ton domaine).
4. Tunnel : **recréer en mode config locale (ingress versionné)** vs garder le remote-managed (config dans le dashboard, hors git).
5. Secrets : **sops + age dans le repo** vs `.env` sur l'hôte seulement.
6. Nom du repo : `infra` ? `homelab` ? (tu le crées, je pousse).
7. Previews sur le Pi aussi, ou seulement sur le VPS ? (**les deux, même mécanique**).

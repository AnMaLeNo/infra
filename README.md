# infra

Outil générique pour déployer des projets GitHub sur **un** serveur (Raspberry Pi, VPS…) de façon déclarative, reproductible et pilotée par un LLM.

> **Tu es un LLM (Claude Code) lancé par le runner ?** Lis dans l'ordre : ce README, `docs/conventions.md`, puis uniquement les `SKILL.md` que le runner t'a indiqués. Ne devine jamais : si une information manque, lis `host.yaml` / `registry.yaml` ou lance `infra status`.

## Idée

- Un **repo de config privé** (ex. `infra-raspberry-pi`) décrit *l'état voulu* du serveur : `host.yaml` (le serveur), `registry.yaml` (les projets : repo GitHub + comment les exposer), `secrets/` (chiffrés sops+age), `skills/` (skills custom). Il embarque cet outil comme sous-module `tool/` épinglé sur une version.
- L'outil fournit : la **stack d'infra** (Traefik, Cloudflare Tunnel, UI), le **CLI `infra`** (actions mécaniques et idempotentes), des **skills** (modes d'emploi pour le LLM) et le **runner** qui lance `claude -p` avec les bons skills à chaque changement de config.
- Chaque serveur a son propre clone. L'outil ne gère **que** le serveur où il tourne.

```
registry.yaml modifié (UI web ou git push)
        │
        ▼
 runner : détecte le diff par projet
        │   → choisit les skills : skills/service/<service>, skills/expose/<expose>, skills/previews…
        ▼
 claude -p  "Applique le projet X" + SKILL.md
        │   → lit l'état réel (infra status), fait le nécessaire via le CLI (infra apply…),
        │     corrige ce qui n'est pas mécanique (port à deviner, build qui casse…)
        ▼
 Traefik route  https://<domain>.<private_zone>  (Tailscale)   et/ou
                https://<domain>.<public_zone>   (Cloudflare Tunnel → Traefik)
```

## Les trois modes d'exposition

| `expose` | Accessible par | URL | Mécanisme |
|---|---|---|---|
| `none` | personne (daemon, collecteur, broker) | — | tourne dans Docker, aucune route HTTP |
| `private` | les appareils du tailnet | `https://<domain>.<private_zone>` | DNS wildcard → IP Tailscale de l'hôte ; Traefik écoute sur cette IP ; certificat Let's Encrypt wildcard (DNS-01 Cloudflare) |
| `public` | internet | `https://<domain>.<public_zone>` | CNAME → Cloudflare Tunnel → Traefik. Aucun port ouvert sur la box |

`previews: private|public` ajoute `https://pr-<N>.<domain>.<zone>` par pull request.

Un mode = un skill (`skills/expose/<mode>/SKILL.md`). Un type de service = un skill (`skills/service/<type>/SKILL.md`). Le repo de config peut en ajouter ou en remplacer dans `skills/` (même chemin relatif).

## `registry.yaml` (repo de config)

```yaml
projects:
  catacombes:
    repo: https://github.com/AnMaLeNo/catacombes
    enabled: true
    service: compose        # skills/service/compose — le repo a un docker-compose.yml
    expose: public          # none | private | public
    domain: catacombes      # défaut : le nom du projet
    container: web          # service compose qui reçoit le trafic HTTP
    port: 8420              # port d'écoute dans le container
    previews: none          # none | private | public
    needs: [mosquitto]      # stacks partagées requises (stack/<nom>)
    env: true               # secrets/projects/catacombes.env (sops)
```

Minimum : `repo` + `expose`. Tout le reste a une valeur par défaut ou est deviné par le LLM.

## Dépôt

```
README.md             ← tu es ici
docs/                 conventions (chemins, réseaux, labels, noms), architecture, décisions
skills/               1 SKILL.md par opération, écrits pour un LLM
  expose/{none,private,public}/   service/{compose,static}/   previews/   undeploy/
  bootstrap-host/   agent-account/   secrets/   migrate-legacy/
stack/                compose des services d'infra : traefik/, cloudflared/, ui/, mosquitto/
infra/                le CLI + API + runner (Python)
example-config/       à quoi ressemble un repo de config
install.sh            installation de l'outil sur un hôte déjà bootstrappé
```

## Installer sur un serveur

```bash
sudo git clone --recursive git@github.com:<toi>/infra-<serveur>.git /srv/infra
sudo /srv/infra/tool/install.sh      # dépendances, utilisateur ai-infra, CLI, runner
infra bootstrap                      # Traefik, tunnel, UI… (skill bootstrap-host)
```

Voir `skills/bootstrap-host/SKILL.md` pour un hôte vierge.

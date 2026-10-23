# infra

Outil générique pour déployer des projets GitHub sur **un** serveur (Raspberry Pi, VPS…) de façon déclarative, reproductible et pilotée par un LLM.

> **Tu es un LLM (Claude Code) lancé par le runner ?** Lis dans l'ordre : ce README, `docs/conventions.md`, puis uniquement les `SKILL.md` que le runner t'a indiqués. Ne devine jamais : si une information manque, lis `host.yaml` / `registry.yaml` ou lance `infra status`.

## Idée

- Un **repo de config privé** (ex. `infra-raspberry-pi`) décrit *l'état voulu* du serveur : `host.yaml` (le serveur), `registry.yaml` (les projets : repo GitHub + comment les exposer), `secrets/` (chiffrés sops+age), `skills/` (skills custom). Il embarque cet outil comme sous-module `tool/` épinglé sur une version.
- **C'est un LLM qui déploie, quand on le lui demande.** Modifier `registry.yaml` (front ou git) ne déclenche rien ; le bouton **Lancer Claude** d'un projet (ou `infra enqueue <P>`) met une demande dans la file, et le runner lance `claude -p` avec l'objectif (l'entrée du projet), les skills correspondants (but + état cible + conventions, pas des procédures) et un serveur **MCP** de petits tools atomiques. Claude choisit le chemin, lit le repo du projet, écrit ce qui manque, lance `docker compose`, vérifie. Il n'y a volontairement **pas** de commande « déploie tout » déterministe : un projet sans Dockerfile, un cas tordu, une erreur de build sont des situations normales que l'agent gère.
- Les tools MCP (`infra mcp`) sont des raccourcis fiables pour les gestes répétitifs qu'on ne veut pas voir réinventés : synchroniser un repo au bon endroit, générer l'override compose (ports neutralisés, labels Traefik, réseau), déchiffrer un `.env`, créer un enregistrement DNS, lire l'état. Chacun fait **une** chose et échoue clairement quand il ne s'applique pas ; l'agent fait alors autrement, à la main. On en ajoute au fil de l'usage, à partir de ce que les runs ont dû faire manuellement.
- Chaque serveur a son propre clone. L'outil ne gère **que** le serveur où il tourne.

```
registry.yaml modifié (UI web ou git)  →  rien ne se passe
        │
        ▼  bouton « Lancer Claude » (ou `infra enqueue <P>`)
 runner (infra watch) : prend la demande, un projet à la fois, choisit les skills
        │   skills/service/<service>, skills/expose/<expose>, skills/previews, skills/migrate-legacy…
        ▼
 claude -p  « amène le projet X à l'état voulu »  + SKILL.md  + MCP `infra`
        │   → constate l'état réel, fait le nécessaire (tools MCP, docker compose, git, PR si besoin), vérifie
        ▼
 Traefik route  https://<domain>.<private_zone>  (Tailscale)   et/ou
                https://<domain>.<public_zone>   (Cloudflare Tunnel → Traefik)
```

## Reprendre la session d'un run

Dans **Historique**, un run a un bouton **Continuer cette session dans VS Code** : il ouvre `https://editor.<private_zone>` (VS Code de Microsoft en version web, `code serve-web`, sous `ai-infra`, service `infra-editor`, **privé** : tailnet uniquement) directement sur la session Claude de ce run. Les runs nomment leur session (`<projet> · jj/mm hh:mm`) et enregistrent son id ; le bouton le dépose dans `runs/.editor-open`, que lit une petite extension (`infra/editor_ext/`) installée avec l'extension Claude Code, qui utilise le token Claude de l'hôte et est réglée pour lister les sessions des runs. Première visite avec `?tkn=<token>` (token dans `~ai-infra/.config/infra-editor/token`), un cookie suffit ensuite. L'éditeur est un shell complet sous `ai-infra`.

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
    service: compose        # compose | static ; absent → Claude choisit et l'écrit
    expose: public          # none | private | public
    domain: catacombes      # défaut : le nom du projet
    container: web          # écrit par Claude au run (service compose qui reçoit le HTTP), pas à saisir
    port: 8420              # idem (port d'écoute dans le container) ; de même compose_file, build, static_dir
    previews: none          # none | private | public
    needs: [mosquitto]      # stacks partagées requises (stack/<nom>)
    env: true               # secrets/projects/catacombes.env (sops)
```

Minimum : `repo` + `expose`. L'utilisateur décrit le *quoi* (repo, exposition, domaine, previews, dépendances, secrets) ; le *comment* (`service` s'il est absent, `container`, `port`, `compose_file`, `build`, `static_dir`) est constaté par Claude pendant le run et écrit dans le registre via `registry_set` — le front l'affiche en lecture seule, et ces champs ne comptent pas comme « modifié depuis le dernier run ».

## Dépôt

```
README.md             ← tu es ici
docs/                 conventions (chemins, réseaux, labels, noms), architecture, décisions
skills/               1 SKILL.md par opération, écrits pour un LLM
  expose/{none,private,public}/   service/{compose,static}/   previews/   undeploy/
  bootstrap-host/   agent-account/   secrets/   migrate-legacy/
stack/                compose des services d'infra : traefik/, cloudflared/, ui/, mosquitto/
infra/                CLI, serveur MCP (`infra mcp`), API/UI, runner (Python)
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

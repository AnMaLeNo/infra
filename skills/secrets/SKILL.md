---
name: secrets
description: Variables d'environnement et tokens chiffrés sops+age dans le repo de config ; jamais en clair nulle part ailleurs.
---
## But
Tout secret vit chiffré dans `<config>/secrets/` (déchiffrable par la clé age de l'hôte et celle de l'humain) et n'apparaît en clair que dans les fichiers `.env` 0600 que l'outil écrit sur l'hôte.

## Où
| Fichier chiffré | Contenu |
|---|---|
| `secrets/infra.env` | `CF_API_TOKEN`, `CF_ZONE_ID`, `CF_ACCOUNT_ID`, `ACME_EMAIL`, `TS_AUTHKEY` (opt.), `CLAUDE_CODE_OAUTH_TOKEN`, `GITHUB_TOKEN` (opt. : repos privés + PR ; token fine-grained Contents + Pull requests read/write, injecté par l'outil dans `repo_sync` et dans `git`/`gh` du run, jamais écrit sur disque) |
| `secrets/cloudflared.json` | credentials du tunnel (`infra tunnel create`) |
| `secrets/projects/<P>.env` | `.env` du projet → `/srv/apps/<P>/.env` |
| `secrets/projects/<P>.preview.env` | surcharges previews |

## Outils
`infra secrets edit <rel>` (sops), `infra secrets set <rel> CLE=valeur` (ou `CLE` seule, valeur sur stdin), `infra secrets keys <rel>` / tool MCP `secrets_keys` (noms seulement), `env_decrypt` (écrit le `.env` d'un projet), `infra secrets rekey` après ajout d'un destinataire dans `.sops.yaml`.

## Ne pas faire
- Écrire une valeur de secret dans `registry.yaml`, `host.yaml`, un override, un log, un commit, ou ta sortie.
- `cat` un `.env` déchiffré. Si tu dois vérifier qu'une variable existe, utilise `secrets_keys`.
- Inventer une valeur manquante : termine en échec en listant les noms attendus.

---
name: secrets
description: Ajouter, modifier ou lire un secret (variables d'environnement d'un projet, tokens d'infra) chiffré avec sops+age dans le repo de config.
---
# Secrets

## Où
| Fichier (chiffré, dans `<config>/secrets/`) | Contenu |
|---|---|
| `infra.env` | `CF_API_TOKEN` (Zone.DNS Edit + Cloudflare Tunnel Edit), `CF_ZONE_ID`, `CF_ACCOUNT_ID`, `ACME_EMAIL`, `TS_AUTHKEY` (optionnel), `CLAUDE_CODE_OAUTH_TOKEN` (runner) |
| `cloudflared.json` | credentials du tunnel (créé par `infra tunnel create`) |
| `projects/<P>.env` | `.env` du projet, déchiffré vers `/srv/apps/<P>/.env` |
| `projects/<P>.preview.env` | surcharges pour les previews |

Règle sops (`<config>/.sops.yaml`) : tout `secrets/**` est chiffré pour la clé age de l'hôte (+ celle de l'humain). Clé privée : `/srv/infra/.age-key`, jamais dans git.

## Étapes
- Éditer : `infra secrets edit projects/<P>.env` (ouvre `sops`, chiffre à la sauvegarde). Format `.env` : `CLE=valeur`, une par ligne, pas de guillemets inutiles.
- Ajouter une variable sans éditeur : `infra secrets set projects/<P>.env CLE=valeur` (la valeur est lue depuis stdin si absente : `infra secrets set projects/<P>.env CLE` puis coller).
- Lire les **noms** de variables : `infra secrets keys projects/<P>.env`. Les valeurs ne s'affichent jamais dans un run LLM.
- Appliquer : `infra render <P>` redéchiffre ; `infra up <P>` redémarre si le `.env` a changé.
- Ajouter un destinataire (nouvel hôte, nouvel humain) : ajoute la clé publique dans `.sops.yaml` puis `infra secrets rekey`.

## Ne pas faire
- Ne jamais écrire un secret en clair dans `registry.yaml`, `host.yaml`, un override, un log ou un commit.
- Ne jamais `cat` un fichier déchiffré dans la sortie d'un run.

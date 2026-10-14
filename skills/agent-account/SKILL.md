---
name: agent-account
description: Un compte Unix par agent IA / automatisation, jamais partagé, à droits limités et journalisés.
---
## But
Chaque outil qui se connecte à l'hôte (Devin, Claude distant, GitHub Actions…) a son propre compte `ai-<outil>`, dont la clé publique est versionnée dans `<config>/agents/`, avec un sudo limité et enregistré (sudoreplay). Révoquer = supprimer le fichier.

## État cible
- `<config>/agents/ai-<outil>.pub` existe (une ligne, commentaire = outil + date), commité.
- `sudo infra agents sync` passé : compte créé, groupes `docker` + `infra`, `authorized_keys` régénéré depuis le dossier, `/etc/sudoers.d/ai-<outil>` = `NOPASSWD` sur `systemctl, tailscale, apt-get, infra` seulement, avec `log_input, log_output`.
- `ai-github` : pas de sudo, ForceCommand `infra preview up|down …` uniquement.
- `ssh ai-<outil>@<tailscale_ip> infra --version` fonctionne.
- Anciens comptes partagés (`agent`, `alpage`…) retirés avec `sudo infra agents retire <nom>` **après** vérification du nouvel accès ; mentionnés dans `host.yaml: notes`.

## Conventions
Le groupe `docker` équivaut à root : c'est assumé et compensé par la journalisation ; ne jamais donner `ALL` dans sudoers. Pas de mot de passe. Home 0700.

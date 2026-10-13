---
name: agent-account
description: Créer (ou révoquer) un compte Unix dédié à un agent IA ou à une automatisation. Un compte par agent, jamais partagé.
---
# Compte agent IA

## Quand
Un nouvel outil doit se connecter à l'hôte (Devin, Claude Code distant, GitHub Actions…).

## Règles
- Nom : `ai-<outil>` (`ai-devin`, `ai-github`, `ai-claude`). Jamais de compte partagé entre deux outils, jamais le compte humain.
- Clé publique versionnée : `<config>/agents/<nom>.pub` — c'est la source de vérité. `authorized_keys` est régénéré depuis ce dossier par `infra agents sync`. Révoquer = supprimer le fichier + `infra agents sync`.
- Droits : groupe `docker` (⚠ équivaut à root sur l'hôte — c'est assumé, documenté, et compensé par la journalisation) + groupe `infra` (écriture `/srv/apps`, `/srv/data`, `/srv/infra`).
- `sudo` **limité** par `/etc/sudoers.d/ai-<nom>` : `NOPASSWD` sur `/usr/bin/systemctl`, `/usr/bin/tailscale`, `/usr/bin/apt-get`, `/usr/local/bin/infra` uniquement, avec `log_input, log_output` (sudoreplay). Pas de `ALL`.
- Profil `ai-github` : shell restreint à `infra preview *` (ForceCommand), pas de sudo.
- Pas de mot de passe. Home `/home/ai-<nom>` 0700.

## Étapes
1. Ajoute la clé : `<config>/agents/ai-<nom>.pub` (une ligne, commentaire = outil + date). Commit.
2. `sudo infra agents sync` : crée le compte s'il manque, groupes, sudoers, `authorized_keys`. Vérifie : `id ai-<nom>`, `sudo -l -U ai-<nom>`, `ssh ai-<nom>@<tailscale_ip> infra --version`.
3. Pour révoquer : supprime le `.pub`, `sudo infra agents sync --prune` (verrouille le compte, retire la clé, garde le home pour audit).

## Migration des anciens comptes partagés
Les comptes `agent` et `alpage` existants sont remplacés : créer `ai-devin`, vérifier l'accès, puis `sudo infra agents retire agent alpage` (verrouille, ne supprime pas). Documenter dans `host.yaml: notes`.

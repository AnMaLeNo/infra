---
name: agent-account
description: Un compte Unix par agent IA / automatisation / machine distante, jamais partagé, clé SSH seulement, sudo journalisé.
---
## But
Chaque outil ou machine qui se connecte à l'hôte (Devin, Claude distant, GitHub Actions, un PC…) a son propre compte, décrit dans `<config>/agents/` : la clé publique (`<nom>.pub`) et, au besoin, le niveau de sudo (`<nom>.yaml`). Le repo de config suffit à recréer tous les comptes sur un hôte réinstallé. Révoquer = supprimer le `.pub` puis `sudo infra agents sync --prune`.

## État cible
- `<config>/agents/<nom>.pub` existe (une seule ligne, commentaire = outil/machine), commité. `<nom>` : `[a-z_][a-z0-9_-]*`, ni `root` ni `ai-infra`.
- `<config>/agents/<nom>.yaml` (facultatif) : `sudo: none | limited | full` (défaut `limited`).
  - `limited` : `NOPASSWD` sur `systemctl, tailscale, apt-get, infra` ;
  - `full` : `NOPASSWD: ALL` ;
  - `none` : pas de sudo.
  Dans tous les cas `log_input, log_output` (sudoreplay).
- `sudo infra agents sync` passé : compte créé sans mot de passe (connexion par clé uniquement), groupes `docker` + `infra` + `infra-agents`, home 0700, `authorized_keys` régénéré depuis le dossier, `/etc/sudoers.d/<nom>` conforme au niveau (validé par visudo).
- `ai-github` : pas de sudo, ForceCommand `infra preview up|down …` uniquement.
- `ssh <nom>@<tailscale_ip> infra --version` fonctionne.
- Comptes partagés ou créés à la main (`agent`, `alpage`…) retirés avec `sudo infra agents retire <nom>` **après** vérification du nouvel accès ; mentionnés dans `host.yaml: notes` tant qu'ils existent.

## Conventions
- Le groupe `infra-agents` est le repère « compte géré par infra » (les anciens `ai-*` sont aussi reconnus) : `sync` ne modifie et `--prune` ne verrouille que ces comptes. Un compte existant hors de ce groupe (humain, compte fait à la main) est ignoré et signalé : le supprimer puis relancer `sync`, ou l'ajouter au groupe s'il doit être géré.
- Le groupe `docker` équivaut à root : c'est assumé et compensé par la journalisation. `sudo: full` est un choix humain explicite, écrit dans le repo.
- Pas de mot de passe, jamais.

## Ne pas faire
- Écrire à la main un fichier dans `/etc/sudoers.d/` ou un `authorized_keys` pour un compte géré : il serait écrasé, ou perdu au prochain hôte. Passer par `agents/`.

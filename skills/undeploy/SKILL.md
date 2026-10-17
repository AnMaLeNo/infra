---
name: undeploy
description: Projet désactivé (enabled false) ou retiré du registre : l'arrêter proprement en gardant ses données.
---
## But
Plus rien ne tourne ni ne route pour ce projet, mais ses données et son clone sont conservés : un humain peut le réactiver ou décider de purger.

## État cible
- Toutes ses previews détruites (`infra preview prune <P> --all`).
- `docker compose -p <P> … down --remove-orphans` (sans `-v`) ; `docker ps -a --filter label=com.docker.compose.project=<P>` vide.
- Aucun enregistrement DNS du projet (`dns_prune`).
- `/srv/data/<P>/` et `/srv/apps/<P>/` toujours présents ; ton résumé rappelle que `infra purge <P>` existe et que c'est une décision humaine.

## Ne pas faire
- `down -v`, `rm -rf /srv/data/<P>`, `infra purge` : jamais de ta propre initiative.

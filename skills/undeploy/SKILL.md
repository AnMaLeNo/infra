---
name: undeploy
description: Arrêter un projet (enabled false ou retiré du registre) proprement, en gardant ses données par défaut.
---
# Undeploy

## Quand
`enabled: false`, ou projet supprimé de `registry.yaml`.

## Étapes
1. `infra preview prune <P> --all` : détruit toutes les previews.
2. `infra down <P>` : `docker compose -p <P> down --remove-orphans` (sans `-v`). Vérifie : `docker ps -a --filter label=com.docker.compose.project=<P>` vide.
3. `infra dns prune <P>` : supprime CNAME/A/wildcards du projet. Vérifie `infra dns list`.
4. Les données (`/srv/data/<P>/`) et le clone (`/srv/apps/<P>/`) **restent**. Seul un humain décide `infra purge <P>` (supprime données + clone + secrets déchiffrés). Ne le lance jamais de ta propre initiative ; signale-le dans le run.

## Vérification finale
`infra status <P>` → `not running`, `routes: none`, `data: kept (/srv/data/<P>)`.

## Idempotence
Projet déjà arrêté ⇒ chaque étape ne fait rien.

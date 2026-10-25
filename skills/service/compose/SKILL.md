---
name: service/compose
description: Le projet fournit son propre docker-compose.yml ; le faire tourner sur cet hôte selon les conventions.
---
## But
Le projet tourne en Docker Compose sur cet hôte, à partir de son propre `docker-compose.yml`, sans rien modifier dans son repo, avec un override généré qui l'adapte aux conventions de l'hôte.

## État cible
- Le code est dans `/srv/apps/<P>/repo` sur la bonne `ref` (tool `repo_sync`).
- `registry.yaml` contient `container` (service compose qui reçoit le HTTP) et `port` (port d'écoute interne) exacts — c'est **toi** qui les détermines (compose, Dockerfile `EXPOSE`, code), l'utilisateur ne les saisit jamais ; fixés via `registry_set` (de même `compose_file` si ce n'est pas `docker-compose.yml`).
- `/srv/apps/<P>/compose.override.yml` existe, généré par le tool `compose_override` : ports publiés neutralisés, `restart: unless-stopped`, labels Traefik et réseau `traefik-net` sur `container`, `.env` déchiffré si `env: true`, bind mounts `/srv/data/<P>/…` pour les données déclarées dans `data:`.
- `docker compose -p <P> --project-directory repo -f repo/docker-compose.yml -f compose.override.yml [--env-file .env] up -d --build` est passé ; tous les containers `running`/`healthy` (tool `project_status`).
- Aucun port publié sur l'hôte (`project_status.published_ports` vide), sauf ceux de `expose_ports:` liés à l'IP Tailscale.
- Si `uses:` est renseigné (services partagés de `shared.yaml`, ex. `[mqtt]`) : le projet **ne lance pas** sa propre copie du service (si son compose en contient une — souvent pour le dev — elle n'est pas démarrée : `profiles` dans l'override, ou pas dans la liste des services lancés) ; tu lis le code pour trouver quel(s) service(s) compose s'y connectent et par quelles variables d'env, tu l'écris dans `consumers` via `registry_set` (ex. `{"consumers": {"api": {"MQTT_HOST": "mqtt", "MQTT_PORT": "1883"}}}`), et `compose_override` met ces services sur `traefik-net` avec ces variables : le service partagé y répond sous son nom. Si un `uses` n'est pas déclaré dans `shared.yaml`, termine `bloqué` (needs_human : « déclarer <S> dans Services partagés ») — ne le crée pas.

## Outils utiles
`repo_sync`, `registry_get`/`registry_set`, `compose_override` (lis le fichier généré avant de l'utiliser), `env_decrypt`, `project_status`, et Bash pour `docker compose …`, `docker logs`, lire le compose.

## Conventions à respecter
`docs/conventions.md` § Docker. En particulier : seul `docker-compose.yml` du repo est utilisé (jamais un `docker-compose.dev.yml`/override de dev du repo) ; données persistantes en bind mount sous `/srv/data/<P>/` ; pas de `latest` ajouté par toi.

## Pièges connus
- Volumes nommés contenant des données (postgres…) : déclare-les dans `data: ["<svc>:<chemin>"]` pour qu'ils deviennent des bind mounts ; sinon un `down -v` les perd.
- Variables `${VAR}` sans défaut dans le compose : elles doivent être dans `secrets/projects/<P>.env` (`env: true`) ; si elles manquent, termine en échec en listant les noms (jamais inventer de valeur).
- Image amd64 only sur un Pi (arm64) : cherche une variante multi-arch ou un `platform:` ; sinon échec explicite.
- `network_mode: host`, `privileged`, `devices:` : acceptables avec une raison écrite dans `notes:`.

## Ne pas faire
- Éditer `compose.override.yml` à la main (il sera régénéré) : si le tool ne produit pas ce qu'il faut, ajuste le registre (`data`, `expose_ports`, `container`, `port`, `consumers`) et relance-le ; si c'est un vrai manque du tool, écris l'override à la main **et** signale-le dans ton résumé.
- Modifier le repo du projet pour le déploiement (sauf : il n'a aucun compose → voir `migrate-legacy`).

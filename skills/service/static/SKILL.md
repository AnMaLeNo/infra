---
name: service/static
description: Site statique (fichiers prêts ou build npm) sans compose dans le repo ; servi par un nginx fourni par l'outil.
---
## But
Le site est servi par un container nginx, à partir des fichiers du repo (éventuellement produits par `build:`), sans que le repo n'ait à contenir de Dockerfile ni de compose.

## État cible
- Code dans `/srv/apps/<P>/repo` (`repo_sync`).
- Si `build:` est défini (ex. `npm ci && npm run build`), il a été exécuté (dans un container `node:22`, pas sur l'hôte) et `static_dir` contient un `index.html`.
- `/srv/apps/<P>/compose.static.yml` (nginx `1.27-alpine`, bind mount `static_dir` en lecture seule, port 80) et `compose.override.yml` générés par `compose_override` ; `container=web`, `port=80`.
- `docker compose -p <P> -f compose.static.yml -f compose.override.yml up -d` passé ; `curl` sur l'IP du container → 200.

## Outils utiles
`repo_sync`, `compose_override` (fait le build si `build:`), `project_status`.

## Pièges connus
- Build qui attend des variables (`VITE_*`) : `env: true` + `secrets/projects/<P>.env`.
- Routing côté client (SPA) : nginx par défaut renvoie 404 sur les routes profondes ; si le site en a besoin, ajoute une config nginx dans `/srv/apps/<P>/nginx.conf` et un bind mount dans l'override — signale-le.

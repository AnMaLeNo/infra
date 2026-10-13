---
name: service/static
description: Déployer un site statique (HTML/JS, ou build Vite/Astro…) sans compose dans le repo : l'outil fournit un nginx.
---
# Service `static`

## Quand
`service: static`. Le repo n'a pas de compose ; il contient soit des fichiers statiques prêts (`static_dir`, défaut `.`), soit un build à faire (`build: "npm ci && npm run build"`, `static_dir: dist`).

## Étapes
1. `infra fetch <P>`.
2. Si `build:` est défini, il est exécuté dans un container `node:22` par `infra render` — vérifie que `static_dir` existe après : `ls /srv/apps/<P>/repo/<static_dir>/index.html`.
3. `infra render <P>` génère un compose complet (`nginx:1.27-alpine`, bind mount du dossier en lecture seule, port 80) + labels. `container=web`, `port=80` sont imposés.
4. `infra up <P>`, puis le skill `expose/<mode>`.

## Vérification finale
`curl -sI http://$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' <P>-web-1)/` → 200.

## Idempotence
Pas de rebuild si le commit n'a pas changé.

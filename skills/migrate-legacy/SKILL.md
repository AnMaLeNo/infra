---
name: migrate-legacy
description: Faire passer un service déjà déployé « à l'ancienne » (systemd, process à la main, compose avec ports publiés, quick tunnel) dans le modèle de l'outil, sans coupure inutile.
---
# Migrer un service legacy

## Quand
Un service tourne sur l'hôte en dehors de `/srv/apps` (inventaire : `infra doctor --legacy` liste les sockets `0.0.0.0`, les units systemd non standard, les process orphelins, les compose hors `/srv/apps`).

## Étapes
1. **Décrire l'existant** dans le run : comment il est lancé (unit, compose, cwd), port, données (chemins), exposition actuelle (port publié, tunnel, serve).
2. **Ajouter l'entrée** dans `registry.yaml` avec `service`, `expose`, `domain`, `port`, et `legacy: <description courte>`. Si le repo n'a pas de compose et n'est pas statique : écris-en un **dans le repo du projet** (PR), c'est le seul cas où on touche au repo projet pour le déploiement. Dockerfile minimal, `restart: unless-stopped`, données dans un volume monté.
3. **Déployer en parallèle** sur un autre `domain` temporaire si le nom est déjà pris (ex. `catacombes-new`) : `infra apply <P>`. Teste.
4. **Données** : copie `rsync -a` des données de l'ancien emplacement vers `/srv/data/<P>/`, pendant une courte coupure si nécessaire ; adapte les montages.
5. **Basculer** : met `domain` définitif, `infra apply <P>` (le CNAME est mis à jour avec `--force` si l'ancien pointait ailleurs), puis arrête l'ancien : `sudo systemctl disable --now <unit>` / `docker compose -f <ancien> down` / `kill` du process orphelin. **Supprime** l'unit (`/etc/systemd/system/<unit>`) et `daemon-reload`.
6. Retire `legacy:` du registre, ajoute une ligne dans `<config>/docs/migrations.md` (date, quoi → quoi).

## Vérification finale
`infra doctor --legacy` ne liste plus le service ; l'URL répond ; l'ancien port n'écoute plus (`ss -ltnp`).

## Ne pas faire
- Ne pas supprimer les anciennes données avant d'avoir vérifié la nouvelle instance pendant au moins un redémarrage.
- Ne pas migrer deux services en même temps.

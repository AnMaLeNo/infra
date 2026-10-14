---
name: migrate-legacy
description: Un service déjà déployé « à l'ancienne » (systemd, process à la main, compose avec ports publiés, quick tunnel) doit passer dans le modèle, sans coupure inutile ni perte de données.
---
## But
Le projet tourne selon le modèle (`service/*`, `expose/*`), l'ancienne instance est arrêtée et nettoyée, les données ont suivi, et tout est journalisé dans `<config>/docs/migrations.md`.

## État cible
- Tu as décrit l'existant dans ton résumé : comment il est lancé (unit systemd, compose, cwd, user), port, où sont les données, comment il est exposé (port publié, tunnel, `tailscale serve`).
- Si le repo n'a ni compose ni Dockerfile : un `Dockerfile` + `docker-compose.yml` minimal est proposé **en PR sur le repo du projet** (seul cas où on touche au repo projet pour du déploiement) — `restart: unless-stopped`, données dans un volume, pas de port publié. Tant que la PR n'est pas mergée, déploie depuis sa branche (`ref:`).
- La nouvelle instance tourne et répond (sur un `domain` temporaire si l'ancien nom est encore pris).
- Données copiées (`rsync -a`) de l'ancien emplacement vers `/srv/data/<P>/…` pendant la bascule ; bind mounts déclarés dans `data:`.
- Bascule : `domain` définitif, DNS (`dns_ensure(force=True)` si l'ancien CNAME pointait sur l'ancien tunnel), puis ancien service arrêté **et** désactivé (`systemctl disable --now`, unit supprimée + `daemon-reload` ; ou `docker compose down` de l'ancien dossier ; ou `kill` du process orphelin). L'ancien port n'écoute plus (`ss -ltnp`).
- `legacy:` retiré du registre ; une ligne dans `docs/migrations.md` (date, service, avant → après).

## Pièges connus
- Ne migre qu'un service à la fois ; ne supprime les anciennes données qu'après au moins un redémarrage réussi de la nouvelle instance (dis-le dans le résumé, laisse l'humain supprimer).
- Un service exposé par l'ancien tunnel remote-managed : le CNAME doit basculer vers le nouveau tunnel (`dns_ensure(force=True)`), l'ancien `cloudflared` ne doit être arrêté qu'une fois que **tous** ses hostnames ont basculé.
- Les exceptions listées dans `host.yaml: exceptions` (IBKR, unused, piproxy…) ne se migrent pas sans décision humaine.

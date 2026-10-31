---
name: shared-service
description: Un service consommé par plusieurs projets (broker MQTT, base de données…) tourne en un seul exemplaire sur l'hôte, déclaré dans shared.yaml, joignable par son nom sur traefik-net et, si demandé, sur des IPs explicites de l'hôte.
---
## But
Le service partagé `<S>` déclaré dans `shared.yaml` existe en **un seul exemplaire** sur l'hôte, conforme aux choix humains (type, IPs d'écoute, port, auth, données), et chaque projet qui le déclare dans `uses:` peut le joindre par le nom `<S>`. Il appartient à l'hôte, pas à un projet : aucun projet ne doit en lancer sa propre copie.

## État cible
- `<config>/shared/<S>/docker-compose.yml` (+ fichiers de conf) existe, écrit par toi dans le repo de config (c'est ce qui permet de reconstruire l'hôte) ; commite et pousse si tu en as le droit, sinon dis-le dans ton résumé (l'humain ou l'outil le fera), ce n'est pas bloquant. Image à tag explicite (jamais `latest`), `restart: unless-stopped`, healthcheck.
- `docker compose -p shared-<S> --project-directory <config>/shared/<S> up -d` est passé ; conteneur `running`/`healthy` (tool `shared_status`).
- Le conteneur est sur `traefik-net` avec l'alias réseau `<S>` (`shared_status.alias_ok`) : les projets le joignent en `<S>:<port interne>`.
- Si `port` est déclaré : publié **uniquement** sur les IPs de `listen` (`lan` → chaque IP de `host.yaml: lan_ip`, une ou plusieurs car un hôte peut avoir eth0 et wlan0 ; `tailnet` → `tailscale_ip`), une ligne `ports:` par IP (`"<ip>:<port>:<port>"`). `shared_status.listening` == `listen_wanted`, `on_all_interfaces` faux. Si `port` est absent : aucun `ports:`.
- `auth: anonymous` → accès sans identifiants (choix humain, ne le « corrige » pas) ; `auth: users` → identifiants dans `secrets/shared/<S>.env` (noms via `secrets_keys`), jamais inventés.
- `data: true` → données en bind mount sous `/srv/shared/<S>/data` (pas de volume nommé). Si un exemplaire legacy existe (volume nommé, autre compose), ses données sont **migrées** avant la bascule (ex. messages retenus d'un broker, base) et l'ancien est arrêté — seulement si `notes`/`legacy` t'y autorisent, sinon `bloqué`.
- `shared.yaml` contient tes constats via `shared_set` : `image`, `container`, `compose_file` si différent.
- `enabled: false` ou entrée retirée : `docker compose -p shared-<S> down` (sans `-v`), données conservées, fichiers `shared/<S>/` laissés.

## Outils utiles
`shared_get` (dont `users` : qui le consomme), `shared_status`, `host_addresses` (IPs déclarées vs portées par l'hôte), `shared_set`, `secrets_keys`, `observe` (remarque pour l'humain, hors statut), et Bash : `docker compose`, `ss -ltn`, `docker inspect`, `docker logs`, le client du protocole (ex. `mosquitto_sub`) pour vérifier qu'il répond.

## Conventions à respecter
`docs/conventions.md` § Docker et § Services partagés. Nom de projet compose `shared-<S>` ; réseau `traefik-net` externe ; alias = nom du service ; jamais `0.0.0.0` ni `ports: - "1883:1883"` sans IP.

## Pièges connus
- `listen: [lan]` sans `lan_ip` dans `host.yaml`, ou une IP de `lan_ip` que l'hôte ne porte pas (`host_addresses.lan_ip_missing`, `warning`) : termine `bloqué`, ne devine pas (des clients externes ont cette IP en dur). En revanche, laquelle des IP porte la route par défaut ne te regarde pas : l'humain déclare les IP, tu fais écouter le service dessus, c'est tout.
- `notes` est une information laissée par l'humain (clients hors Docker, historique) : pas un état à atteindre ni à vérifier. Si tu y repères quelque chose d'utile (un ESP qui ne répond pas, une IP qui est celle du Wi-Fi, un reste à nettoyer), dépose-le avec `observe` : l'humain le verra sur la carte du service ; ça ne rend pas le run `bloqué`.
- Changer le `port` ou l'IP casse les clients hors Docker (ESP, autres machines) : ce sont des choix humains, tu ne les changes pas.
- Broker MQTT : les messages **retenus** et les last-will vivent dans la persistance ; une bascule sans migration perd la dernière consigne. Vérifie après coup avec `mosquitto_sub --retained-only`.
- Deux exemplaires (legacy + nouveau) sur le même port LAN : le second ne démarre pas (`address already in use`) — arrête l'ancien d'abord, dans la même minute, et vérifie que les clients se reconnectent (`docker logs`).

## Ne pas faire
- Créer ou laisser un second exemplaire du service dans un projet (`uses:` l'interdit) ; si un compose de projet en contient un, c'est au run du projet de le neutraliser, pas à toi de le supprimer du repo.
- Modifier `kind`, `listen`, `port`, `auth`, `data`, `enabled` : demande-les via `run_report(needs_human=…)`.
- Écouter sur toutes les interfaces, même « temporairement ».

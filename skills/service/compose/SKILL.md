---
name: service/compose
description: Déployer un projet qui fournit son propre docker-compose.yml. Type de service par défaut.
---
# Service `compose`

## Quand
`registry.yaml: projects.<P>.service: compose` (défaut). Le repo du projet contient un `docker-compose.yml` (ou `compose.yaml`) qui fonctionne tel quel avec `docker compose up`.

## Préconditions
- `infra status <P>` répond (hôte bootstrappé, Traefik up).
- Les stacks de `needs:` tournent : `infra stack status`.

## Étapes
1. **Récupérer le code** : `infra fetch <P>` → clone/pull `/srv/apps/<P>/repo` sur `ref`. Vérifie : `git -C /srv/apps/<P>/repo log -1 --oneline`.
2. **Comprendre le compose** : lis `/srv/apps/<P>/repo/docker-compose.yml`. Identifie le service HTTP d'entrée et son port d'écoute interne. Si `container`/`port` manquent dans le registre, déduis-les (le service avec `ports:` ou un `Dockerfile` qui `EXPOSE`), puis écris-les dans `registry.yaml` via `infra set <P> container=<svc> port=<n>` — ne laisse pas une déduction implicite.
3. **Repérer les écarts aux conventions** (`docs/conventions.md`) et les traiter dans l'override, pas dans le repo du projet :
   - `ports:` publiés → neutralisés automatiquement par `infra render`. Si un port non HTTP est indispensable, ajoute-le à `expose_ports:` du registre.
   - `network_mode: host`, `privileged`, `devices:` → acceptable seulement avec une raison ; note-la dans `registry.yaml: projects.<P>.notes`.
   - volumes nommés contenant des données → bind mount sous `/srv/data/<P>/` (ajoute `volumes:` dans `infra set <P> data=<svc>:<chemin>`).
   - fichiers d'override de dev (`docker-compose.dev.yml`, `override` dans le repo) → **ne pas** les utiliser ; seul `docker-compose.yml` + l'override généré.
   - `env_file: .env` attendu par le compose → fourni par le déchiffrement de `secrets/projects/<P>.env` (`env: true`). S'il manque des variables, demande-les (échec explicite dans le run), n'invente pas de valeurs.
4. **Générer l'override** : `infra render <P>` → `/srv/apps/<P>/compose.override.yml` (labels Traefik, réseau, ports neutralisés, bind mounts) + `.env` déchiffré. Lis le fichier généré pour vérifier qu'il correspond à ce que tu attends. Ne l'édite jamais à la main.
5. **Lancer** : `infra up <P>` (= `docker compose -p <P> -f repo/docker-compose.yml -f compose.override.yml up -d --build --remove-orphans`). Vérifie : `infra status <P>` → tous les containers `running`/`healthy`.
6. Passe au skill `expose/<mode>` indiqué par le runner.

## Vérification finale
`infra status <P>` : containers up, aucun port publié (`docker compose -p <P> ps --format json` → `Publishers` vide sauf `expose_ports`), service attaché à `traefik-net`.

## Idempotence
`fetch` sans nouveau commit, `render` sans changement de registre, `up` sans changement d'image/config ⇒ aucune action. Rejouer le skill sur un projet déjà conforme ne doit rien redémarrer.

## Erreurs connues
- Build qui échoue sur ARM (image amd64 only) : cherche une variante `arm64` de l'image ou un `platform:`; si impossible, échec explicite avec la raison.
- Le compose attend des variables absentes → `infra render` échoue avec la liste ; complète `secrets/projects/<P>.env` via `infra secrets edit projects/<P>.env`.

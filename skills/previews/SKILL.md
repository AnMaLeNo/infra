---
name: previews
description: Activer des environnements de preview par pull request : https://pr-<N>.<domain>.<zone>, créés à l'ouverture d'une PR, détruits à la fermeture.
---
# Previews par PR

## Quand
`previews: private` ou `previews: public`. Développement d'un projet, lien de preview à un client.

## Comment ça marche
- Chaque PR ouverte ⇒ `docker compose -p <P>-pr-<N>` dans `/srv/apps/<P>/previews/pr-<N>/`, même compose + override généré avec `Host(\`pr-<N>.<domain>.<zone>\`)`, `.env` = celui du projet + `PR_NUMBER`, `PR_DOMAIN`, et les éventuelles surcharges `secrets/projects/<P>.preview.env`.
- Un seul enregistrement DNS wildcard par projet (`*.<domain>.<zone>`) couvre toutes les PR.
- Déclenchement : workflow GitHub Actions **dans le repo du projet** qui appelle le workflow réutilisable de cet outil ; il rejoint le tailnet (action Tailscale, nœud éphémère) puis `ssh ai-github@<tailscale_ip> infra preview up <P> <N>`. Aucun SSH exposé sur internet.
- Données : si le projet a une base, `infra preview up` la seed depuis le dernier dump de `/srv/data/<P>/backups/` si présent ; les données d'une preview existante survivent aux redéploiements ; `preview down` les supprime.
- Verrou par PR (`flock`) : deux déploiements de la même PR ne se chevauchent jamais.

## Étapes
1. `infra dns ensure <P>` crée le wildcard (`CNAME *.<domain>.<public_zone>` → tunnel, ou `A *.<domain>.<private_zone>` → IP Tailscale). Vérifie `infra dns list`.
2. `infra preview install <P>` écrit dans le repo du projet `.github/workflows/preview.yml` (appel du workflow réutilisable) et affiche les secrets GitHub à créer : `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`, `INFRA_HOST`, `INFRA_SSH_KEY`. Ouvre une PR sur le repo du projet avec ce fichier (c'est la seule modification acceptable dans un repo projet).
3. Teste à la main : `infra preview up <P> 0 --ref <branche>` puis `curl -sI https://pr-0.<domain>.<zone>/`, puis `infra preview down <P> 0`.
4. Vérifie que le compte `ai-github` existe (`skills/agent-account`), avec `sudo` limité à `infra preview *`.

## Vérification finale
`infra preview list <P>` vide (ou les PR ouvertes), DNS wildcard présent, workflow en place.

## Idempotence
`preview up` sur une PR déjà déployée au même commit ⇒ rien. Le workflow vérifie l'état de la PR avant de déployer (pas de preview pour une PR fermée).

## Erreurs connues
- Vite/webpack dev server refuse l'hôte : `PR_DOMAIN` doit être lu par l'app (`VITE_ALLOWED_HOSTS`, `VITE_HMR_HOST`…). Ajoute la variable dans `secrets/projects/<P>.preview.env`.
- Trop de previews (RAM) : `infra preview prune <P>` supprime celles dont la PR est fermée.

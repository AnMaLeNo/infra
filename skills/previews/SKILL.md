---
name: previews
description: Environnements de preview par pull request sur https://pr-<N>.<domain>.<zone>, créés/détruits par GitHub Actions.
---
## But
Chaque PR ouverte sur le repo du projet a sa propre instance (`docker compose -p <P>-pr-<N>`) joignable sur `https://pr-<N>.<domain>.<public_zone>` (`previews: public`) ou `https://pr-<N>.<domain>.<private_zone>` (`previews: private`), et disparaît à la fermeture de la PR.

## Comment c'est câblé (mécanique, volontairement déterministe)
Un run LLM par push de PR coûterait trop cher : les previews sont donc gérées par `infra preview up|down <P> <N>`, qui réutilise les mêmes générateurs (`repo_sync` sur `refs/pull/N/head`, `compose_override` avec `pr=N`, `.env` du projet + `secrets/projects/<P>.preview.env` + `PR_NUMBER`/`PR_DOMAIN`), avec un verrou par PR. Le repo du projet appelle le workflow réutilisable `AnMaLeNo/infra/.github/workflows/preview.yml`, qui rejoint le tailnet (action Tailscale, nœud éphémère) puis `ssh ai-github@<tailscale_ip> infra preview …` (compte à ForceCommand, aucun SSH exposé sur internet).

## État cible
- DNS wildcard du projet présent : `CNAME *.<domain>.<public_zone>` → tunnel, ou `A *.<domain>.<private_zone>` → IP Tailscale (`dns_ensure` le crée d'après `previews:`).
- Le compte `ai-github` existe (`skills/agent-account`) et sa clé publique est dans `<config>/agents/ai-github.pub`.
- Le repo du projet a `.github/workflows/preview.yml` (contenu : `infra preview install <P>`) et les secrets GitHub `TS_OAUTH_CLIENT_ID`, `TS_OAUTH_SECRET`, `INFRA_HOST`, `INFRA_SSH_KEY` — ouvre une PR sur le repo du projet pour le workflow, et liste les secrets à créer dans ton résumé (tu ne peux pas les créer).
- Test : `infra preview up <P> <N>` avec le numéro d'une PR réellement ouverte sur le repo (le code vient de `refs/pull/N/head`, il n'y a pas d'option `--ref`), de préférence via le compte restreint : `ssh ai-github@<tailscale_ip> infra preview up <P> <N> --url`, puis `curl -sI https://pr-<N>.<domain>.<zone>/`, puis `infra preview down <P> <N>`.

## Pièges connus
- Dev servers (Vite…) qui refusent l'hôte : l'app doit lire `PR_DOMAIN` (`VITE_ALLOWED_HOSTS`, `VITE_HMR_HOST`) ; mets ces variables dans `secrets/projects/<P>.preview.env`.
- Base de données : `preview up` seed depuis le dernier dump de `/srv/data/<P>/backups/` s'il existe ; sinon la preview démarre vide.

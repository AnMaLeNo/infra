# Skills

Un skill = un `SKILL.md` qui décrit un **but** et l'**état cible** à atteindre, pas une procédure. Le LLM (Claude Code) choisit lui-même le chemin ; il a Bash, les fichiers, et les tools MCP de l'outil (`infra mcp`) pour les gestes mécaniques et répétitifs qu'on ne veut pas voir réinventés (synchroniser un repo au bon endroit, générer l'override compose, calculer les labels Traefik, créer un enregistrement DNS, déchiffrer un .env). Si un tool ne convient pas à un projet (pas de compose, cas spécial), il le dit clairement et le LLM fait autrement, à la main.

## Résolution

Quand le projet `P` change dans `registry.yaml`, le runner donne au LLM :

| Situation | Skills |
|---|---|
| `enabled: false` (ou projet supprimé) | `undeploy/` |
| `enabled: true` | `service/<P.service>/` + `expose/<P.expose>/` (+ `previews/` si `P.previews != none`) ; `migrate-legacy/` en plus si `legacy:` est renseigné |

Pour chaque chemin : `<config>/skills/<chemin>/SKILL.md` (custom) sinon `tool/skills/<chemin>/SKILL.md`. Ajouter un mode `expose: lan` = écrire `<config>/skills/expose/lan/SKILL.md`.

## Format

```
---
name: expose/public
description: Une phrase.
---
## But
## État cible          ← liste de faits vérifiables ; le run est réussi quand tout est vrai
## Outils utiles       ← tools MCP / commandes, sans ordre imposé
## Conventions à respecter
## Pièges connus
## Ne pas faire
```

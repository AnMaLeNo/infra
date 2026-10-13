# Skills

Un skill = un dossier avec un `SKILL.md` : le mode d'emploi d'une opération, écrit pour un LLM (Claude Code).

## Résolution

Quand le projet `P` change dans `registry.yaml`, le runner donne au LLM :

| Situation | Skills |
|---|---|
| `enabled: false` (ou projet supprimé) | `undeploy/` |
| `enabled: true` | `service/<P.service>/` + `expose/<P.expose>/` (+ `previews/` si `P.previews != none`) |

Pour chaque chemin, le runner prend d'abord `<config>/skills/<chemin>/SKILL.md` (skill custom du repo de config), sinon `tool/skills/<chemin>/SKILL.md`. Ajouter un mode `expose: lan` = créer `<config>/skills/expose/lan/SKILL.md`.

Les autres skills (`bootstrap-host`, `agent-account`, `secrets`, `migrate-legacy`) sont invoqués par un humain ou par un LLM qui en a besoin.

## Format

```
---
name: expose/public
description: Une phrase. Quand l'utiliser.
---
# Titre
## Quand
## Préconditions
## Étapes      (numérotées, commandes exactes, vérification après chaque étape)
## Vérification finale
## Idempotence (ce qui ne doit rien faire si c'est déjà fait)
## Erreurs connues
```

Règles d'écriture : commandes exactes, pas de « etc. » ; chaque étape dit comment vérifier qu'elle a réussi ; dire explicitement ce que le LLM **ne doit pas** faire (éditer un fichier généré, publier un port, sudo hors liste).

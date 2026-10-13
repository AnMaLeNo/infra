---
name: expose/none
description: Projet qui doit tourner sans être joignable en HTTP (daemon, collecteur, broker, bot).
---
# Expose `none`

## Quand
`expose: none`. Le projet n'a pas de front HTTP, ou on ne veut pas y accéder.

## Étapes
1. Vérifie que le service a bien été lancé par `service/<type>`.
2. `infra render <P>` ne met **aucun** label Traefik et ne rattache rien à `traefik-net`. Vérifie : `docker inspect <P>-<container>-1 -f '{{json .Config.Labels}}'` ne contient pas `traefik.enable`.
3. Aucun port publié (`infra status <P>`). Si le projet a besoin d'être joint par un autre protocole (MQTT, SSH…), c'est `expose_ports:` lié à l'IP Tailscale — jamais `0.0.0.0`.
4. Si le projet était `private`/`public` avant : `infra dns prune <P>` supprime ses enregistrements DNS. Vérifie : `infra dns list | grep <domain>` vide.

## Vérification finale
`infra status <P>` → running, `routes: none`.

## Idempotence
Rien à faire si aucun label/DNS n'existe.

---
name: expose/none
description: Le projet tourne sans aucune route HTTP (daemon, collecteur, broker, bot).
---
## But
Le projet tourne, mais personne ne peut le joindre en HTTP, ni par Traefik, ni par un port.

## État cible
- Aucun label `traefik.*` sur ses containers, aucun container dans `traefik-net`.
- Aucun port publié (`project_status.published_ports` vide) ; si un protocole non HTTP doit être joignable (MQTT, SSH…), c'est `expose_ports:` dans le registre → port lié à l'IP Tailscale uniquement, jamais `0.0.0.0`.
- Aucun enregistrement DNS pour ce projet (`dns_list` ne contient pas `<domain>.<zone>`) — s'il était `private`/`public` avant, `dns_prune`.

## Outils utiles
`compose_override` (ne met aucun label quand `expose: none`), `project_status`, `dns_prune`, `dns_list`.

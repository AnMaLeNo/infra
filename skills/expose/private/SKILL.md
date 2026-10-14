---
name: expose/private
description: Joignable uniquement depuis le tailnet, en HTTPS valide, sur https://<domain>.<private_zone>.
---
## But
Depuis n'importe quel appareil du tailnet, `https://<domain>.<private_zone>` sert l'application avec un certificat valide ; depuis internet, le nom ne mène nulle part.

## État cible
- Le service tourne (`service/<type>` satisfait).
- Le container `container` est dans `traefik-net` et porte les labels du routeur `<P>-private` (entrypoint `tailnet`, `Host(\`<domain>.<private_zone>\`)`, TLS resolver `cf`) — produits par `traefik_labels` / `compose_override`.
- Aucun routeur sur l'entrypoint `tunnel`/`websecure` (pas de `<P>-public`) ; s'il était `public` avant, le CNAME public est supprimé (`dns_prune(public_only=True)`).
- `curl -sI --resolve <domain>.<private_zone>:443:<tailscale_ip> https://<domain>.<private_zone>/` → 2xx/3xx sans `-k`.
- `project_status.urls.private` affiché dans ton résumé.

## Outils utiles
`host_info` (zones, IP Tailscale), `traefik_labels`, `compose_override`, `project_status`, `dns_list`. Le DNS privé est un wildcard partagé (`*.<private_zone>` → IP Tailscale, posé au bootstrap) : rien à créer par projet.

## Pièges connus
- Certificat `TRAEFIK DEFAULT CERT` : l'ACME DNS-01 a échoué → `docker logs traefik | grep -i acme` (token Cloudflare, droits Zone.DNS Edit).
- 404 Traefik : le container n'est pas dans `traefik-net`, ou le `Host()` ne correspond pas à `domain`.
- L'app fabrique des URLs absolues en `http://` : elle doit lire `X-Forwarded-Proto` ou recevoir sa base URL par variable d'env.

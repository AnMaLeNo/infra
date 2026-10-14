---
name: expose/public
description: Joignable depuis internet sur https://<domain>.<public_zone> via le Cloudflare Tunnel, sans ouvrir de port.
---
## But
`https://<domain>.<public_zone>` répond depuis n'importe où, en passant par Cloudflare → le tunnel de l'hôte → Traefik → le container. Aucun port n'est ouvert sur la box ni publié par Docker. Le projet reste aussi joignable en privé.

## État cible
- Le service tourne ; le container porte les routeurs `<P>-private` (entrypoint `tailnet`) **et** `<P>-public` (entrypoint `tunnel` ; `websecure` si l'hôte est en `public_ingress: direct`).
- Un `CNAME <domain>.<public_zone> → <tunnel_id>.cfargotunnel.com` proxied existe (`dns_ensure`), ou un `A` vers `public_ip` en mode direct. C'est **la seule** chose qui rend le projet public : l'ingress du tunnel est un catch-all vers Traefik, rien à configurer dans Cloudflare.
- `curl -sI https://<domain>.<public_zone>/` → 2xx/3xx avec `server: cloudflare` (propagation : jusqu'à 2 min).
- Si `previews: public`, voir `previews/`.

## Outils utiles
`host_info`, `traefik_labels`, `compose_override`, `dns_ensure`, `dns_list`, `project_status`, `docker logs cloudflared`.

## Pièges connus
- `dns_ensure` renvoie `conflict` : un enregistrement existe déjà avec une autre cible (ancien tunnel, ancienne IP). Ne force (`force=True`) que si tu es sûr que l'ancienne cible est obsolète (ex. migration legacy explicitement demandée) ; sinon demande.
- Cloudflare 530/1033 : cloudflared n'est pas connecté (`stack_status`).
- 404 Traefik derrière le tunnel : `Host()` ≠ nom demandé.

## Ne pas faire
- `cloudflared tunnel --url` (quick tunnel), `tailscale funnel`, modifier `stack/cloudflared/config.yml` pour ajouter un hostname, créer des enregistrements dans le dashboard.

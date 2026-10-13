---
name: expose/public
description: Rendre un projet accessible depuis internet sur https://<domain>.<public_zone> via le Cloudflare Tunnel, sans ouvrir de port.
---
# Expose `public`

## Quand
`expose: public`. Site client, projet à partager, API publique.

## Préconditions
- Hôte en `public_ingress: cloudflare-tunnel` : stack `cloudflared` up (`infra stack status cloudflared`), `host.yaml: cloudflare.tunnel_id` renseigné. Hôte en `direct` (VPS) : Traefik écoute sur 80/443.
- Token API Cloudflare dans `secrets/infra.env` (`CF_DNS_API_TOKEN`, droit Zone.DNS Edit).

## Étapes
1. Le service tourne (`service/<type>`).
2. `infra render <P>` : labels du routeur `<P>-public` (entrypoint `tunnel`, ou `websecure` en direct) **et** du routeur `<P>-private` (un projet public reste aussi joignable en privé). Vérifie l'override.
3. `infra up <P>` si changé.
4. **DNS** : `infra dns ensure <P>` crée `CNAME <domain>.<public_zone> → <tunnel_id>.cfargotunnel.com` (proxied) — ou un `A` vers l'IP publique en mode direct. Vérifie : `infra dns list | grep <domain>`. C'est la seule étape qui rend le projet public : pas de config à faire dans le dashboard Cloudflare, l'ingress du tunnel est un catch-all vers Traefik.
5. Test depuis l'hôte : `curl -sI https://<domain>.<public_zone>/` → 200/30x, header `server: cloudflare`. Propagation : jusqu'à 1–2 min si le CNAME vient d'être créé.
6. Si le projet a `previews: public`, enchaîne avec `previews/`.

## Vérification finale
`infra status <P>` → `routes: public https://<domain>.<public_zone>, private https://<domain>.<private_zone>`.

## Ne pas faire
- Ne pas modifier `stack/cloudflared/config.yml` pour ajouter un hostname : l'ingress est catch-all.
- Ne pas créer d'enregistrement dans le dashboard : tout passe par `infra dns`.
- Ne jamais lancer `cloudflared tunnel --url` (quick tunnel) ni `tailscale funnel`.

## Idempotence
`dns ensure` ne fait rien si le CNAME existe avec la bonne cible.

## Erreurs connues
- `530`/`1033` Cloudflare : cloudflared déconnecté → `docker logs cloudflared --tail 50`.
- `404 page not found` (Traefik) : le CNAME arrive mais aucun routeur ne matche → le `Host()` du label ≠ nom demandé (vérifier `domain`).
- CNAME existant vers une autre cible (ancien tunnel, ancienne IP) : `dns ensure` refuse d'écraser sans `--force` ; vérifie avec l'humain avant.

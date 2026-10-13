---
name: expose/private
description: Rendre un projet accessible uniquement depuis le tailnet, en HTTPS valide, sur https://<domain>.<private_zone>.
---
# Expose `private`

## Quand
`expose: private`. Usage perso : outils, dashboards, apps domotique.

## Préconditions
- `host.yaml: private_zone` défini, enregistrement `A *.<private_zone> → <tailscale_ip>` présent (`infra dns check`), Traefik up avec l'entrypoint `tailnet` (`infra stack status traefik`).

## Étapes
1. Le service tourne (`service/<type>` fait avant).
2. `infra render <P>` a produit les labels du routeur `<P>-private` (entrypoint `tailnet`, `Host(\`<domain>.<private_zone>\`)`, tls resolver `cf`) et rattaché `container` à `traefik-net`. Vérifie dans `/srv/apps/<P>/compose.override.yml`.
3. `infra up <P>` si l'override a changé.
4. Si le projet était `public` avant : `infra dns prune <P> --public` supprime le CNAME ; le routeur `<P>-public` disparaît au `render`. Vérifie `infra dns list`.
5. Test : `curl -sI --resolve <domain>.<private_zone>:443:<tailscale_ip> https://<domain>.<private_zone>/` → 200/30x, certificat valide (pas de `-k`). Si 404 : Traefik n'a pas le routeur → `docker logs traefik --since 2m`.

## Vérification finale
`infra status <P>` → `routes: private https://<domain>.<private_zone>`. Depuis l'extérieur du tailnet, le nom ne résout que vers une IP 100.x injoignable : rien à vérifier, c'est structurel.

## Idempotence
Labels identiques ⇒ `up` ne redémarre rien. DNS : wildcard partagé, rien par projet.

## Erreurs connues
- Certificat `TRAEFIK DEFAULT CERT` : DNS-01 a échoué → `docker logs traefik | grep -i acme` ; vérifier `CF_DNS_API_TOKEN` dans `secrets/infra.env` (droit Zone.DNS Edit).
- L'app génère des liens absolus en `http://` : vérifier qu'elle lit `X-Forwarded-Proto` ; sinon variable d'env de base URL dans `secrets/projects/<P>.env`.

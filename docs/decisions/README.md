# Décisions (ADR courts)

| # | Décision | Pourquoi | Rejeté |
|---|---|---|---|
| 1 | Reverse proxy unique : **Traefik v3** | déjà en prod sur le VPS, découverte Docker par labels, ACME DNS-01 intégré, même config Pi/VPS | Caddy (moins de labels), nginx (config manuelle), `tailscale serve` (un seul hostname `.ts.net`, pas de sous-domaines) |
| 2 | Privé = `*.<private_zone>` → **IP Tailscale** + certs DNS-01 | vrais noms, HTTPS valide sans warning, injoignable d'internet par construction, aucune dépendance à MagicDNS | ports publiés (visibles sur tout le LAN), Tailscale serve |
| 3 | Public = **un Cloudflare Tunnel → Traefik**, un CNAME par app | zéro port ouvert, un seul tunnel à maintenir, « rendre public » = 1 enregistrement DNS idempotent | Tailscale Funnel (3 ports, `.ts.net` seulement), quick tunnels `trycloudflare` (éphémères), un tunnel par app |
| 4 | Tunnel **géré localement** (`config.yml` versionné, catch-all) | source de vérité dans git, reconstruction sans dashboard | remote-managed (config dans le dashboard, invisible de git) |
| 5 | Secrets **sops + age** dans le repo de config | reconstruction OS vierge + repo + 1 clé ; diff lisible ; pas de serveur de secrets à opérer | `.env` seulement sur l'hôte (dépend du backup), Vault |
| 6 | Un outil **générique** (repo public) + un repo de config **par serveur** (privé) qui l'embarque en sous-module épinglé | l'outil ne gère que l'hôte où il tourne (aucun droit croisé) ; mise à jour = bump du sous-module, zéro merge | fork privé (impossible sur GitHub pour un repo public ; merges à chaque MAJ), un repo pour tous les serveurs |
| 7 | LLM = **`claude -p`** lancé par un runner sur l'hôte, un run par projet modifié, skills sélectionnés par `service`/`expose`/`previews` | simple, un seul fournisseur, les skills sont de la doc versionnée | orchestrateur maison, agent permanent |
| 8 | Le CLI `infra` fait **tout ce qui est mécanique** (fetch, render, up, dns) ; le LLM ne fait que ce qui demande du jugement | idempotence et reproductibilité ne dépendent pas du LLM ; coût et durée des runs réduits | tout confier au LLM |
| 9 | Les repos projets restent **génériques** (un `docker-compose.yml` qui marche) ; labels/réseaux/ports générés en override | un projet n'a rien à savoir du serveur ; `ports:` neutralisés par `!reset` | labels Traefik dans chaque repo (comme alpage aujourd'hui) |
| 10 | Un compte Unix **par agent IA**, clés versionnées, sudo limité + sudoreplay | traçabilité, révocation = suppression d'un fichier | compte partagé `alpage`/`agent` |
| 11 | Previews déclenchées par GitHub Actions **via Tailscale** (nœud éphémère) | plus de SSH exposé sur internet, même mécanique Pi/VPS | runner self-hosted par repo, SSH public |

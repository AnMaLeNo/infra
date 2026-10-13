# Exemple de repo de config

```
host.yaml          le serveur
registry.yaml      les projets
.sops.yaml         destinataires age
secrets/           infra.env, cloudflared.json, projects/<P>.env  (chiffrés)
agents/            clés publiques des agents IA
skills/            skills custom (même arborescence que tool/skills/, prioritaires)
tool/              sous-module → AnMaLeNo/infra @ vX.Y.Z
docs/migrations.md journal des migrations
```

Créer un repo de config :
```bash
git init infra-monserveur && cd infra-monserveur
git submodule add https://github.com/AnMaLeNo/infra.git tool
cp -r tool/example-config/{host.yaml,registry.yaml,.sops.yaml,agents} .
mkdir -p secrets/projects skills
```

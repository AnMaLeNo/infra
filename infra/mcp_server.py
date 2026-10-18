"""Serveur MCP (stdio) : expose infra/tools.py à Claude Code. Lancé par le runner via --mcp-config."""
from __future__ import annotations

from mcp.server.mcpserver import MCPServer

from . import tools

server = MCPServer("infra", instructions="Tools atomiques de l'outil infra : synchroniser un repo, générer l'override compose, "
                   "calculer les labels Traefik, déchiffrer un .env, gérer le DNS Cloudflare, lire l'état. Chaque tool échoue "
                   "clairement quand il ne s'applique pas : fais alors autrement.")

server.tool(name="host_info")(tools.host_info)
server.tool(name="registry_get")(tools.registry_get)
server.tool(name="registry_set")(tools.registry_set)
server.tool(name="repo_sync")(tools.repo_sync)
server.tool(name="compose_override")(tools.compose_override)
server.tool(name="traefik_labels")(tools.traefik_labels_tool)
server.tool(name="env_decrypt")(tools.env_decrypt)
server.tool(name="secrets_keys")(tools.secrets_keys_tool)
server.tool(name="project_status")(tools.project_status)
server.tool(name="dns_ensure")(tools.dns_ensure)
server.tool(name="dns_prune")(tools.dns_prune)
server.tool(name="dns_list")(tools.dns_list)
server.tool(name="stack_status")(tools.stack_status)
server.tool(name="skills_list")(tools.skills_list)


def main() -> None:
    server.run("stdio")


if __name__ == "__main__":
    main()

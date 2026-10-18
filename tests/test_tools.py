import pytest

from infra import tools
from infra.mcp_server import server


def test_mcp_tools_registered():
    import asyncio
    names = {t.name for t in asyncio.run(server.list_tools())}
    assert {"repo_sync", "compose_override", "traefik_labels", "dns_ensure", "project_status", "registry_set"} <= names
    desc = {t.name: t.description for t in asyncio.run(server.list_tools())}
    assert "Échoue si le repo n'a pas de compose" in desc["compose_override"]


def test_compose_override_requires_sync(env):
    with pytest.raises(FileNotFoundError, match="repo_sync"):
        tools.compose_override("catacombes")


def test_compose_override_without_compose(env, tmp_path):
    from infra import paths
    repo = paths.app_dir("catacombes") / "repo"
    repo.mkdir(parents=True)
    (repo / "server.py").write_text("")
    with pytest.raises(FileNotFoundError, match="pas de docker-compose.yml"):
        tools.compose_override("catacombes")


def test_traefik_labels_tool_and_registry_set(env):
    out = tools.traefik_labels_tool("catacombes")
    assert out["urls"]["public"] == "https://catacombes.antoine42.com"
    assert any("catacombes-public.entrypoints=tunnel" in lab for lab in out["labels"])
    tools.registry_set("catacombes", {"port": 9999, "notes": None})
    assert tools.registry_get("catacombes")["catacombes"]["port"] == 9999
    assert tools.host_info()["host"]["name"] == "pi"

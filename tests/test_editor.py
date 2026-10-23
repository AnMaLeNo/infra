import pytest

from infra import editor, secrets, stack


def test_command_binds_gateway_and_injects_token(env, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: "/usr/local/bin/code-server")
    monkeypatch.setattr(stack, "network_gateway", lambda: "172.30.0.1")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": "tok"})
    argv, e = editor.command()
    assert argv[:3] == ["/usr/local/bin/code-server", "--bind-addr", "172.30.0.1:8443"]
    assert argv[3].endswith("config")   # le dossier des runs : même cwd que `claude -p`
    assert e["CLAUDE_CODE_OAUTH_TOKEN"] == "tok" and e["GIT_TERMINAL_PROMPT"] == "0"


def test_command_requires_code_server(env, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: None)
    with pytest.raises(RuntimeError, match="code-server"):
        editor.command()

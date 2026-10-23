import pytest

from infra import editor, secrets, stack


@pytest.fixture
def home(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    return tmp_path / "home"


def test_command_serves_web_on_gateway_with_token_file(env, home, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: "/usr/local/bin/vscode-cli")
    monkeypatch.setattr(stack, "network_gateway", lambda: "172.30.0.1")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": "tok"})
    argv, e = editor.command()
    assert argv[:2] == ["/usr/local/bin/vscode-cli", "serve-web"]
    assert argv[argv.index("--host") + 1] == "172.30.0.1" and argv[argv.index("--port") + 1] == "8443"
    assert argv[argv.index("--default-folder") + 1].endswith("config")   # le dossier des runs, comme `claude -p`
    assert "--without-connection-token" not in argv
    f = home / ".config" / "infra-editor" / "token"
    assert argv[argv.index("--connection-token-file") + 1] == str(f)
    assert f.stat().st_mode & 0o777 == 0o600 and len(f.read_text()) >= 24
    assert e["CLAUDE_CODE_OAUTH_TOKEN"] == "tok" and e["GIT_TERMINAL_PROMPT"] == "0"


def test_token_is_stable_across_launches(env, home, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: "/x/vscode-cli")
    monkeypatch.setattr(stack, "network_gateway", lambda: "172.30.0.1")
    editor.command()
    first = editor.token_file().read_text()
    editor.command()
    assert editor.token_file().read_text() == first


def test_command_requires_cli(env, home, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: None)
    with pytest.raises(RuntimeError, match="vscode-cli"):
        editor.command()

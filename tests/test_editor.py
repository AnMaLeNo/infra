import pytest

from infra import editor, secrets


@pytest.fixture
def home(tmp_path, monkeypatch):
    monkeypatch.setenv("HOME", str(tmp_path))
    return tmp_path


def test_command_serves_web_with_token_file(env, home, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: "/usr/local/bin/vscode-cli")
    monkeypatch.setattr(secrets, "infra_env", lambda: {"CLAUDE_CODE_OAUTH_TOKEN": "tok"})
    argv, e = editor.command("172.30.0.1")
    assert argv[:2] == ["/usr/local/bin/vscode-cli", "serve-web"]
    assert argv[argv.index("--host") + 1] == "172.30.0.1" and "--without-connection-token" not in argv
    assert argv[argv.index("--cli-data-dir") + 1] == str(editor.data_dir())
    assert argv[argv.index("--default-folder") + 1].endswith("config")   # le dossier des runs, comme `claude -p`
    f = home / ".config" / "infra-editor" / "token"
    assert argv[argv.index("--connection-token-file") + 1] == str(f)
    assert f.stat().st_mode & 0o777 == 0o600
    first = f.read_text()
    editor.command("172.30.0.1")
    assert f.read_text() == first   # stable d'un lancement à l'autre
    assert e["CLAUDE_CODE_OAUTH_TOKEN"] == "tok" and e["GIT_TERMINAL_PROMPT"] == "0"


def test_command_requires_cli(env, home, monkeypatch):
    monkeypatch.setattr(editor.shutil, "which", lambda n: None)
    with pytest.raises(RuntimeError, match="vscode-cli"):
        editor.command("172.30.0.1")


def test_install_extension_waits_for_server_then_installs(home, monkeypatch):
    srv = editor.data_dir() / "serve-web/abc/bin/code-server"
    srv.parent.mkdir(parents=True)
    srv.touch()
    codes, ran = iter([202, 302]), []
    monkeypatch.setattr(editor.httpx, "get", lambda url, timeout: type("R", (), {"status_code": next(codes)})())
    monkeypatch.setattr(editor.time, "sleep", lambda s: None)
    monkeypatch.setattr(editor.subprocess, "run", lambda cmd, env, check: ran.append(cmd) or type("P", (), {"returncode": 0})())
    assert editor.install_extension("172.30.0.1", {}) is True
    assert ran == [[str(srv), "--install-extension", "anthropic.claude-code"]]

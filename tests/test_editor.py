import json
import zipfile

import pytest

from infra import editor, paths, runner, secrets


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
    cli = [str(srv), "--server-data-dir", str(editor.server_dir()), "--install-extension"]
    assert ran == [[*cli, "anthropic.claude-code"], [*cli, str(editor.data_dir() / "infra-open-session.vsix"), "--force"]]


def test_configure_shows_run_sessions_and_keeps_other_settings(home):
    f = editor.server_dir() / "data" / "Machine" / "settings.json"
    f.parent.mkdir(parents=True)
    f.write_text('{"editor.fontSize": 14}')
    editor.configure()
    assert json.loads(f.read_text()) == {"editor.fontSize": 14, "claudeCode.claudeProcessWrapper": "/usr/bin/env"}


def test_vsix_has_the_layout_vscode_expects(home):
    with zipfile.ZipFile(editor.build_vsix()) as z:
        assert set(z.namelist()) == {"[Content_Types].xml", "extension.vsixmanifest",
                                     "extension/package.json", "extension/extension.js"}


def test_request_session_drops_the_id_for_the_extension(env):
    editor.request_session("FD3ACD69-38C1-4560-9A1A-F04596739D38")
    assert (paths.runs_dir() / ".editor-open").read_text() == "fd3acd69-38c1-4560-9a1a-f04596739d38"
    with pytest.raises(ValueError):
        editor.request_session("../../etc/passwd")


def test_session_of_prefers_the_recorded_id_then_reads_the_log(env):
    runs = paths.runs_dir()
    runs.mkdir(exist_ok=True)
    (runs / "a-p.json").write_text('{"session_id": "recorded"}')
    (runs / "b-p.json").write_text("{}")
    (runs / "b-p.log").write_text('{"type": "system", "session_id": "from-log"}\n{"type": "assistant"}\n')
    (runs / "c-p.json").write_text("{}")
    assert [runner.session_of(f) for f in ("a-p.json", "b-p.json", "c-p.json")] == ["recorded", "from-log", None]


def test_api_route_requests_the_session_of_a_run(env):
    from fastapi.testclient import TestClient

    from infra import api
    runs = paths.runs_dir()
    runs.mkdir(exist_ok=True)
    (runs / "a-p.json").write_text('{"session_id": "fd3acd69-38c1-4560-9a1a-f04596739d38"}')
    (runs / "b-p.json").write_text("{}")
    c = TestClient(api.app)
    assert c.post("/api/runs/a-p.json/editor").json() == {"ok": True}
    assert (runs / ".editor-open").read_text() == "fd3acd69-38c1-4560-9a1a-f04596739d38"
    assert c.post("/api/runs/b-p.json/editor").status_code == 404      # run sans session (dry, très ancien)
    assert c.post("/api/runs/absent.json/editor").status_code == 404
    assert c.post("/api/runs/..%2Fx.json/editor").status_code in (400, 404)

from infra import secrets


def test_check_ignores_dotfiles(env):
    d = secrets.secrets_dir()
    (d / "projects").mkdir(parents=True, exist_ok=True)
    (d / "projects" / ".gitkeep").write_text("")
    assert secrets.check() == {}

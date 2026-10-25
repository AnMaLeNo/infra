"""Commit + push automatiques du repo de config après un run."""
import subprocess

from infra import config, gitsync, paths, runner


def _init_repo(tmp_path):
    cfg = paths.config_dir()
    bare = tmp_path / "remote.git"
    subprocess.run(["git", "init", "-q", "--bare", str(bare)], check=True)
    subprocess.run(["git", "-C", str(cfg), "init", "-q", "-b", "main"], check=True)
    subprocess.run(["git", "-C", str(cfg), "remote", "add", "origin", str(bare)], check=True)
    (cfg / ".gitignore").write_text("runs/\ntool\n")
    subprocess.run(["git", "-C", str(cfg), "-c", "user.name=t", "-c", "user.email=t@t", "add", "-A"], check=True)
    subprocess.run(["git", "-C", str(cfg), "-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "init"], check=True)
    subprocess.run(["git", "-C", str(cfg), "push", "-q", "-u", "origin", "main"], check=True)
    return bare


def test_commit_push_only_when_changed(env):
    bare = _init_repo(env)
    assert gitsync.commit_push("rien") == {"ok": True, "committed": False, "pushed": False}
    config.set_project_fields("catacombes", source="llm", container="web", port=8420)
    out = gitsync.commit_push("run catacombes : ok — constats")
    assert out["ok"] and out["committed"] and out["pushed"]
    log = subprocess.run(["git", "-C", str(bare), "log", "-1", "--format=%an|%s"], check=False, capture_output=True, text=True).stdout.strip()
    assert log == "infra (pi)|run catacombes : ok — constats"


def test_no_git_dir_is_reported_not_raised(env):
    out = gitsync.commit_push("x")
    assert out["ok"] is False and "dépôt git" in out["error"]


def test_push_failure_is_reported(env):
    bare = _init_repo(env)
    subprocess.run(["git", "-C", str(paths.config_dir()), "remote", "set-url", "origin", str(env / "absent.git")], check=True)
    config.set_project_fields("catacombes", source="llm", port=1)
    out = gitsync.commit_push("run")
    assert out["committed"] and not out["pushed"] and "push refusé" in out["error"]
    assert bare.exists()


def test_run_record_carries_git_result(env):
    _init_repo(env)
    runner.enqueue("catacombes")
    rec = runner.process_one(dry=True)          # dry : pas de claude, pas de commit
    assert "git" not in rec

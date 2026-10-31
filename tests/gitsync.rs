mod common;

use std::process::Command;

use infra::{config, gitsync, paths};
use serde_json::json;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn init_repo(e: &common::Env) -> std::path::PathBuf {
    let cfg = paths::config_dir();
    let bare = e.root.path().join("remote.git");
    assert!(Command::new("git").args(["init", "-q", "--bare"]).arg(&bare).status().unwrap().success());
    git(&cfg, &["init", "-q", "-b", "main"]);
    git(&cfg, &["remote", "add", "origin", &bare.to_string_lossy()]);
    std::fs::write(cfg.join(".gitignore"), "runs/\ntool\n").unwrap();
    git(&cfg, &["add", "-A"]);
    git(&cfg, &["commit", "-q", "-m", "init"]);
    git(&cfg, &["push", "-q", "-u", "origin", "main"]);
    bare
}

#[test]
fn commit_push_only_when_changed() {
    let e = common::env();
    let bare = init_repo(&e);
    assert_eq!(gitsync::commit_push("rien"), json!({"ok": true, "committed": false, "pushed": false}));
    config::set_project_fields("catacombes", "llm", json!({"container": "web", "port": 8420}).as_object().unwrap()).unwrap();
    let out = gitsync::commit_push("run catacombes : ok — constats");
    assert!(out["ok"] == true && out["committed"] == true && out["pushed"] == true, "{out}");
    assert_eq!(git(&bare, &["log", "-1", "--format=%an|%s", "main"]), "infra (pi)|run catacombes : ok — constats");
}

#[test]
fn no_git_dir_is_reported_not_raised() {
    let _e = common::env();
    let out = gitsync::commit_push("x");
    assert!(out["ok"] == false && out["error"].as_str().unwrap().contains("dépôt git"));
}

#[test]
fn push_failure_is_reported() {
    let e = common::env();
    let bare = init_repo(&e);
    git(&paths::config_dir(), &["remote", "set-url", "origin", &e.root.path().join("absent.git").to_string_lossy()]);
    config::set_project_fields("catacombes", "llm", json!({"port": 1}).as_object().unwrap()).unwrap();
    let out = gitsync::commit_push("run");
    assert!(out["committed"] == true && out["pushed"] == false && out["error"].as_str().unwrap().contains("push refusé"), "{out}");
    assert!(bare.exists());
}

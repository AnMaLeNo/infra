//! Commit + push automatiques du repo de config après chaque run : tout ce que Claude ou l'outil écrit
//! (registry.yaml, shared.yaml, shared/<S>/, host.yaml…) est versionné sans geste humain, avec l'historique complet.
//! Le push passe par https + GITHUB_TOKEN (secrets/infra.env) même si le remote est en ssh (voir compose::git_env).
use std::collections::HashMap;
use std::process::{Command, Output};

use serde_json::{json, Value};

use crate::{compose, config, paths, secrets};

fn git(args: &[&str], env: &HashMap<String, String>) -> Output {
    // safe.directory explicite : le repo de config appartient à root et l'UI tourne en ai-infra dans un conteneur
    // (sans le /etc/gitconfig de l'hôte) → sinon « dubious ownership » et aucun commit depuis le front.
    let cfg = paths::config_dir().to_string_lossy().into_owned();
    Command::new("git")
        .args(["-c", &format!("safe.directory={cfg}"), "-C", &cfg])
        .args(args)
        .envs(env)
        .output()
        .unwrap_or_else(|e| Output { status: std::process::ExitStatus::default(), stdout: vec![], stderr: e.to_string().into_bytes() })
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim().to_string()
}

/// `git add -A` + commit + push du repo de config. Idempotent : rien à commiter → {"committed": false}.
/// N'échoue jamais bruyamment (le run est déjà fini) : retourne ok/erreur, token masqué.
pub fn commit_push(message: &str) -> Value {
    if !paths::config_dir().join(".git").exists() {
        return json!({"ok": false, "committed": false, "error": "le dossier de config n'est pas un dépôt git"});
    }
    let ienv = secrets::infra_env();
    let vals: Vec<String> = ienv.values().cloned().collect();
    let env = compose::git_env();
    let host = config::host().map(|h| h.name).unwrap_or_else(|_| "host".into());
    let mut out = json!({"ok": true, "committed": false, "pushed": false});
    let r = git(&["add", "-A"], &env);
    if !r.status.success() {
        out["ok"] = json!(false);
        out["error"] = json!(secrets::mask(&text(&r.stderr), &vals));
        return out;
    }
    if git(&["diff", "--cached", "--quiet"], &env).status.success() {
        return out;
    }
    let (un, ue) = (format!("user.name=infra ({host})"), format!("user.email=infra@{host}.invalid"));
    let r = git(&["-c", &un, "-c", &ue, "commit", "-q", "-m", message], &env);
    if !r.status.success() {
        let msg = if r.stderr.is_empty() { text(&r.stdout) } else { text(&r.stderr) };
        out["ok"] = json!(false);
        out["error"] = json!(secrets::mask(&msg, &vals));
        return out;
    }
    out["committed"] = json!(true);
    out["commit"] = json!(text(&git(&["rev-parse", "--short", "HEAD"], &env).stdout));
    let r = git(&["push", "-q", "origin", "HEAD"], &env);
    if !r.status.success() {
        let err = text(&r.stderr);
        let tail: String = err.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        out["ok"] = json!(false);
        out["error"] = json!(format!("commit local fait mais push refusé : {}", secrets::mask(&tail, &vals)));
        return out;
    }
    out["pushed"] = json!(true);
    out
}

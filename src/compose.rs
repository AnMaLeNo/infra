//! Opérations git + docker compose sur une cible (projet ou preview).
use std::collections::HashMap;
use std::fs;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::render::Target;
use crate::secrets;
use crate::util::{run_json, Cmd, Output};

/// Env pour git/gh : jamais de prompt interactif, et si `GITHUB_TOKEN` est dans secrets/infra.env,
/// authentification https sur github.com via la config git passée en variables (rien n'est écrit sur disque).
pub fn git_env() -> HashMap<String, String> {
    let mut env = HashMap::from([("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())]);
    if let Some(token) = secrets::infra_env().get("GITHUB_TOKEN") {
        // les deux formes de remote (https et ssh) passent par https + token : ai-infra n'a pas de clé ssh
        let url = format!("url.https://x-access-token:{token}@github.com/.insteadOf");
        env.insert("GIT_CONFIG_COUNT".into(), "2".into());
        env.insert("GIT_CONFIG_KEY_0".into(), url.clone());
        env.insert("GIT_CONFIG_VALUE_0".into(), "https://github.com/".into());
        env.insert("GIT_CONFIG_KEY_1".into(), url);
        env.insert("GIT_CONFIG_VALUE_1".into(), "git@github.com:".into());
        env.insert("GH_TOKEN".into(), token.clone());
    }
    env
}

fn git(repo: &str, args: &[&str], env: Option<&HashMap<String, String>>) -> Result<Output> {
    let mut cmd = vec!["git", "-C", repo];
    cmd.extend(args);
    let mut c = Cmd::new().quiet();
    if let Some(e) = env {
        c = c.env(e);
    }
    c.run(&cmd)
}

/// Clone ou met à jour le repo sur `ref`. Retourne le commit.
pub fn fetch(t: &Target) -> Result<String> {
    let repo = t.repo_dir();
    let repo_s = repo.to_string_lossy().into_owned();
    let r#ref = match t.pr {
        None => t.project.r#ref.clone(),
        Some(pr) => format!("refs/pull/{pr}/head"),
    };
    let env = git_env();
    if !repo.join(".git").exists() {
        fs::create_dir_all(repo.parent().unwrap())?;
        if let Err(e) = Cmd::new().env(&env).run(&["git", "clone", "--quiet", &t.project.repo, &repo_s]) {
            let hint = if env.contains_key("GH_TOKEN") {
                "GITHUB_TOKEN est défini mais refusé : vérifie ses droits (Contents) sur ce repo"
            } else {
                "repo privé ou URL fausse ; pour un repo privé : `infra secrets set infra.env GITHUB_TOKEN` (token fine-grained, Contents read)"
            };
            let msg = secrets::mask(&e.to_string(), &[env.get("GH_TOKEN").cloned().unwrap_or_default()]);
            return Err(anyhow!("clone de {} impossible : {hint}. {msg}", t.project.repo));
        }
    }
    git(&repo_s, &["fetch", "--quiet", "--prune", "origin", &r#ref], Some(&env))?;
    git(&repo_s, &["checkout", "--quiet", "--detach", "FETCH_HEAD"], None)?;
    git(&repo_s, &["clean", "-fdq", "-e", ".env"], None)?;
    Ok(git(&repo_s, &["rev-parse", "--short", "HEAD"], None)?.stdout.trim().to_string())
}

pub fn compose_cmd(t: &Target) -> Vec<String> {
    let mut cmd: Vec<String> = ["docker", "compose", "-p", &t.compose_project(), "--project-directory"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    cmd.push(t.repo_dir().to_string_lossy().into_owned());
    cmd.extend(["-f".to_string(), t.compose_file().to_string_lossy().into_owned()]);
    if t.override_file().exists() {
        cmd.extend(["-f".to_string(), t.override_file().to_string_lossy().into_owned()]);
    }
    if t.env_file().exists() {
        cmd.extend(["--env-file".to_string(), t.env_file().to_string_lossy().into_owned()]);
    }
    cmd
}

pub fn up(t: &Target) -> Result<()> {
    let mut cmd = compose_cmd(t);
    cmd.extend(["up", "-d", "--build", "--remove-orphans", "--quiet-pull"].map(String::from));
    Cmd::new().no_capture().run(&cmd)?;
    Ok(())
}

pub fn down(t: &Target, volumes: bool) -> Result<()> {
    if !t.compose_file().exists() {
        // plus de code : on arrête par nom de projet
        let filter = format!("label=com.docker.compose.project={}", t.compose_project());
        let ids = Cmd::new().quiet().run(&["docker", "ps", "-aq", "--filter", &filter])?.stdout;
        let ids: Vec<&str> = ids.split_whitespace().collect();
        if !ids.is_empty() {
            let mut cmd = vec!["docker", "rm", "-f"];
            cmd.extend(ids);
            Cmd::new().quiet().run(&cmd)?;
        }
        return Ok(());
    }
    let mut cmd = compose_cmd(t);
    cmd.extend(["down", "--remove-orphans"].map(String::from));
    if volumes {
        cmd.push("-v".into());
    }
    Cmd::new().no_capture().run(&cmd)?;
    Ok(())
}

pub fn ps(t: &Target) -> Result<Vec<Value>> {
    run_json(&["docker", "compose", "-p", &t.compose_project(), "ps", "-a", "--format", "json"], None, None)
}

pub fn published_ports(t: &Target) -> Result<Vec<String>> {
    let mut out = vec![];
    for c in ps(t)? {
        for p in c.get("Publishers").and_then(Value::as_array).into_iter().flatten() {
            if let Some(pp) = p.get("PublishedPort").and_then(Value::as_u64).filter(|n| *n > 0) {
                out.push(format!(
                    "{}:{}->{}/{}",
                    p.get("URL").and_then(Value::as_str).unwrap_or(""),
                    pp,
                    p.get("TargetPort").and_then(Value::as_u64).unwrap_or(0),
                    p.get("Protocol").and_then(Value::as_str).unwrap_or("")
                ));
            }
        }
    }
    Ok(out)
}

pub fn status(t: &Target) -> Result<Value> {
    let containers = ps(t)?;
    let s = |c: &Value, k: &str| c.get(k).and_then(Value::as_str).map(str::to_string);
    let states: serde_json::Map<String, Value> = containers
        .iter()
        .map(|c| {
            let st = s(c, "Health").filter(|h| !h.is_empty()).or_else(|| s(c, "State")).unwrap_or_default();
            (s(c, "Service").unwrap_or_default(), Value::String(st))
        })
        .collect();
    let mut nets: Vec<String> = vec![];
    if t.expose() != "none" && !containers.is_empty() {
        let container = if t.project.service.as_deref() == Some("static") {
            Some("web".to_string())
        } else {
            t.project.container.clone().or_else(|| states.keys().next().cloned())
        };
        for c in &containers {
            if s(c, "Service") == container {
                if let Some(id) = s(c, "ID") {
                    let info = run_json(&["docker", "inspect", &id], None, None)?;
                    nets = info
                        .first()
                        .and_then(|i| i.pointer("/NetworkSettings/Networks"))
                        .and_then(Value::as_object)
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                }
            }
        }
    }
    let running = !containers.is_empty() && states.values().all(|v| matches!(v.as_str(), Some("running" | "healthy")));
    let commit = if t.repo_dir().exists() {
        git(&t.repo_dir().to_string_lossy(), &["rev-parse", "--short", "HEAD"], None)
            .ok()
            .map(|o| Value::String(o.stdout.trim().to_string()))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(json!({
        "project": t.compose_project(),
        "running": running,
        "containers": states,
        "published_ports": published_ports(t)?,
        "traefik_net": nets.iter().any(|n| n == "traefik-net"),
        "urls": if containers.is_empty() { json!({}) } else { json!(t.urls()) },
        "commit": commit,
    }))
}

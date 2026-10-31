//! Tools atomiques exposés au LLM (MCP) et au CLI. Chaque fonction fait UNE chose, est idempotente,
//! et lève une erreur explicite quand elle ne s'applique pas (le LLM fait alors autrement).
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use crate::render::{missing_env_vars, render, render_env, traefik_labels, Target};
use crate::{compose, config, dns, paths, secrets, skills, stack};

pub fn target(project: &str, pr: Option<u64>) -> Result<Target> {
    Ok(Target::new(project, config::project(project)?, config::host()?, pr))
}

fn s(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn host_info() -> Result<Value> {
    let h = config::host()?;
    Ok(json!({
        "host": crate::models::to_map(&h),
        "paths": {"config": s(&paths::config_dir()), "tool": s(&paths::tool_dir()), "apps": s(&paths::apps_dir()),
                  "data": s(&paths::data_dir()), "stack_state": s(&paths::stack_state_dir())},
        "conventions": s(&paths::tool_dir().join("docs/conventions.md")),
    }))
}

pub fn registry_get(project: Option<&str>) -> Result<Value> {
    if let Some(p) = project {
        return Ok(json!({p: config::project(p)?.dump()}));
    }
    Ok(Value::Object(config::registry()?.projects.iter().map(|(n, p)| (n.clone(), Value::Object(p.dump()))).collect()))
}

pub fn registry_set(project: &str, fields: &Map<String, Value>) -> Result<Value> {
    Ok(json!({project: config::set_project_fields(project, "llm", fields)?.dump_non_default()}))
}

pub fn repo_sync(project: &str, pr: Option<u64>) -> Result<Value> {
    let t = target(project, pr)?;
    let commit = compose::fetch(&t)?;
    let mut files: Vec<String> = fs::read_dir(t.repo_dir())?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with(".git"))
        .collect();
    files.sort();
    files.truncate(50);
    Ok(json!({
        "path": s(&t.repo_dir()), "commit": commit,
        "ref": match pr { None => t.project.r#ref.clone(), Some(n) => format!("refs/pull/{n}/head") },
        "has_compose": t.repo_dir().join(&t.project.compose_file).exists(),
        "has_dockerfile": t.repo_dir().join("Dockerfile").exists(),
        "top_level": files,
    }))
}

pub fn compose_override(project: &str, pr: Option<u64>) -> Result<Value> {
    let t = target(project, pr)?;
    if !t.repo_dir().exists() {
        bail!("repo non synchronisé : appelle repo_sync({project:?}) d'abord");
    }
    if t.project.service.as_deref() != Some("static") && !t.compose_file().exists() {
        bail!(
            "pas de {} dans {} : ce projet n'est pas un `service: compose`. Écris un Dockerfile + docker-compose.yml \
             (PR sur le repo, voir migrate-legacy) ou passe en `service: static`.",
            t.project.compose_file,
            t.repo_dir().display()
        );
    }
    let changed = render(&t)?;
    Ok(json!({
        "override": s(&t.override_file()), "content": fs::read_to_string(t.override_file())?, "changed": changed,
        "env_file": if t.env_file().exists() { Some(s(&t.env_file())) } else { None },
        "missing_env": missing_env_vars(&t)?,
        "compose_cmd": compose::compose_cmd(&t).join(" "),
    }))
}

pub fn traefik_labels_tool(project: &str, pr: Option<u64>) -> Result<Value> {
    let t = target(project, pr)?;
    Ok(json!({"labels": traefik_labels(&t)?, "networks": ["traefik-net"], "urls": t.urls()}))
}

pub fn env_decrypt(project: &str, pr: Option<u64>) -> Result<Value> {
    let t = target(project, pr)?;
    fs::create_dir_all(t.dir())?;
    if !t.project.env {
        render_env(&t)?;
        return Ok(json!({"env_file": null, "note": "env: false dans le registre ; aucun .env écrit"}));
    }
    render_env(&t)?;
    Ok(json!({"env_file": s(&t.env_file()), "keys": secrets::keys(&secrets::project_env_path(project, false))?}))
}

pub fn secrets_keys_tool(rel: &str) -> Result<Value> {
    Ok(json!({"file": format!("secrets/{rel}"), "keys": secrets::keys(rel)?}))
}

pub fn project_status(project: &str) -> Result<Value> {
    let t = target(project, None)?;
    let mut st = compose::status(&t)?;
    st["wanted"] = Value::Object(t.project.dump_non_default());
    st["paths"] = json!({"dir": s(&t.dir()), "repo": s(&t.repo_dir()), "override": s(&t.override_file()), "data": s(&paths::data_dir().join(project))});
    Ok(st)
}

pub fn dns_ensure(project: &str, force: bool) -> Result<Value> {
    Ok(json!(dns::ensure_project(project, &config::project(project)?, &config::host()?, force)?))
}

pub fn dns_prune(project: &str, public_only: bool) -> Result<Value> {
    Ok(json!(dns::prune_project(project, &config::project(project)?, &config::host()?, public_only)?))
}

pub fn dns_list() -> Result<Value> {
    Ok(json!(dns::list_records(&config::host()?)?))
}

pub fn stack_status() -> Result<Value> {
    let mut out = Map::new();
    for n in stack::all_names()? {
        out.insert(n.clone(), json!(stack::status(&n)?));
    }
    Ok(Value::Object(out))
}

fn detected_lan_ip() -> Option<String> {
    let out = Command::new("ip").args(["-4", "route", "get", "1.1.1.1"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let words: Vec<&str> = text.split_whitespace().collect();
    let i = words.iter().position(|w| *w == "src")?;
    words.get(i + 1).map(|w| w.to_string())
}

pub fn host_addresses() -> Result<Value> {
    let h = config::host()?;
    let detected = detected_lan_ip();
    let warning = match (&h.lan_ip, &detected) {
        (Some(lan), det) if !lan.is_empty() && Some(lan) != det.as_ref() => Some(format!(
            "host.yaml lan_ip={lan} mais l'IP LAN détectée est {} : signale-le, ne corrige pas tout seul",
            det.clone().unwrap_or_else(|| "None".into())
        )),
        _ => None,
    };
    Ok(json!({"tailscale_ip": h.tailscale_ip, "lan_ip": h.lan_ip, "lan_ip_detected": detected,
              "traefik_net_gateway": stack::network_gateway()?, "warning": warning}))
}

pub fn shared_get(name: Option<&str>) -> Result<Value> {
    let names: Vec<String> = match name {
        Some(n) => vec![n.to_string()],
        None => config::shared()?.services.keys().cloned().collect(),
    };
    let mut out = Map::new();
    for n in names {
        let mut m = config::shared_service(&n)?.dump();
        m.insert("users".into(), json!(config::users_of(&n)?));
        m.insert("dir".into(), json!(s(&paths::shared_dir(&n))));
        m.insert("state".into(), json!(s(&paths::shared_state_dir(&n))));
        out.insert(n, Value::Object(m));
    }
    Ok(Value::Object(out))
}

pub fn shared_set(name: &str, fields: &Map<String, Value>) -> Result<Value> {
    config::shared_service(name)?;
    let human = ["kind", "listen", "port", "auth", "data", "enabled"];
    let mut bad: Vec<&String> = fields.keys().filter(|k| human.contains(&k.as_str())).collect();
    bad.sort();
    if !bad.is_empty() {
        bail!("{bad:?} sont des choix humains : pas via shared_set (demande-les dans needs_human)");
    }
    Ok(json!({name: config::set_shared_fields(name, "llm", fields)?.dump_non_default()}))
}

fn cmd_out(cmd: &[&str]) -> String {
    Command::new(cmd[0]).args(&cmd[1..]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

pub fn shared_status(name: &str) -> Result<Value> {
    let svc = config::shared_service(name)?;
    let proj = format!("shared-{name}");
    let ps = cmd_out(&["docker", "compose", "-p", &proj, "ps", "-a", "--format", "json"]);
    let containers: Vec<Value> = ps
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|c| {
            json!({"name": c.get("Name"), "service": c.get("Service"), "state": c.get("State"), "health": c.get("Health"),
                   "image": c.get("Image"), "ports": c.get("Publishers").cloned().unwrap_or(json!([]))})
        })
        .collect();
    let mut aliases: Vec<String> = vec![];
    for c in &containers {
        let n = c["name"].as_str().unwrap_or("");
        let ins = cmd_out(&["docker", "inspect", n, "--format", "{{json .NetworkSettings.Networks}}"]);
        let nets: Value = serde_json::from_str(ins.trim()).unwrap_or(json!({}));
        if let Some(a) = nets.get(stack::NETWORK).and_then(|n| n.get("Aliases")).and_then(Value::as_array) {
            aliases.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
    }
    let mut listening: Vec<String> = vec![];
    let mut clients: Option<usize> = None;
    if let Some(port) = svc.port {
        let filt = format!("( sport = :{port} )");
        listening = cmd_out(&["ss", "-Hltn", &filt])
            .lines()
            .filter_map(|l| l.split_whitespace().nth(3).map(str::to_string))
            .collect();
        clients = Some(cmd_out(&["ss", "-Htn", "state", "established", &filt]).lines().count());
    }
    let h = config::host()?;
    let listen_wanted: Vec<String> = match svc.port {
        Some(port) => svc
            .listen
            .iter()
            .filter_map(|x| match x.as_str() {
                "lan" => h.lan_ip.clone().filter(|v| !v.is_empty()),
                "tailnet" => Some(h.tailscale_ip.clone()),
                _ => None,
            })
            .map(|ip| format!("{ip}:{port}"))
            .collect(),
        None => vec![],
    };
    Ok(json!({
        "wanted": svc.dump_non_default(), "users": config::users_of(name)?, "containers": containers,
        "alias_ok": aliases.iter().any(|a| a == name), "listening": listening, "listen_wanted": listen_wanted,
        "on_all_interfaces": listening.iter().any(|a| a.starts_with("0.0.0.0:") || a.starts_with("*:") || a.starts_with("[::]:")),
        "tcp_clients": clients,
        "paths": {"dir": s(&paths::shared_dir(name)), "state": s(&paths::shared_state_dir(name))},
    }))
}

pub fn skills_list() -> Result<Value> {
    Ok(json!(skills::list_all()))
}

pub fn report_path() -> PathBuf {
    paths::runs_dir().join(".report.json")
}

pub fn run_report(project: &str, status: &str, summary: &str, needs_human: &str) -> Result<Value> {
    if !matches!(status, "ok" | "bloqué" | "échec") {
        return Err(anyhow!("status doit être ok|bloqué|échec"));
    }
    let rep = json!({"project": project, "status": status, "summary": summary, "needs_human": needs_human, "at": crate::util::now_iso()});
    let p = report_path();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(&p, serde_json::to_string(&rep)?)?;
    Ok(rep)
}

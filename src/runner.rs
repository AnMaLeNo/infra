//! Lance `claude -p` sur un projet avec les bons skills, et journalise dans <config>/runs/.
//!
//! Rien ne se déclenche tout seul : un run est demandé explicitement (bouton « Lancer Claude » du front → `enqueue`,
//! ou `infra run <P>` en CLI). `infra watch` (service infra-runner) consomme la file `runs/queue/` un projet à la fois.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Map, Value};

use crate::models::{Project, Shared};
use crate::state::{self, Entry};
use crate::util::{now_iso, which};
use crate::{compose, config, gitsync, paths, secrets, skills, tools};

pub const ALLOWED_TOOLS: &str = "Bash,Read,Edit,Write,Glob,Grep,WebFetch,mcp__infra";

fn yaml_entry(name: &str, v: Option<Value>) -> String {
    let doc = json!({name: v.unwrap_or(Value::Null)});
    let y: serde_yaml::Value = serde_json::from_value(doc).unwrap_or(serde_yaml::Value::Null);
    serde_yaml::to_string(&y).unwrap_or_default().trim().to_string()
}

fn skills_lines(sk: &indexmap::IndexMap<String, Option<PathBuf>>) -> Vec<String> {
    sk.iter().map(|(rel, p)| format!("- {rel} → {}", p.as_ref().map(|p| p.display().to_string()).unwrap_or_default())).collect()
}

pub fn build_prompt(name: &str, prj: Option<&Project>, previous: Option<&Value>) -> Result<String> {
    let h = config::host()?;
    let sk = skills::paths_for(prj);
    let missing: Vec<&String> = sk.iter().filter(|(_, p)| p.is_none()).map(|(r, _)| r).collect();
    if !missing.is_empty() {
        bail!("skills introuvables : {missing:?} (ni dans <config>/skills ni dans tool/skills)");
    }
    let tool = paths::tool_dir();
    let after = yaml_entry(name, prj.map(|p| Value::Object(p.dump_non_default())));
    let before = yaml_entry(name, previous.cloned());
    let mut lines: Vec<String> = vec![
        format!("Tu es l'agent de déploiement de l'hôte `{}` (outil `infra`). Répertoire courant : {} (repo de config).", h.name, paths::config_dir().display()),
        format!("Contexte à lire d'abord : {} puis {}.", tool.join("README.md").display(), tool.join("docs/conventions.md").display()),
        String::new(),
        format!("Objectif : amener le projet `{name}` à l'état voulu ci-dessous. Tu as carte blanche sur le chemin ;"),
        "les skills décrivent le but et l'état cible à atteindre, pas une procédure.".into(),
        String::new(),
        "Entrée du registre AVANT :".into(), "```yaml".into(), before, "```".into(),
        "Entrée du registre APRÈS (état voulu) :".into(), "```yaml".into(), after, "```".into(),
        String::new(),
        "Skills à satisfaire :".into(),
    ];
    lines.extend(skills_lines(&sk));
    lines.push(String::new());
    if prj.is_some_and(|p| p.enabled && p.service.is_none()) {
        lines.extend([
            "`service` n'est pas fixé dans le registre : regarde le repo et choisis `compose` (il a — ou tu lui écris — un".into(),
            "docker-compose.yml) ou `static` (fichiers/ build servis par nginx), écris ton choix avec registry_set, puis satisfais".into(),
            "le skill service/<choix> correspondant (l'autre est fourni pour que tu puisses comparer).".into(),
            String::new(),
        ]);
    }
    lines.extend([
        "Le *comment* est à toi : `container`, `port`, `compose_file`, `build`, `static_dir` sont des constats que tu".into(),
        "déduis du repo (compose, Dockerfile, package.json…) et que tu écris dans le registre avec registry_set — l'utilisateur".into(),
        "ne les saisit pas et ne les connaît pas.".into(),
        String::new(),
        "Tu disposes du serveur MCP `infra` (tools atomiques : repo_sync, compose_override, traefik_labels, env_decrypt,".into(),
        "dns_ensure/dns_prune/dns_list, registry_get/registry_set, project_status, host_info, secrets_keys…) et de Bash".into(),
        "(docker compose, git, curl, sudo limité). Les tools sont des raccourcis fiables pour les gestes répétitifs ; si l'un".into(),
        "ne convient pas au projet, il te le dira : fais alors autrement, à la main, et dis-le dans ton résumé.".into(),
        String::new(),
        "Règles : commence par constater l'état réel (project_status) et n'agis que sur l'écart ; ne publie jamais de port ;".into(),
        "n'affiche jamais la valeur d'un secret ; n'invente pas de valeur manquante (termine en échec avec la question précise) ;".into(),
        "ne modifie pas le repo du projet sauf cas prévu par un skill (PR). Termine par un résumé : ce qui a été fait,".into(),
        "les URLs, ce que tu as dû faire à la main (candidats à devenir des tools), ce qu'il reste à un humain.".into(),
        String::new(),
        format!("Clôture obligatoire : termine par UN appel au tool `run_report(project='{name}', status=ok|bloqué|échec, summary, needs_human)`."),
        "`ok` seulement si l'état cible est atteint et vérifié ; `bloqué` s'il manque quelque chose à un humain (rien de cassé) ;".into(),
        "`échec` sinon. Sans cet appel, le run compte comme échec.".into(),
    ]);
    Ok(lines.join("\n"))
}

/// Prompt d'un run sur un service partagé (shared.yaml) : même cadre que les projets, skill `shared-service`.
pub fn build_shared_prompt(name: &str, svc: Option<&Shared>, previous: Option<&Value>) -> Result<String> {
    let h = config::host()?;
    let sk = skills::paths_for_shared(svc);
    let missing: Vec<&String> = sk.iter().filter(|(_, p)| p.is_none()).map(|(r, _)| r).collect();
    if !missing.is_empty() {
        bail!("skills introuvables : {missing:?}");
    }
    let tool = paths::tool_dir();
    let key = config::shared_key(name);
    let after = yaml_entry(name, svc.map(|s| Value::Object(s.dump_non_default())));
    let before = yaml_entry(name, previous.cloned());
    let users = config::users_of(name)?;
    let mut lines: Vec<String> = vec![
        format!("Tu es l'agent de déploiement de l'hôte `{}` (outil `infra`). Répertoire courant : {} (repo de config).", h.name, paths::config_dir().display()),
        format!("Contexte à lire d'abord : {} puis {}.", tool.join("README.md").display(), tool.join("docs/conventions.md").display()),
        String::new(),
        format!("Objectif : amener le SERVICE PARTAGÉ `{name}` (shared.yaml, un seul exemplaire sur l'hôte) à l'état voulu ci-dessous."),
        "Tu as carte blanche sur le chemin ; le skill décrit le but et l'état cible, pas une procédure.".into(),
        String::new(),
        "Entrée de shared.yaml AVANT :".into(), "```yaml".into(), before, "```".into(),
        "Entrée de shared.yaml APRÈS (état voulu) :".into(), "```yaml".into(), after, "```".into(),
        format!("Projets qui le consomment (`uses:`) : {}.", if users.is_empty() { "aucun pour l instant".to_string() } else { users.join(", ") }),
        String::new(),
        "Skills à satisfaire :".into(),
    ];
    lines.extend(skills_lines(&sk));
    lines.extend([
        String::new(),
        "Les choix humains (kind, listen, port, auth, data) sont fixés : tu ne les changes pas ; s'ils sont incohérents ou".into(),
        "incomplets (ex. `listen: [lan]` sans `lan_ip` dans host.yaml), termine `bloqué` avec la question précise.".into(),
        "Le *comment* (image, container, compose_file) est à toi : écris-le avec shared_set.".into(),
        String::new(),
        "Tu disposes du serveur MCP `infra` (shared_get/shared_set/shared_status, host_addresses, host_info, registry_get,".into(),
        "secrets_keys, stack_status…) et de Bash (docker compose, ss, git, curl, sudo limité).".into(),
        String::new(),
        "Règles : constate l'état réel d'abord (shared_status) et n'agis que sur l'écart ; n'écoute jamais sur 0.0.0.0 ;".into(),
        "n'affiche jamais la valeur d'un secret ; n'invente pas de valeur manquante. Termine par un résumé : fait, vérifié,".into(),
        "fait à la main (candidats à devenir des tools), reste à un humain.".into(),
        String::new(),
        format!("Clôture obligatoire : termine par UN appel au tool `run_report(project='{key}', status=ok|bloqué|échec, summary, needs_human)`."),
        "`ok` seulement si l'état cible est atteint et vérifié ; `bloqué` s'il manque quelque chose à un humain (rien de cassé) ;".into(),
        "`échec` sinon. Sans cet appel, le run compte comme échec.".into(),
    ]);
    Ok(lines.join("\n"))
}

/// Rapport déposé par le tool MCP `run_report` pendant le run. Absent ou pour un autre projet → statut `inconnu`.
pub fn read_report(name: &str) -> Value {
    let unknown = json!({"status": "inconnu"});
    let Ok(text) = fs::read_to_string(tools::report_path()) else { return unknown };
    let Ok(rep) = serde_json::from_str::<Value>(&text) else { return unknown };
    let st = rep.get("status").and_then(Value::as_str).unwrap_or("");
    if rep.get("project").and_then(Value::as_str) != Some(name) || !matches!(st, "ok" | "bloqué" | "échec") {
        return unknown;
    }
    rep
}

pub fn claude_bin() -> Result<PathBuf> {
    which("claude").ok_or_else(|| anyhow!("`claude` introuvable dans le PATH (npm i -g @anthropic-ai/claude-code)"))
}

pub fn run_project(name: &str, prj: Option<&Project>, previous: Option<&Value>, dry: bool) -> Result<Value> {
    let prompt = build_prompt(name, prj, previous)?;
    run_inner(name, &prompt, skills::for_project(prj), previous, prj.map(|p| Value::Object(p.dump())), dry)
}

/// Run sur un service partagé : enregistré sous la clé `shared.<nom>` (runs, état, historique).
pub fn run_shared(name: &str, svc: Option<&Shared>, previous: Option<&Value>, dry: bool) -> Result<Value> {
    let prompt = build_shared_prompt(name, svc, previous)?;
    run_inner(&config::shared_key(name), &prompt, skills::for_shared(svc), previous, svc.map(|s| Value::Object(s.dump())), dry)
}

fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n { s.to_string() } else { s.chars().skip(count - n).collect() }
}

fn run_inner(name: &str, prompt: &str, skill_list: Vec<String>, previous: Option<&Value>, after: Option<Value>, dry: bool) -> Result<Value> {
    let started = chrono::Utc::now();
    let mut rec = json!({"project": name, "started": crate::util::iso(started), "skills": skill_list,
                         "before": previous.cloned().unwrap_or(Value::Null), "after": after.unwrap_or(Value::Null), "prompt": prompt});
    if dry {
        rec["dry"] = json!(true);
        rec["ok"] = json!(true);
        return Ok(rec);
    }
    let llm = config::host()?.llm;
    // id de session choisi ici (pas lu dans la sortie) et nom lisible : le front ouvre cette session dans l'éditeur
    let session_id = uuid::Uuid::new_v4().to_string();
    rec["model"] = json!(llm.model);
    rec["effort"] = json!(llm.effort);
    rec["session_id"] = json!(session_id);
    let ienv = secrets::infra_env();
    // git/gh de Claude : pas de prompt, et GITHUB_TOKEN (repos privés, PR) si présent — jamais écrit sur disque
    let mut env = compose::git_env();
    if let Some(t) = ienv.get("CLAUDE_CODE_OAUTH_TOKEN") {
        if std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_none() {
            env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), t.clone());
        }
    }
    let mcp_cfg = paths::runs_dir().join(".mcp.json");
    fs::create_dir_all(paths::runs_dir())?;
    let infra_bin = which("infra").map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "infra".into());
    fs::write(
        &mcp_cfg,
        serde_json::to_string(&json!({"mcpServers": {"infra": {"command": infra_bin, "args": ["mcp"], "env": {"INFRA_CONFIG_DIR": paths::config_dir().to_string_lossy()}}}}))?,
    )?;
    let local = started.with_timezone(&chrono::Local);
    let run_name = format!("{name} · {}", local.format("%d/%m %H:%M"));
    let _ = fs::remove_file(tools::report_path()); // jamais réutiliser le rapport d'un run précédent
    // stream-json + --verbose : on garde TOUT le déroulé (tool calls, textes, résultat) dans runs/<run>.log
    let out = Command::new(claude_bin()?)
        .args(["-p", prompt, "--output-format", "stream-json", "--verbose", "--model", &llm.model, "--effort", &llm.effort])
        .args(["--allowedTools", ALLOWED_TOOLS, "--permission-mode", "acceptEdits", "--mcp-config"])
        .arg(&mcp_cfg)
        .args(["--strict-mcp-config", "--session-id", &session_id, "--name", &run_name])
        .current_dir(paths::config_dir())
        .envs(&env)
        .output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let secret_values: Vec<String> = ienv.values().cloned().collect();
    let result_ev = stdout.lines().rev().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|ev| ev.get("type").and_then(Value::as_str) == Some("result"));
    let result_text = match &result_ev {
        Some(ev) => {
            rec["cost_usd"] = ev.get("total_cost_usd").cloned().unwrap_or(Value::Null);
            rec["duration_ms"] = ev.get("duration_ms").cloned().unwrap_or(Value::Null);
            rec["num_turns"] = ev.get("num_turns").cloned().unwrap_or(Value::Null);
            rec["is_error"] = json!(ev.get("is_error").and_then(Value::as_bool).unwrap_or(false));
            ev.get("result").and_then(Value::as_str).unwrap_or("").to_string()
        }
        None => {
            rec["is_error"] = json!(true);
            tail(&stdout, 4000)
        }
    };
    rec["result"] = json!(secrets::mask(&result_text, &secret_values));
    rec["stderr"] = json!(secrets::mask(&tail(&stderr, 4000), &secret_values));
    let rep = read_report(name);
    let status = rep["status"].as_str().unwrap_or("inconnu").to_string();
    rec["status"] = json!(status);
    rec["needs_human"] = json!(secrets::mask(rep.get("needs_human").and_then(Value::as_str).unwrap_or(""), &secret_values));
    rec["report"] = json!(secrets::mask(rep.get("summary").and_then(Value::as_str).unwrap_or(""), &secret_values));
    // claude -p sort 0 même quand il s'est déclaré bloqué : c'est son appel à run_report qui fait foi
    rec["ok"] = json!(out.status.success() && !rec["is_error"].as_bool().unwrap_or(false) && status == "ok");
    rec["finished"] = json!(now_iso());
    let p = save(&rec)?;
    let mut log = secrets::mask(&stdout, &secret_values);
    if !stderr.is_empty() {
        log.push_str("\n\n--- stderr ---\n");
        log.push_str(&secrets::mask(&stderr, &secret_values));
    }
    fs::write(p.with_extension("log"), log)?;
    Ok(rec)
}

pub fn save(rec: &Value) -> Result<PathBuf> {
    let d = paths::runs_dir();
    fs::create_dir_all(&d)?;
    let started = rec["started"].as_str().unwrap_or("").replace(':', "").replace("+00:00", "Z");
    let p = d.join(format!("{started}-{}.json", rec["project"].as_str().unwrap_or("")));
    fs::write(&p, serde_json::to_string_pretty(rec)?)?;
    Ok(p)
}

fn run_files(filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let d = paths::runs_dir();
    let Ok(rd) = fs::read_dir(&d) else { return vec![] };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            !n.starts_with('.') && n.ends_with(".json") && filter(&n)
        })
        .collect();
    v.sort();
    v
}

fn summary(r: &Value, p: &Path) -> Value {
    let mut m = Map::new();
    for k in ["project", "started", "finished", "ok", "status", "skills", "cost_usd", "duration_ms", "num_turns", "dry", "model", "effort"] {
        m.insert(k.into(), r.get(k).cloned().unwrap_or(Value::Null));
    }
    m.insert("file".into(), json!(p.file_name().unwrap().to_string_lossy()));
    let log = p.with_extension("log");
    m.insert("log".into(), if log.exists() { json!(log.file_name().unwrap().to_string_lossy()) } else { Value::Null });
    Value::Object(m)
}

fn read_json(p: &Path) -> Result<Value> {
    Ok(serde_json::from_str(&fs::read_to_string(p)?)?)
}

pub fn list_runs(limit: usize) -> Result<Vec<Value>> {
    let mut files = run_files(|_| true);
    files.reverse();
    files.into_iter().take(limit).map(|p| Ok(summary(&read_json(&p)?, &p))).collect()
}

pub fn runs_for_project(name: &str) -> Result<Vec<Value>> {
    let suffix = format!("-{name}.json");
    run_files(|n| n.ends_with(&suffix)).into_iter().map(|p| Ok(summary(&read_json(&p)?, &p))).collect()
}

fn safe_file(file: &str) -> Result<PathBuf> {
    if file.contains('/') || file.starts_with('.') {
        bail!("nom de fichier invalide");
    }
    Ok(paths::runs_dir().join(file))
}

pub fn get_run(file: &str) -> Result<Value> {
    read_json(&safe_file(file)?)
}

/// Session Claude d'un run : enregistrée, sinon (runs antérieurs) lue dans le premier événement de son .log.
pub fn session_of(file: &str) -> Option<String> {
    let run = get_run(file).ok()?;
    if let Some(sid) = run.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        return Some(sid.to_string());
    }
    let log = fs::read_to_string(safe_file(file).ok()?.with_extension("log")).ok()?;
    let first: Value = serde_json::from_str(log.lines().next()?).ok()?;
    first.get("session_id").and_then(Value::as_str).map(str::to_string)
}

pub fn get_log(file: &str) -> String {
    safe_file(file).ok().and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default()
}

/// Exécute la plus ancienne demande de la file. Retourne le run, ou None si la file est vide.
pub fn process_one(dry: bool) -> Result<Option<Value>> {
    let files = state::queue_files()?;
    let Some(req) = files.first() else { return Ok(None) };
    let name = req.file_stem().unwrap().to_string_lossy().into_owned();
    let shared_name = name.strip_prefix("shared.").map(str::to_string);
    let mut st = state::load_state();
    let previous = st.get(&name).and_then(|s| s.get("entry")).filter(|v| !v.is_null()).cloned();
    let entry: Option<Entry> = match &shared_name {
        Some(sn) => config::shared()?.services.get(sn).cloned().map(Entry::Shared),
        None => config::registry()?.projects.get(&name).cloned().map(Entry::Project),
    };
    let skill_list = match &entry {
        Some(Entry::Shared(s)) => skills::for_shared(Some(s)),
        None if shared_name.is_some() => skills::for_shared(None),
        Some(Entry::Project(p)) => skills::for_project(Some(p)),
        None => vec![],
    };
    state::set_running(Some(&name))?;
    let res = match (&shared_name, &entry) {
        (Some(sn), Some(Entry::Shared(s))) => run_shared(sn, Some(s), previous.as_ref(), dry),
        (Some(sn), _) => run_shared(sn, None, previous.as_ref(), dry),
        (None, Some(Entry::Project(p))) => run_project(&name, Some(p), previous.as_ref(), dry),
        (None, _) => run_project(&name, None, previous.as_ref(), dry),
    };
    let mut rec = match res {
        Ok(r) => r,
        Err(e) => {
            // un run qui plante ne doit pas tuer le service
            let r = json!({"project": name, "started": now_iso(), "ok": false, "result": format!("erreur du runner : {e}"), "skills": skill_list});
            let _ = save(&r);
            r
        }
    };
    state::set_running(None)?;
    let _ = fs::remove_file(req);
    if !dry {
        // relu après le run : le LLM a pu écrire ses constats (service, container, port, image…)
        let after: Option<Entry> = match (&entry, &shared_name) {
            (None, _) => None,
            (Some(_), Some(sn)) => config::shared()?.services.get(sn).cloned().map(Entry::Shared),
            (Some(_), None) => config::registry()?.projects.get(&name).cloned().map(Entry::Project),
        };
        let ok = rec["ok"].as_bool().unwrap_or(false);
        let status = rec.get("status").and_then(Value::as_str).map(str::to_string);
        state::mark_applied(&mut st, &name, after.as_ref(), ok, status.as_deref())?;
        // la config écrite pendant le run (constats, shared/<S>/, host.yaml…) est versionnée tout de suite
        let report: String = rec.get("report").and_then(Value::as_str).unwrap_or("").chars().take(72).collect();
        let msg = format!("run {name} : {} — {report}", status.clone().unwrap_or_else(|| "erreur".into()));
        rec["git"] = gitsync::commit_push(msg.trim_end_matches([' ', '—']));
        if rec.get("started").is_some() && rec.get("skills").is_some_and(|s| !s.is_null()) {
            save(&rec)?;
        }
    }
    Ok(Some(rec))
}

/// Boucle du service infra-runner : consomme la file, un projet à la fois.
pub fn watch(interval: f64, once: bool, dry: bool) -> Result<()> {
    eprintln!("runner : file {} (toutes les {interval}s)", state::queue_dir()?.display());
    loop {
        let rec = process_one(dry)?;
        if let Some(r) = &rec {
            let st = r.get("status").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| if r["ok"].as_bool().unwrap_or(false) { "ok".into() } else { "échec".into() });
            eprintln!("run {} : {st}", r["project"].as_str().unwrap_or(""));
        }
        if once {
            return Ok(());
        }
        if rec.is_none() {
            std::thread::sleep(std::time::Duration::from_secs_f64(interval));
        }
    }
}

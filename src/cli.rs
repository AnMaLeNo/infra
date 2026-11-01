//! CLI `infra` : les opérations mécaniques et idempotentes. Voir skills/ pour l'usage.
use std::io::BufRead;

use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::render::{missing_env_vars, render, Target};
use crate::{agents, compose, config, dns, doctor, editor, gitsync, history, previews, runner, secrets, skills, stack, state, tunnel, VERSION};

#[derive(Parser)]
#[command(name = "infra", version = VERSION, about = "Déploiement déclaratif piloté par registry.yaml.", arg_required_else_help = true)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Valide host.yaml et registry.yaml, et la présence des skills.
    Validate,
    /// État réel d'un projet (ou de tous) : containers, ports publiés, routes.
    Status {
        name: Option<String>,
        #[arg(long = "json")]
        as_json: bool,
    },
    /// Clone / met à jour le repo du projet sur `ref`.
    Fetch { name: String },
    /// Génère compose.override.yml et .env (déchiffré). Ne lance rien.
    Render { name: String },
    /// docker compose up -d --build avec l'override généré (aide humaine ; le LLM lance docker compose lui-même).
    Up { name: String },
    /// Arrête le projet (données conservées sauf --volumes).
    Down {
        name: String,
        /// supprime aussi les volumes nommés
        #[arg(long)]
        volumes: bool,
    },
    /// Modifie une entrée de registry.yaml (ex. infra set catacombes port=8420 container=web).
    Set {
        name: String,
        /// cle=valeur …  (valeur vide = supprimer)
        #[arg(required = true)]
        fields: Vec<String>,
    },
    /// Supprime containers, volumes, clone, .env et /srv/data/<P>. Humain uniquement.
    Purge {
        name: String,
        #[arg(long)]
        yes: bool,
    },
    /// Lance immédiatement un run LLM (claude -p) pour un projet, avec ses skills (sans passer par la file).
    Run {
        name: String,
        /// affiche le prompt sans lancer claude
        #[arg(long)]
        dry: bool,
    },
    /// Service infra-runner : exécute les runs demandés (front / `infra enqueue`), un à la fois. Rien d'automatique.
    Watch {
        #[arg(long, default_value_t = 3.0)]
        interval: f64,
        #[arg(long)]
        once: bool,
        #[arg(long)]
        dry: bool,
    },
    /// Service infra-editor : VS Code web (`code serve-web`) sur le dossier des runs, pour reprendre la session Claude d'un run.
    Editor,
    /// Demande un run pour un projet (équivalent du bouton « Lancer Claude » du front).
    Enqueue { name: String },
    /// Projets modifiés depuis leur dernier run, demandes en attente, run en cours.
    Plan,
    /// Recalcule les empreintes « modifié » de runs/.state.json depuis l'entrée du dernier run et le host.yaml
    /// courant (à lancer après une mise à jour qui change la formule du hash, sinon tout passe orange).
    Rehash,
    /// Derniers runs LLM.
    Runs {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Historique d'un projet : versions de sa config + ses runs.
    History { name: String },
    /// Affiche / règle le modèle et l'effort de claude -p pour tous les runs (host.yaml → llm).
    Llm {
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
    },
    /// Skills disponibles, ou ceux qu'un projet utilise.
    Skills { name: Option<String> },
    /// Serveur MCP (stdio) : les tools atomiques de l'outil, pour Claude Code (branché par le runner).
    Mcp,
    /// API + UI web.
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
    /// Vérifie l'hôte : docker, tailscale, traefik, tunnel, dns, age, runner.
    Doctor {
        /// liste aussi ce qui tourne hors modèle
        #[arg(long)]
        legacy: bool,
    },
    /// Enregistrements DNS Cloudflare
    #[command(subcommand)]
    Dns(DnsCmd),
    /// Stacks d'infra (traefik, cloudflared, ui…)
    #[command(subcommand)]
    Stack(StackCmd),
    /// Cloudflare Tunnel
    #[command(subcommand)]
    Tunnel(TunnelCmd),
    /// Secrets sops+age
    #[command(subcommand)]
    Secrets(SecretsCmd),
    /// Previews par PR
    #[command(subcommand)]
    Preview(PreviewCmd),
    /// Comptes agents IA
    #[command(subcommand)]
    Agents(AgentsCmd),
}

#[derive(Subcommand)]
pub enum DnsCmd {
    List,
    /// Crée les enregistrements voulus pour un projet (CNAME public, wildcards previews).
    Ensure {
        name: String,
        /// écrase un enregistrement existant divergent
        #[arg(long)]
        force: bool,
    },
    /// Supprime les enregistrements d'un projet.
    Prune {
        name: String,
        /// ne supprime que le CNAME public
        #[arg(long = "public")]
        public_only: bool,
    },
    /// A *.<private_zone> → tailscale_ip.
    Bootstrap,
    Check,
}

#[derive(Subcommand)]
pub enum StackCmd {
    Up {
        name: Option<String>,
        #[arg(long)]
        all: bool,
    },
    Down { name: String },
    Status { name: Option<String> },
}

#[derive(Subcommand)]
pub enum TunnelCmd {
    /// Crée le tunnel (API), chiffre les credentials, écrit tunnel_id dans host.yaml.
    Create {
        #[arg(long)]
        name: Option<String>,
    },
    Info,
}

#[derive(Subcommand)]
pub enum SecretsCmd {
    Edit { rel: String },
    /// infra secrets set projects/x.env CLE=valeur  (ou CLE seule : valeur lue sur stdin).
    Set {
        rel: String,
        #[arg(required = true)]
        pairs: Vec<String>,
    },
    Keys { rel: String },
    Check,
    Rekey,
}

#[derive(Args)]
pub struct PreviewRef {
    pub name: String,
    pub pr: u64,
    /// n'affiche que l'URL
    #[arg(long)]
    pub url: bool,
}

#[derive(Subcommand)]
pub enum PreviewCmd {
    Up(PreviewRef),
    Down(PreviewRef),
    List { name: String },
    Prune {
        name: String,
        /// toutes, pas seulement les PR fermées
        #[arg(long)]
        all: bool,
    },
    /// Affiche le workflow à ajouter dans le repo du projet et les secrets GitHub à créer.
    Install { name: String },
}

#[derive(Subcommand)]
pub enum AgentsCmd {
    Sync {
        /// verrouille les comptes ai-* sans clé
        #[arg(long)]
        prune: bool,
    },
    Retire {
        #[arg(required = true)]
        names: Vec<String>,
    },
}

pub fn yaml<T: Serialize>(v: &T) -> String {
    let j = serde_json::to_value(v).unwrap_or(Value::Null);
    let y: serde_yaml::Value = serde_json::from_value(j).unwrap_or(serde_yaml::Value::Null);
    serde_yaml::to_string(&y).unwrap_or_default().trim_end().to_string()
}

fn out<T: Serialize>(v: &T) {
    println!("{}", yaml(v));
}

fn or_text<T: Serialize>(v: &T, empty: bool, text: &str) -> Value {
    if empty { json!(text) } else { serde_json::to_value(v).unwrap_or(Value::Null) }
}

fn target(name: &str, pr: Option<u64>) -> Result<Target> {
    Ok(Target::new(name, config::project(name)?, config::host()?, pr))
}

/// `cle=valeur` du CLI → JSON : vide = null, true/false, entier, [..]/{..} en YAML, sinon chaîne.
pub fn parse_kv(fields: &[String]) -> Map<String, Value> {
    let mut kv = Map::new();
    for f in fields {
        let (k, v) = f.split_once('=').unwrap_or((f.as_str(), ""));
        let val = if v.is_empty() {
            Value::Null
        } else if v == "true" || v == "false" {
            json!(v == "true")
        } else if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) {
            v.parse::<i64>().map(Value::from).unwrap_or_else(|_| json!(v))
        } else if v.starts_with('[') || v.starts_with('{') {
            serde_yaml::from_str::<serde_yaml::Value>(v).ok().and_then(|y| serde_json::to_value(y).ok()).unwrap_or_else(|| json!(v))
        } else {
            json!(v)
        };
        kv.insert(k.to_string(), val);
    }
    kv
}

/// Exécute la commande ; retourne le code de sortie.
pub fn run(cli: Cli) -> Result<i32> {
    match cli.cmd {
        Command::Validate => {
            let h = config::host()?;
            let reg = config::registry()?;
            let mut problems = vec![];
            for (name, prj) in &reg.projects {
                for (rel, p) in skills::paths_for(Some(prj)) {
                    if p.is_none() {
                        problems.push(format!("{name}: skill {rel} introuvable"));
                    }
                }
            }
            let bad = !problems.is_empty();
            out(&json!({"host": h.name, "projects": reg.projects.len(), "problems": or_text(&problems, problems.is_empty(), "aucun")}));
            Ok(if bad { 1 } else { 0 })
        }
        Command::Status { name, as_json } => {
            let reg = config::registry()?;
            let h = config::host()?;
            let names: Vec<String> = match name { Some(n) => vec![n], None => reg.projects.keys().cloned().collect() };
            let mut res = Map::new();
            for n in names {
                let t = Target::new(&n, config::project(&n)?, h.clone(), None);
                let mut s = compose::status(&t)?;
                s["wanted"] = json!({"enabled": t.project.enabled, "expose": t.project.expose, "previews": t.project.previews});
                res.insert(n, s);
            }
            if as_json { println!("{}", serde_json::to_string_pretty(&res)?) } else { out(&res) }
            Ok(0)
        }
        Command::Fetch { name } => {
            println!("{}", compose::fetch(&target(&name, None)?)?);
            Ok(0)
        }
        Command::Render { name } => {
            let t = target(&name, None)?;
            let changed = render(&t)?;
            let missing = missing_env_vars(&t)?;
            out(&json!({"override": t.override_file().to_string_lossy(), "changed": changed, "missing_env": or_text(&missing, missing.is_empty(), "aucune")}));
            Ok(if missing.is_empty() { 0 } else { 2 })
        }
        Command::Up { name } => {
            let t = target(&name, None)?;
            compose::up(&t)?;
            out(&compose::status(&t)?);
            Ok(0)
        }
        Command::Down { name, volumes } => {
            compose::down(&target(&name, None)?, volumes)?;
            Ok(0)
        }
        Command::Set { name, fields } => {
            let kv = parse_kv(&fields);
            let prj = config::set_project_fields(&name, "cli", &kv)?;
            let git = gitsync::commit_push(&format!("cli : projet {name} modifié"));
            out(&json!({name: prj.dump_non_default(), "git": git}));
            Ok(0)
        }
        Command::Purge { name, yes } => {
            if !yes {
                eprint!("Supprimer DÉFINITIVEMENT données et clone de {name} ? [y/N] ");
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes" | "o" | "oui") {
                    eprintln!("abandon");
                    return Ok(1);
                }
            }
            let t = target(&name, None)?;
            compose::down(&t, true)?;
            let _ = std::fs::remove_dir_all(t.dir());
            let _ = std::fs::remove_dir_all(crate::paths::data_dir().join(&name));
            println!("purgé");
            Ok(0)
        }
        Command::Run { name, dry } => {
            let reg = config::registry()?;
            let prj = reg.projects.get(&name).cloned();
            let mut st = state::load_state();
            let previous = st.get(&name).and_then(|s| s.get("entry")).filter(|v| !v.is_null()).cloned();
            let rec = runner::run_project(&name, prj.as_ref(), previous.as_ref(), dry)?;
            if dry {
                println!("{}", rec["prompt"].as_str().unwrap_or(""));
                return Ok(0);
            }
            let ok = rec["ok"].as_bool().unwrap_or(false);
            let entry = prj.map(state::Entry::Project);
            state::mark_applied(&mut st, &name, entry.as_ref(), ok, rec.get("status").and_then(Value::as_str))?;
            println!("{}", rec["result"].as_str().unwrap_or(""));
            Ok(if ok { 0 } else { 1 })
        }
        Command::Watch { interval, once, dry } => {
            runner::watch(interval, once, dry)?;
            Ok(0)
        }
        Command::Editor => {
            editor::serve()?;
            Ok(0)
        }
        Command::Enqueue { name } => {
            if config::registry()?.projects.contains_key(&name) {
                config::project(&name)?;
            }
            out(&state::enqueue(&name)?);
            Ok(0)
        }
        Command::Plan => {
            let changed = state::changed_since_last_run(&state::load_state())?;
            let queued = state::queued()?;
            let running = state::running();
            out(&json!({
                "modifiés": or_text(&changed, changed.is_empty(), "aucun"),
                "en attente": or_text(&queued, queued.is_empty(), "aucune"),
                "en cours": running.map(Value::String).unwrap_or(json!("aucun")),
            }));
            Ok(0)
        }
        Command::Rehash => {
            let mut st = state::load_state();
            let done = state::rehash(&mut st)?;
            out(&json!({"recalculés": done, "modifiés": state::changed_since_last_run(&st)?}));
            Ok(0)
        }
        Command::Runs { limit } => {
            out(&runner::list_runs(limit)?);
            Ok(0)
        }
        Command::History { name } => {
            out(&json!({"config": history::for_project(&name)?, "runs": runner::runs_for_project(&name)?}));
            Ok(0)
        }
        Command::Llm { model, effort } => {
            if model.is_some() || effort.is_some() {
                let mut f = Map::new();
                if let Some(m) = model { f.insert("model".into(), json!(m)); }
                if let Some(e) = effort { f.insert("effort".into(), json!(e)); }
                let llm = config::set_llm(&f)?;
                let mut m = crate::models::to_map(&llm);
                m.insert("git".into(), gitsync::commit_push("cli : settings LLM"));
                out(&m);
            } else {
                out(&config::host()?.llm);
            }
            Ok(0)
        }
        Command::Skills { name } => {
            match name {
                Some(n) => {
                    let m: IndexMap<String, String> = skills::paths_for(Some(&config::project(&n)?))
                        .into_iter()
                        .map(|(k, v)| (k, v.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "None".into())))
                        .collect();
                    out(&m);
                }
                None => out(&skills::list_all()),
            }
            Ok(0)
        }
        Command::Mcp => {
            crate::mcp::serve_stdio()?;
            Ok(0)
        }
        Command::Serve { host, port } => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(crate::api::serve(&host, port))?;
            Ok(0)
        }
        Command::Doctor { legacy } => {
            let rep = doctor::doctor(legacy);
            out(&rep);
            let ko = rep.values().any(|v| v.as_str().is_some_and(|s| s.starts_with("KO")));
            Ok(if ko { 1 } else { 0 })
        }
        Command::Dns(d) => {
            let h = config::host()?;
            match d {
                DnsCmd::List => out(&dns::list_records(&h)?),
                DnsCmd::Ensure { name, force } => {
                    let res = dns::ensure_project(&name, &config::project(&name)?, &h, force)?;
                    out(&res);
                    if res.values().any(|v| v == "conflict") {
                        eprintln!("conflit : un enregistrement existe avec une autre cible ; vérifie puis --force");
                        return Ok(2);
                    }
                }
                DnsCmd::Prune { name, public_only } => {
                    let res = dns::prune_project(&name, &config::project(&name)?, &h, public_only)?;
                    out(&or_text(&res, res.is_empty(), "rien"));
                }
                DnsCmd::Bootstrap => out(&dns::bootstrap(&h)?),
                DnsCmd::Check => out(&dns::check(&h)?),
            }
            Ok(0)
        }
        Command::Stack(s) => {
            match s {
                StackCmd::Up { name, all } => {
                    let names = if all { stack::all_names()? } else { vec![name.ok_or_else(|| anyhow!("nom de stack requis (ou --all)"))?] };
                    for n in names {
                        stack::up(&n)?;
                        let st = stack::status(&n)?;
                        out(&json!({n: st}));
                    }
                }
                StackCmd::Down { name } => stack::down(&name)?,
                StackCmd::Status { name } => {
                    let names = match name { Some(n) => vec![n], None => stack::all_names()? };
                    let mut m = Map::new();
                    for n in names {
                        m.insert(n.clone(), json!(stack::status(&n)?));
                    }
                    out(&m);
                }
            }
            Ok(0)
        }
        Command::Tunnel(t) => {
            match t {
                TunnelCmd::Create { name } => println!("{}", tunnel::create(name.as_deref())?),
                TunnelCmd::Info => out(&tunnel::info()?),
            }
            Ok(0)
        }
        Command::Secrets(s) => match s {
            SecretsCmd::Edit { rel } => {
                secrets::edit(&rel)?;
                Ok(0)
            }
            SecretsCmd::Set { rel, pairs } => {
                let mut values = IndexMap::new();
                let stdin = std::io::stdin();
                for p in &pairs {
                    match p.split_once('=') {
                        Some((k, v)) => values.insert(k.to_string(), v.to_string()),
                        None => {
                            let mut line = String::new();
                            stdin.lock().read_line(&mut line)?;
                            values.insert(p.clone(), line.trim_end_matches('\n').to_string())
                        }
                    };
                }
                secrets::set_values(&rel, &values)?;
                println!("{} variable(s) écrite(s) dans secrets/{rel}", values.len());
                Ok(0)
            }
            SecretsCmd::Keys { rel } => {
                out(&secrets::keys(&rel)?);
                Ok(0)
            }
            SecretsCmd::Check => {
                let res = secrets::check();
                out(&or_text(&res, res.is_empty(), "aucun secret"));
                Ok(if res.values().any(|v| v != "ok") { 1 } else { 0 })
            }
            SecretsCmd::Rekey => {
                secrets::rekey()?;
                Ok(0)
            }
        },
        Command::Preview(p) => {
            match p {
                PreviewCmd::Up(r) => {
                    if r.url {
                        crate::util::STDOUT_RESERVED.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let res = previews::up(&target(&r.name, Some(r.pr))?)?;
                    if r.url { println!("{}", res["url"].as_str().unwrap_or("")) } else { out(&res) }
                }
                PreviewCmd::Down(r) => {
                    if r.url {
                        crate::util::STDOUT_RESERVED.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    previews::down(&target(&r.name, Some(r.pr))?)?;
                    println!("{}", if r.url { "" } else { "supprimée" });
                }
                PreviewCmd::List { name } => {
                    let l = previews::list_previews(&name)?;
                    out(&or_text(&l, l.is_empty(), "aucune"));
                }
                PreviewCmd::Prune { name, all } => {
                    let r = previews::prune(&name, all)?;
                    out(&or_text(&r, r.is_empty(), "rien"));
                }
                PreviewCmd::Install { name } => println!("{}", previews::workflow_snippet(&name)?),
            }
            Ok(0)
        }
        Command::Agents(a) => {
            match a {
                AgentsCmd::Sync { prune } => out(&agents::sync(prune)?),
                AgentsCmd::Retire { names } => out(&agents::retire(&names)?),
            }
            Ok(0)
        }
    }
}

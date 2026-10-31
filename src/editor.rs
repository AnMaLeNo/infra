//! Éditeur web (VS Code de Microsoft, `code serve-web`) pour reprendre la session Claude d'un run.
//!
//! Il tourne sous le même utilisateur (ai-infra) et ouvre le même dossier (config_dir) que les runs : leurs sessions
//! sont donc dans l'historique de l'extension Claude Code, installée au démarrage avec une petite extension à nous
//! (`editor_ext/`, embarquée dans le binaire) qui ouvre la session demandée par le front. Même token Claude et même
//! accès git que le runner. Il n'écoute que sur l'IP de l'hôte vue depuis traefik-net : Traefik le sert en privé
//! (`editor.<private_zone>`, tailnet), jamais en public. Accès protégé par un token de connexion (fichier 0600,
//! généré au premier lancement) : ouvrir une fois `…/?tkn=<token>`.
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Result};
use rust_embed::Embed;
use serde_json::{json, Value};

use crate::util::which;
use crate::{compose, paths, secrets, stack};

pub const PORT: u16 = 8443;
pub const CLI: &str = "vscode-cli";
pub const EXTENSION: &str = "anthropic.claude-code";
/// L'extension n'affiche dans « Session history » les sessions lancées par `claude -p` que si `claudeProcessWrapper` est
/// défini (sinon elle ne montre que celles créées dans VS Code). `env` exécute simplement le binaire embarqué qu'on lui passe.
pub const SETTINGS: [(&str, &str); 1] = [("claudeCode.claudeProcessWrapper", "/usr/bin/env")];
/// Petite extension à nous (editor_ext/) : au démarrage de VS Code, ouvre la session déposée par `request_session`.
pub const OPEN_FILE: &str = ".editor-open";

#[derive(Embed)]
#[folder = "editor_ext/"]
struct EditorExt;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/root"))
}

/// Fixé explicitement : le serveur VS Code téléchargé et sa CLI (l'emplacement par défaut dépend de la version).
pub fn data_dir() -> PathBuf {
    home().join(".local/share/infra-editor")
}

pub fn server_dir() -> PathBuf {
    data_dir().join("server")
}

pub fn configure() -> Result<()> {
    let f = server_dir().join("data/Machine/settings.json");
    fs::create_dir_all(f.parent().unwrap())?;
    let mut cur: Value = fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}));
    if !cur.is_object() {
        cur = json!({});
    }
    for (k, v) in SETTINGS {
        cur[k] = json!(v);
    }
    fs::write(&f, serde_json::to_string_pretty(&cur)?)?;
    Ok(())
}

/// Demande à l'éditeur d'ouvrir cette session Claude au prochain démarrage d'une fenêtre (bouton du front).
pub fn request_session(session_id: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(session_id).map_err(|_| anyhow!("session_id invalide"))?;
    let f = paths::runs_dir().join(OPEN_FILE);
    fs::create_dir_all(f.parent().unwrap())?;
    fs::write(&f, id.hyphenated().to_string())?;
    Ok(())
}

pub fn build_vsix() -> Result<PathBuf> {
    let out = data_dir().join("infra-open-session.vsix");
    fs::create_dir_all(out.parent().unwrap())?;
    let file = fs::File::create(&out)?;
    let mut z = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut names: Vec<_> = EditorExt::iter().map(|c| c.to_string()).collect();
    names.sort();
    for name in names {
        let data = EditorExt::get(&name).unwrap().data;
        z.start_file(&name, opts)?;
        z.write_all(&data)?;
    }
    z.finish()?;
    Ok(out)
}

pub fn token_file() -> Result<PathBuf> {
    let f = home().join(".config/infra-editor/token");
    if !f.exists() {
        fs::create_dir_all(f.parent().unwrap())?;
        let mut buf = [0u8; 24];
        std::io::Read::read_exact(&mut fs::File::open("/dev/urandom")?, &mut buf)?;
        fs::write(&f, hex::encode(buf))?;
        fs::set_permissions(&f, fs::Permissions::from_mode(0o600))?;
    }
    Ok(f)
}

pub fn command(host: &str) -> Result<(Vec<String>, HashMap<String, String>)> {
    let cli = which(CLI).ok_or_else(|| anyhow!("`{CLI}` introuvable dans le PATH (CLI VS Code standalone, voir install.sh)"))?;
    command_with(&cli, host)
}

pub fn command_with(cli: &Path, host: &str) -> Result<(Vec<String>, HashMap<String, String>)> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    env.extend(compose::git_env());
    if let Some(t) = secrets::infra_env().get("CLAUDE_CODE_OAUTH_TOKEN") {
        env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), t.clone());
    }
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    let argv = vec![
        s(cli.to_path_buf()), "serve-web".into(), "--host".into(), host.into(), "--port".into(), PORT.to_string(),
        "--connection-token-file".into(), s(token_file()?), "--cli-data-dir".into(), s(data_dir()),
        "--server-data-dir".into(), s(server_dir()), "--default-folder".into(), s(paths::config_dir()),
        "--accept-server-license-terms".into(), "--disable-telemetry".into(),
    ];
    Ok((argv, env))
}

fn code_servers() -> Vec<PathBuf> {
    let base = data_dir().join("serve-web");
    let Ok(rd) = fs::read_dir(&base) else { return vec![] };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|e| e.path().join("bin/code-server"))
        .filter(|p| p.exists())
        .map(|p| (p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    v.sort();
    v.into_iter().map(|(_, p)| p).collect()
}

/// Le serveur VS Code ne se télécharge qu'à la première requête (202 tant que ça dure) : on la fait, puis on
/// installe/met à jour l'extension Claude Code et la nôtre avec sa CLI. Idempotent.
pub fn install_extension(host: &str, env: &HashMap<String, String>, tries: u32) -> Result<bool> {
    let url = format!("http://{host}:{PORT}/?tkn={}", fs::read_to_string(token_file()?)?.trim());
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(5)).redirect(reqwest::redirect::Policy::none()).build()?;
    for _ in 0..tries {
        let ready = client.get(&url).send().map(|r| r.status().as_u16() != 202).unwrap_or(false);
        let servers = code_servers();
        if ready && !servers.is_empty() {
            let srv = servers.last().unwrap();
            let vsix = build_vsix()?;
            let runs: [Vec<String>; 2] = [
                vec!["--install-extension".into(), EXTENSION.into()],
                vec!["--install-extension".into(), vsix.to_string_lossy().into_owned(), "--force".into()], // --force : même version, code à jour
            ];
            let mut all_ok = true;
            for args in runs {
                let st = Command::new(srv).arg("--server-data-dir").arg(server_dir()).args(&args).envs(env).status()?;
                all_ok &= st.success();
            }
            return Ok(all_ok);
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    Ok(false)
}

pub fn serve() -> Result<()> {
    let host = stack::network_gateway()?;
    let (argv, env) = command(&host)?;
    configure()?;
    let mut server = Command::new(&argv[0]).args(&argv[1..]).env_clear().envs(&env).spawn()?;
    let (h2, e2) = (host.clone(), env.clone());
    std::thread::spawn(move || {
        if !install_extension(&h2, &e2, 120).unwrap_or(false) {
            eprintln!("extension non installée");
        }
    });
    let st = server.wait()?;
    if !st.success() {
        bail!("vscode-cli terminé avec {st}");
    }
    Ok(())
}

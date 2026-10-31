//! Secrets chiffrés sops+age dans <config>/secrets/. Jamais de valeur dans les logs.
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use indexmap::IndexMap;

use crate::paths;
use crate::util::{dump_dotenv, parse_dotenv, Cmd};

pub fn secrets_dir() -> PathBuf {
    paths::config_dir().join("secrets")
}

fn env() -> HashMap<String, String> {
    HashMap::from([("SOPS_AGE_KEY_FILE".to_string(), paths::age_key_file().to_string_lossy().into_owned())])
}

fn fmt(rel: &str) -> Vec<&'static str> {
    if rel.ends_with(".env") {
        vec!["--input-type", "dotenv", "--output-type", "dotenv"]
    } else if rel.ends_with(".json") {
        vec!["--input-type", "json", "--output-type", "json"]
    } else {
        vec![]
    }
}

pub fn exists(rel: &str) -> bool {
    secrets_dir().join(rel).exists()
}

pub fn decrypt(rel: &str) -> Result<String> {
    let p = secrets_dir().join(rel);
    if !p.exists() {
        return Err(anyhow!("secret absent : secrets/{rel}"));
    }
    let mut cmd: Vec<String> = vec!["sops".into()];
    cmd.extend(fmt(rel).iter().map(|s| s.to_string()));
    cmd.extend(["-d".to_string(), p.to_string_lossy().into_owned()]);
    let e = env();
    Ok(Cmd::new().env(&e).quiet().run(&cmd)?.stdout)
}

/// Chiffre `plaintext` dans secrets/<rel> (crée ou remplace) selon .sops.yaml du repo de config.
pub fn encrypt_write(rel: &str, plaintext: &str) -> Result<()> {
    let p = secrets_dir().join(rel);
    fs::create_dir_all(p.parent().unwrap())?;
    let tmp = p.with_file_name(format!("{}.plain", p.file_name().unwrap().to_string_lossy()));
    fs::write(&tmp, plaintext)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    // sops -e lit les creation_rules relativement au fichier : on chiffre en place puis on renomme
    let mut cmd: Vec<String> = vec!["sops".into()];
    cmd.extend(fmt(rel).iter().map(|s| s.to_string()));
    cmd.extend([
        "--config".to_string(),
        paths::config_dir().join(".sops.yaml").to_string_lossy().into_owned(),
        "--filename-override".to_string(),
        p.to_string_lossy().into_owned(),
        "-e".to_string(),
        tmp.to_string_lossy().into_owned(),
    ]);
    let e = env();
    let cfg = paths::config_dir();
    let res = Cmd::new().env(&e).quiet().cwd(&cfg).run(&cmd);
    let _ = fs::remove_file(&tmp);
    fs::write(&p, res?.stdout)?;
    Ok(())
}

pub fn dotenv(rel: &str) -> Result<IndexMap<String, String>> {
    Ok(parse_dotenv(&decrypt(rel)?))
}

pub fn infra_env() -> IndexMap<String, String> {
    if exists("infra.env") {
        dotenv("infra.env").unwrap_or_default()
    } else {
        IndexMap::new()
    }
}

pub fn keys(rel: &str) -> Result<Vec<String>> {
    Ok(dotenv(rel)?.keys().cloned().collect())
}

pub fn set_values(rel: &str, values: &IndexMap<String, String>) -> Result<()> {
    let mut env = if exists(rel) { dotenv(rel)? } else { IndexMap::new() };
    for (k, v) in values {
        env.insert(k.clone(), v.clone());
    }
    encrypt_write(rel, &dump_dotenv(&env))
}

pub fn edit(rel: &str) -> Result<()> {
    let p = secrets_dir().join(rel);
    if !p.exists() {
        encrypt_write(rel, if rel.ends_with(".json") { "{}\n" } else { "" })?;
    }
    let mut cmd: Vec<String> = vec!["sops".into()];
    cmd.extend(fmt(rel).iter().map(|s| s.to_string()));
    cmd.push(p.to_string_lossy().into_owned());
    let e = env();
    Cmd::new().env(&e).no_capture().run(&cmd)?;
    Ok(())
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

pub fn rekey() -> Result<()> {
    let mut files = vec![];
    walk(&secrets_dir(), &mut files);
    let e = env();
    let cfg = paths::config_dir();
    for p in files {
        Cmd::new().env(&e).cwd(&cfg).run(&["sops", "updatekeys", "-y", &p.to_string_lossy()])?;
    }
    Ok(())
}

/// Retourne {fichier: 'ok'|'erreur ...'} pour tous les secrets.
pub fn check() -> IndexMap<String, String> {
    let mut files = vec![];
    walk(&secrets_dir(), &mut files);
    files.sort();
    let mut out = IndexMap::new();
    for p in files {
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        let rel = p.strip_prefix(secrets_dir()).unwrap().to_string_lossy().into_owned();
        let v = match decrypt(&rel) {
            Ok(_) => "ok".to_string(),
            Err(e) => format!("erreur : {}", e.to_string().lines().last().unwrap_or("")),
        };
        out.insert(rel, v);
    }
    out
}

pub fn project_env_path(project: &str, preview: bool) -> String {
    if preview {
        format!("projects/{project}.preview.env")
    } else {
        format!("projects/{project}.env")
    }
}

/// Remplace chaque valeur secrète (4 caractères ou plus, la plus longue d'abord) par ***.
pub fn mask(text: &str, values: &[String]) -> String {
    let mut vals: Vec<&String> = values.iter().filter(|v| v.len() >= 4).collect();
    vals.sort_by_key(|v| std::cmp::Reverse(v.len()));
    vals.dedup();
    let mut t = text.to_string();
    for v in vals {
        t = t.replace(v.as_str(), "***");
    }
    t
}

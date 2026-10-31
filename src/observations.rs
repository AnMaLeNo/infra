//! Observations laissées par Claude pour l'humain sur une cible (projet ou `shared.<S>`), hors statut du run :
//! un constat utile mais hors périmètre d'infra (client LAN qui ne répond pas, IP sur le Wi-Fi, reste à nettoyer…).
//! Une ligne JSON par observation dans `runs/observations/<clé>.jsonl` ; le front les montre sur la carte de la cible
//! et l'humain les efface quand il les a lues. Le champ `needs_human` d'un `run_report` y est recopié automatiquement.
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::{paths, util};

pub fn dir() -> PathBuf {
    paths::runs_dir().join("observations")
}

fn file(key: &str) -> Result<PathBuf> {
    if key.is_empty() || key.contains('/') || key.starts_with('.') {
        bail!("cible invalide : {key:?}");
    }
    Ok(dir().join(format!("{key}.jsonl")))
}

/// Ajoute une observation ; `run` = nom du fichier de run (sans extension) quand on le connaît.
pub fn add(key: &str, text: &str, source: &str, status: Option<&str>, run: Option<&str>) -> Result<Value> {
    let text = text.trim();
    if text.is_empty() {
        bail!("observation vide");
    }
    let p = file(key)?;
    fs::create_dir_all(p.parent().unwrap())?;
    let obs = json!({"at": util::now_iso(), "text": text, "source": source, "status": status, "run": run});
    let mut line = serde_json::to_string(&obs)?;
    line.push('\n');
    fs::OpenOptions::new().create(true).append(true).open(&p)?.write_all(line.as_bytes())?;
    Ok(obs)
}

/// Observations d'une cible, les plus récentes d'abord.
pub fn list(key: &str) -> Vec<Value> {
    let Ok(p) = file(key) else { return vec![] };
    let Ok(text) = fs::read_to_string(p) else { return vec![] };
    let mut v: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    v.reverse();
    v
}

/// L'humain a lu : on efface (le texte reste dans le run d'origine quand il vient d'un `run_report`).
pub fn clear(key: &str) -> Result<()> {
    let p = file(key)?;
    if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}

use std::io::Write;

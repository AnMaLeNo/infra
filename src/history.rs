//! Historique de la config par projet : runs/history/<P>.jsonl, une ligne par écriture du registre.
//! Avec les runs (runs/<date>-<P>.json), ça donne la vue « de A à Z » d'un projet dans le front.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Value};

use crate::{paths, util};

fn file(name: &str) -> Result<PathBuf> {
    let d = paths::runs_dir().join("history");
    fs::create_dir_all(&d)?;
    Ok(d.join(format!("{name}.jsonl")))
}

/// Ajoute une version de l'entrée (null = retirée du registre). Ignore si identique à la précédente.
pub fn record(name: &str, entry: Option<Value>, source: &str) -> Result<Value> {
    let entry = entry.unwrap_or(Value::Null);
    let prev = for_project(name)?;
    if let Some(last) = prev.last() {
        if last.get("entry") == Some(&entry) {
            return Ok(last.clone());
        }
    }
    let ev = json!({"at": util::now_iso(), "source": source, "entry": entry});
    let mut f = OpenOptions::new().append(true).create(true).open(file(name)?)?;
    writeln!(f, "{}", serde_json::to_string(&ev)?)?;
    Ok(ev)
}

pub fn for_project(name: &str) -> Result<Vec<Value>> {
    let p = file(name)?;
    if !p.exists() {
        return Ok(vec![]);
    }
    let text = fs::read_to_string(p)?;
    Ok(text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect())
}

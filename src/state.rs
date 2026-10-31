//! État des runs (runs/.state.json), empreinte « modifié depuis le dernier run », file d'attente (runs/queue/).
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::models::{project_from_value, shared_from_value};
use crate::{config, paths, util};

/// Champs de l'intention purement informatifs (pour l'humain) : changer une note ne rend pas le projet « modifié ».
pub const HASH_IGNORED: [&str; 1] = ["notes"];

/// Projet ou service partagé : ce que hash et prompt voient de la même façon.
pub enum Entry {
    Project(crate::models::Project),
    Shared(crate::models::Shared),
}

impl Entry {
    pub fn intent(&self) -> Map<String, Value> {
        match self {
            Entry::Project(p) => p.intent(),
            Entry::Shared(s) => s.intent(),
        }
    }
    pub fn dump(&self) -> Map<String, Value> {
        match self {
            Entry::Project(p) => p.dump(),
            Entry::Shared(s) => s.dump(),
        }
    }
    pub fn dump_non_default(&self) -> Map<String, Value> {
        match self {
            Entry::Project(p) => p.dump_non_default(),
            Entry::Shared(s) => s.dump_non_default(),
        }
    }
    /// Relit une entrée enregistrée dans le même modèle que `self`.
    pub fn reparse(&self, v: &Value) -> Result<Entry> {
        Ok(match self {
            Entry::Project(_) => Entry::Project(project_from_value(v)?),
            Entry::Shared(_) => Entry::Shared(shared_from_value(v)?),
        })
    }
}

pub fn hashed(e: &Entry) -> Map<String, Value> {
    let mut m = e.intent();
    m.retain(|k, _| !HASH_IGNORED.contains(&k.as_str()));
    m
}

/// Empreinte de l'intention (sans les constats du LLM ni les notes) ; `absent` si retiré.
/// Identique octet pour octet à la version Python (sha256 de `json.dumps(..., sort_keys=True)`, 16 hexas).
pub fn hash(e: Option<&Entry>) -> String {
    match e {
        None => "absent".to_string(),
        Some(e) => {
            let digest = Sha256::digest(util::py_json_sorted(&Value::Object(hashed(e))).as_bytes());
            hex::encode(digest)[..16].to_string()
        }
    }
}

/// Champs de l'intention qui diffèrent de l'entrée du dernier run (`st` = état enregistré par mark_applied).
/// Vide si jamais lancé. L'entrée enregistrée est relue dans le modèle courant : un champ ajouté depuis
/// au modèle apparaît avec sa valeur par défaut, donc n'est signalé que s'il a été rempli.
pub fn changed_fields(e: &Entry, st: Option<&Value>) -> Vec<String> {
    let Some(prev) = st.and_then(|s| s.get("entry")).filter(|v| !v.is_null()) else { return vec![] };
    let Ok(before) = e.reparse(prev) else { return vec![] };
    let (before, now) = (hashed(&before), hashed(e));
    let mut keys: Vec<String> = before.keys().chain(now.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().filter(|k| before.get(k) != now.get(k)).collect()
}

pub type State = Map<String, Value>;

fn state_file() -> PathBuf {
    paths::runs_dir().join(".state.json")
}

pub fn load_state() -> State {
    fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| match v {
        Value::Object(m) => Some(m),
        _ => None,
    }).unwrap_or_default()
}

pub fn save_state(state: &State) -> Result<()> {
    let p = state_file();
    fs::create_dir_all(p.parent().unwrap())?;
    fs::write(p, serde_json::to_string_pretty(&Value::Object(state.clone()))?)?;
    Ok(())
}

fn st_hash<'a>(state: &'a State, key: &str) -> Option<&'a str> {
    state.get(key).and_then(|s| s.get("hash")).and_then(Value::as_str)
}

/// Projets et services dont l'entrée diffère de celle du dernier run (indicateur « modifié » du front).
pub fn changed_since_last_run(state: &State) -> Result<Vec<String>> {
    let reg = config::registry()?;
    let sh = config::shared()?.services;
    let mut out: Vec<String> = reg
        .projects
        .iter()
        .filter(|(n, p)| st_hash(state, n) != Some(hash(Some(&Entry::Project((*p).clone()))).as_str()))
        .map(|(n, _)| n.clone())
        .collect();
    out.extend(
        sh.iter()
            .filter(|(n, s)| {
                st_hash(state, &config::shared_key(n)) != Some(hash(Some(&Entry::Shared((*s).clone()))).as_str())
            })
            .map(|(n, _)| config::shared_key(n)),
    );
    out.extend(state.iter().filter(|(n, st)| {
        !reg.projects.contains_key(*n)
            && !sh.contains_key(config::shared_name(n).unwrap_or(n))
            && st.get("hash").and_then(Value::as_str) != Some("absent")
    }).map(|(n, _)| n.clone()));
    Ok(out)
}

pub fn mark_applied(state: &mut State, name: &str, entry: Option<&Entry>, ok: bool, status: Option<&str>) -> Result<()> {
    state.insert(
        name.to_string(),
        json!({
            "hash": hash(entry),
            "entry": entry.map(|e| Value::Object(e.dump())).unwrap_or(Value::Null),
            "ok": ok,
            "status": status,
            "at": util::now_iso(),
        }),
    );
    save_state(state)
}

// ---- file d'attente ----

pub fn queue_dir() -> Result<PathBuf> {
    let d = paths::runs_dir().join("queue");
    fs::create_dir_all(&d)?;
    Ok(d)
}

/// Demande un run pour un projet, ou un service partagé sous la clé `shared.<nom>` (idempotent).
pub fn enqueue(name: &str) -> Result<Value> {
    let p = queue_dir()?.join(format!("{name}.json"));
    if p.exists() {
        if let Ok(v) = serde_json::from_str::<Value>(&fs::read_to_string(&p)?) {
            return Ok(v);
        }
    }
    let req = json!({"project": name, "requested": util::now_iso()});
    fs::write(&p, serde_json::to_string(&req)?)?;
    Ok(req)
}

pub fn queued() -> Result<Vec<String>> {
    let mut out: Vec<String> = fs::read_dir(queue_dir()?)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    out.sort();
    Ok(out)
}

/// Demandes en attente, de la plus ancienne à la plus récente (ordre de traitement).
pub fn queue_files() -> Result<Vec<PathBuf>> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(queue_dir()?)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok().map(|t| (t, e.path())))
        .collect();
    files.sort();
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

pub fn running() -> Option<String> {
    fs::read_to_string(paths::runs_dir().join(".running")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn set_running(name: Option<&str>) -> Result<()> {
    let p = paths::runs_dir().join(".running");
    match name {
        None => {
            let _ = fs::remove_file(p);
        }
        Some(n) => fs::write(p, n)?,
    }
    Ok(())
}

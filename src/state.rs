//! État des runs (runs/.state.json), empreinte « modifié depuis le dernier run », file d'attente (runs/queue/).
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::models::{project_from_value, shared_from_value, Host};
use crate::{config, paths, util};

/// Champs de l'intention purement informatifs (pour l'humain) : changer une note ne rend pas le projet « modifié ».
pub const HASH_IGNORED: [&str; 1] = ["notes"];

/// Projet ou service partagé : ce que hash et prompt voient de la même façon.
#[allow(clippy::large_enum_variant)]
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

/// Valeurs de host.yaml dont dépend le rendu de l'entrée, résolues : zones et IP Tailscale selon les expositions
/// utilisées (projet), IP d'écoute selon `listen` quand un `port` est publié (service partagé). Elles entrent dans
/// le hash sous la clé `host` (champs `host.<nom>` dans « modifié ») : changer `lan_ip` rend le broker « modifié »,
/// changer le modèle LLM ou une note de l'hôte ne touche à rien. Vide quand l'entrée ne dépend pas de l'hôte.
pub fn host_deps(e: &Entry, h: &Host) -> Map<String, Value> {
    let mut d = Map::new();
    match e {
        Entry::Project(p) => {
            let mut exposures: Vec<&str> = vec![p.expose.as_str(), p.previews.as_str()];
            exposures.extend(p.entries.values().map(String::as_str));
            if exposures.contains(&"private") {
                d.insert("private_zone".into(), json!(h.private_zone));
                d.insert("tailscale_ip".into(), json!(h.tailscale_ip));
            }
            if exposures.contains(&"public") {
                d.insert("public_zone".into(), json!(h.public_zone));
                d.insert("public_ingress".into(), json!(h.public_ingress));
                if h.public_ingress == "direct" {
                    d.insert("public_ip".into(), json!(h.public_ip));
                }
            }
        }
        Entry::Shared(s) => {
            if s.port.is_some() {
                for l in &s.listen {
                    match l.as_str() {
                        "lan" => {
                            d.insert("lan_ip".into(), json!(h.lan_ip));
                        }
                        "tailnet" => {
                            d.insert("tailscale_ip".into(), json!(h.tailscale_ip));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    d
}

fn host_deps_now(e: &Entry) -> Map<String, Value> {
    config::host().map(|h| host_deps(e, &h)).unwrap_or_default()
}

/// Intention hashée : l'entrée sans les notes, plus `host` (dépendances à host.yaml) si l'entrée en a.
pub fn hashed(e: &Entry) -> Map<String, Value> {
    let mut m = e.intent();
    m.retain(|k, _| !HASH_IGNORED.contains(&k.as_str()));
    let d = host_deps_now(e);
    if !d.is_empty() {
        m.insert("host".into(), Value::Object(d));
    }
    m
}

/// `host: {lan_ip: …}` → `host.lan_ip: …` pour comparer et nommer les champs.
fn flatten_host(m: Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in m {
        match (k.as_str(), v) {
            ("host", Value::Object(h)) => out.extend(h.into_iter().map(|(hk, hv)| (format!("host.{hk}"), hv))),
            (_, v) => {
                out.insert(k, v);
            }
        }
    }
    out
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
    let (mut before, mut now) = (hashed(&before), hashed(e));
    // les dépendances à l'hôte du dernier run sont enregistrées à part ; un état d'avant cette version n'en a pas :
    // on ne signale alors aucun `host.*` plutôt que d'inventer une différence
    match st.and_then(|s| s.get("host")) {
        Some(Value::Object(h)) if !h.is_empty() => {
            before.insert("host".into(), Value::Object(h.clone()));
        }
        Some(Value::Object(_)) => {
            before.remove("host");
        }
        _ => {
            before.remove("host");
            now.remove("host");
        }
    }
    let (before, now) = (flatten_host(before), flatten_host(now));
    let mut keys: Vec<String> = before.keys().chain(now.keys()).cloned().collect();
    keys.sort();
    keys.dedup();
    // un `host.*` présent d'un seul côté vient d'un changement de l'entrée (exposition, listen), déjà signalé
    keys.into_iter()
        .filter(|k| before.get(k) != now.get(k))
        .filter(|k| !k.starts_with("host.") || (before.contains_key(k) && now.contains_key(k)))
        .collect()
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
            "host": entry.map(host_deps_now).unwrap_or_default(),
            "entry": entry.map(|e| Value::Object(e.dump())).unwrap_or(Value::Null),
            "ok": ok,
            "status": status,
            "at": util::now_iso(),
        }),
    );
    save_state(state)
}

/// Recalcule `hash` et `host` de chaque cible depuis l'entrée de son dernier run et le host.yaml courant (quand la
/// formule du hash change, sinon tout passe « modifié » d'un coup). `ok`, `status`, `at`, `entry` sont conservés.
/// Retourne les cibles recalculées.
pub fn rehash(state: &mut State) -> Result<Vec<String>> {
    let reg = config::registry()?;
    let mut done = vec![];
    for (name, st) in state.iter_mut() {
        let Some(prev) = st.get("entry").filter(|v| !v.is_null()).cloned() else { continue };
        let is_shared = config::shared_name(name).is_some() && !reg.projects.contains_key(name);
        let entry = if is_shared { Entry::Shared(shared_from_value(&prev)?) } else { Entry::Project(project_from_value(&prev)?) };
        st["hash"] = json!(hash(Some(&entry)));
        st["host"] = Value::Object(host_deps_now(&entry));
        done.push(name.clone());
    }
    save_state(state)?;
    Ok(done)
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

//! Lecture/écriture de host.yaml, registry.yaml, shared.yaml. Les écritures préservent l'ordre et les clés inconnues.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{Map, Value};
use serde_yaml::Mapping;

use crate::history;
use crate::models::{Host, Llm, Project, Registry, Shared, SharedRegistry};
use crate::paths;

pub fn load_yaml(p: &Path) -> Result<Mapping> {
    if !p.exists() {
        return Ok(Mapping::new());
    }
    let text = fs::read_to_string(p).with_context(|| format!("lecture de {}", p.display()))?;
    if text.trim().is_empty() {
        return Ok(Mapping::new());
    }
    match serde_yaml::from_str::<serde_yaml::Value>(&text).with_context(|| format!("yaml invalide : {}", p.display()))? {
        serde_yaml::Value::Mapping(m) => Ok(m),
        serde_yaml::Value::Null => Ok(Mapping::new()),
        _ => bail!("{} : un dictionnaire est attendu à la racine", p.display()),
    }
}

pub fn dump_yaml(data: &Mapping, p: &Path) -> Result<()> {
    let text = serde_yaml::to_string(&serde_yaml::Value::Mapping(data.clone()))?;
    fs::write(p, text).with_context(|| format!("écriture de {}", p.display()))
}

fn parse<T: serde::de::DeserializeOwned>(m: &Mapping, what: &str) -> Result<T> {
    serde_yaml::from_value(serde_yaml::Value::Mapping(m.clone())).map_err(|e| anyhow!("{what} : {e}"))
}

pub fn host_file() -> PathBuf {
    paths::config_dir().join("host.yaml")
}
pub fn registry_file() -> PathBuf {
    paths::config_dir().join("registry.yaml")
}
pub fn shared_file() -> PathBuf {
    paths::config_dir().join("shared.yaml")
}

pub fn host() -> Result<Host> {
    let h: Host = parse(&load_yaml(&host_file())?, "host.yaml")?;
    h.validate().map_err(|e| anyhow!("host.yaml : {e}"))?;
    Ok(h)
}

pub fn registry() -> Result<Registry> {
    registry_from(&load_yaml(&registry_file())?)
}

fn registry_from(m: &Mapping) -> Result<Registry> {
    let r: Registry = parse(m, "registry.yaml")?;
    r.validate().map_err(|e| anyhow!("registry.yaml : {e}"))?;
    Ok(r)
}

pub fn project(name: &str) -> Result<Project> {
    registry()?.projects.get(name).cloned().ok_or_else(|| anyhow!("projet inconnu : {name}"))
}

fn yk(k: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(k.to_string())
}

fn json_to_yaml(v: &Value) -> serde_yaml::Value {
    serde_yaml::to_value(v).unwrap_or(serde_yaml::Value::Null)
}

fn sub_mapping<'a>(m: &'a mut Mapping, key: &str) -> &'a mut Mapping {
    let e = m.entry(yk(key)).or_insert_with(|| serde_yaml::Value::Mapping(Mapping::new()));
    if !e.is_mapping() {
        *e = serde_yaml::Value::Mapping(Mapping::new());
    }
    e.as_mapping_mut().unwrap()
}

/// Applique `fields` à `raw[section][name]` : null supprime la clé, le reste écrase ; ordre et autres clés préservés.
fn apply_fields(raw: &mut Mapping, section: &str, name: &str, fields: &Map<String, Value>) {
    let entry = sub_mapping(sub_mapping(raw, section), name);
    for (k, v) in fields {
        if v.is_null() {
            entry.remove(yk(k));
        } else {
            entry.insert(yk(k), json_to_yaml(v));
        }
    }
}

/// Modifie host.yaml → llm (modèle / effort de claude -p), depuis le front ou `infra set-llm`.
pub fn set_llm(fields: &Map<String, Value>) -> Result<Llm> {
    let p = host_file();
    let mut raw = load_yaml(&p)?;
    let llm = sub_mapping(&mut raw, "llm");
    for (k, v) in fields {
        if !v.is_null() {
            llm.insert(yk(k), json_to_yaml(v));
        }
    }
    let h: Host = parse(&raw, "host.yaml")?;
    h.validate()?;
    dump_yaml(&raw, &p)?;
    Ok(h.llm)
}

/// Modifie registry.yaml (utilisé par `infra set` et l'UI). Préserve l'ordre et les autres clés.
/// Chaque écriture est historisée dans runs/history/<P>.jsonl.
pub fn set_project_fields(name: &str, source: &str, fields: &Map<String, Value>) -> Result<Project> {
    let p = registry_file();
    let mut raw = load_yaml(&p)?;
    apply_fields(&mut raw, "projects", name, fields);
    let reg = registry_from(&raw)?; // valide avant d'écrire
    dump_yaml(&raw, &p)?;
    let prj = reg.projects.get(name).cloned().ok_or_else(|| anyhow!("projet inconnu : {name}"))?;
    history::record(name, Some(Value::Object(prj.dump())), source)?;
    Ok(prj)
}

pub fn remove_project(name: &str, source: &str) -> Result<()> {
    let p = registry_file();
    let mut raw = load_yaml(&p)?;
    let removed = raw.get_mut(yk("projects")).and_then(|v| v.as_mapping_mut()).and_then(|m| m.remove(yk(name)));
    if removed.is_some() {
        dump_yaml(&raw, &p)?;
        history::record(name, None, source)?;
    }
    Ok(())
}

pub fn domain_of<'a>(name: &'a str, prj: &'a Project) -> &'a str {
    prj.domain.as_deref().unwrap_or(name)
}

pub fn private_host(name: &str, prj: &Project, h: &Host) -> String {
    format!("{}.{}", domain_of(name, prj), h.private_zone)
}

pub fn public_host(name: &str, prj: &Project, h: &Host) -> String {
    format!("{}.{}", domain_of(name, prj), h.public_zone)
}

// ---- services partagés (shared.yaml) ----

pub fn shared() -> Result<SharedRegistry> {
    shared_from(&load_yaml(&shared_file())?)
}

fn shared_from(m: &Mapping) -> Result<SharedRegistry> {
    let mut r: SharedRegistry = parse(m, "shared.yaml")?;
    r.validate().map_err(|e| anyhow!("shared.yaml : {e}"))?;
    Ok(r)
}

pub fn shared_service(name: &str) -> Result<Shared> {
    shared()?
        .services
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow!("service partagé inconnu : {name} (à déclarer dans shared.yaml)"))
}

/// Projets du registre qui déclarent `uses: [<name>]` : calculé, jamais stocké.
pub fn users_of(name: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> =
        registry()?.projects.iter().filter(|(_, p)| p.uses.iter().any(|u| u == name)).map(|(n, _)| n.clone()).collect();
    out.sort();
    Ok(out)
}

/// Modifie shared.yaml ; chaque écriture est historisée (runs/history/shared.<nom>.jsonl).
pub fn set_shared_fields(name: &str, source: &str, fields: &Map<String, Value>) -> Result<Shared> {
    let p = shared_file();
    let mut raw = load_yaml(&p)?;
    apply_fields(&mut raw, "services", name, fields);
    let reg = shared_from(&raw)?;
    dump_yaml(&raw, &p)?;
    let svc = reg.services.get(name).cloned().ok_or_else(|| anyhow!("service partagé inconnu : {name}"))?;
    history::record(&shared_key(name), Some(Value::Object(svc.dump())), source)?;
    Ok(svc)
}

pub fn remove_shared(name: &str, source: &str) -> Result<()> {
    let users = users_of(name)?;
    if !users.is_empty() {
        bail!("{name} est encore utilisé par : {} (retire-le de leur `uses:` d'abord)", users.join(", "));
    }
    let p = shared_file();
    let mut raw = load_yaml(&p)?;
    let removed = raw.get_mut(yk("services")).and_then(|v| v.as_mapping_mut()).and_then(|m| m.remove(yk(name)));
    if removed.is_some() {
        dump_yaml(&raw, &p)?;
        history::record(&shared_key(name), None, source)?;
    }
    Ok(())
}

/// Clé d'un service partagé dans la file, l'état et les runs (`shared.<nom>` : un nom de projet ne contient pas de point).
pub fn shared_key(name: &str) -> String {
    format!("shared.{name}")
}

pub fn shared_name(key: &str) -> Option<&str> {
    key.strip_prefix("shared.")
}

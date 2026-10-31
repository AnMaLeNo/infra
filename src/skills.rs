//! Résolution des skills pour un projet (skills/README.md).
use std::path::PathBuf;

use indexmap::IndexMap;

use crate::models::{Project, Shared};
use crate::paths;

pub fn resolve(rel: &str) -> Option<PathBuf> {
    [paths::config_dir().join("skills"), paths::tool_dir().join("skills")]
        .iter()
        .map(|b| b.join(rel).join("SKILL.md"))
        .find(|p| p.exists())
}

pub fn for_project(prj: Option<&Project>) -> Vec<String> {
    let Some(p) = prj.filter(|p| p.enabled) else { return vec!["undeploy".into()] };
    let mut rels = match &p.service {
        Some(s) => vec![format!("service/{s}")],
        None => vec!["service/compose".into(), "service/static".into()],
    };
    rels.push(format!("expose/{}", p.expose));
    if p.previews != "none" {
        rels.push("previews".into());
    }
    rels
}

pub fn paths_for(prj: Option<&Project>) -> IndexMap<String, Option<PathBuf>> {
    for_project(prj).into_iter().map(|r| (resolve(&r), r)).map(|(p, r)| (r, p)).collect()
}

/// Service partagé (shared.yaml) : un seul skill, qui couvre aussi l'arrêt (`enabled: false` / retiré).
pub fn for_shared(_svc: Option<&Shared>) -> Vec<String> {
    vec!["shared-service".into()]
}

pub fn paths_for_shared(svc: Option<&Shared>) -> IndexMap<String, Option<PathBuf>> {
    for_shared(svc).into_iter().map(|r| (resolve(&r), r)).map(|(p, r)| (r, p)).collect()
}

fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.file_name().is_some_and(|n| n == "SKILL.md") {
                out.push(p);
            }
        }
    }
}

/// {chemin relatif: "tool"|"custom"} ; un skill du repo de config masque celui de l'outil.
pub fn list_all() -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    for (base, origin) in [(paths::tool_dir().join("skills"), "tool"), (paths::config_dir().join("skills"), "custom")] {
        let mut files = vec![];
        walk(&base, &mut files);
        files.sort();
        for p in files {
            let rel = p.parent().unwrap().strip_prefix(&base).unwrap().to_string_lossy().into_owned();
            out.insert(rel, origin.to_string());
        }
    }
    out
}

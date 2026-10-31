//! Previews par PR : <P>-pr-<N> dans /srv/apps/<P>/previews/pr-<N>/, verrou flock par PR.
use std::fs::{self, File};
use std::os::fd::AsRawFd;

use anyhow::{bail, Result};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

use crate::render::{render, Target};
use crate::{compose, config, paths};

struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

fn lock(t: &Target) -> Result<Lock> {
    let parent = t.dir().parent().unwrap().to_path_buf();
    fs::create_dir_all(&parent)?;
    let f = File::create(parent.join(format!(".pr-{}.lock", t.pr.unwrap_or(0))))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        bail!("verrou de preview impossible : {}", std::io::Error::last_os_error());
    }
    Ok(Lock(f))
}

fn check(t: &Target) -> Result<()> {
    if t.project.previews == "none" {
        bail!("{}: previews désactivées dans le registre (previews: none)", t.name);
    }
    if !t.project.enabled {
        bail!("{}: projet désactivé", t.name);
    }
    Ok(())
}

fn main_url(t: &Target) -> Option<String> {
    let urls = t.urls();
    urls.get("public").or_else(|| urls.get("private")).cloned()
}

pub fn up(t: &Target) -> Result<Value> {
    check(t)?;
    let s = {
        let _l = lock(t)?;
        let commit = compose::fetch(t)?;
        render(t)?;
        compose::up(t)?;
        let mut s = compose::status(t)?;
        s["commit"] = json!(commit);
        s
    };
    Ok(json!({"project": t.compose_project(), "commit": s["commit"], "url": main_url(t), "running": s["running"]}))
}

pub fn down(t: &Target) -> Result<()> {
    let _l = lock(t)?;
    compose::down(t, true)?;
    let _ = fs::remove_dir_all(t.dir());
    let _ = fs::remove_dir_all(paths::data_dir().join(t.compose_project()));
    Ok(())
}

pub fn list_previews(name: &str) -> Result<IndexMap<u64, Value>> {
    let (prj, h) = (config::project(name)?, config::host()?);
    let base = paths::app_dir(name).join("previews");
    let mut out = IndexMap::new();
    if base.exists() {
        let mut dirs: Vec<_> = fs::read_dir(&base)?.flatten().map(|e| e.path()).collect();
        dirs.sort();
        for d in dirs {
            let Some(pr) = d.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("pr-")).and_then(|n| n.parse::<u64>().ok()) else { continue };
            let t = Target::new(name, prj.clone(), h.clone(), Some(pr));
            let s = compose::status(&t)?;
            out.insert(pr, json!({"running": s["running"], "url": main_url(&t), "commit": s["commit"]}));
        }
    }
    Ok(out)
}

static GH_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"github\.com[:/]([^/]+)/([^/.]+)").unwrap());

/// Some(true/false) via l'API GitHub publique, None si indéterminable.
pub fn pr_open(repo_url: &str, pr: u64) -> Option<bool> {
    let m = GH_RE.captures(repo_url)?;
    let url = format!("https://api.github.com/repos/{}/{}/pulls/{pr}", &m[1], &m[2]);
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(15)).user_agent("infra").build().ok()?;
    let r = client.get(url).header("Accept", "application/vnd.github+json").send().ok()?;
    if !r.status().is_success() {
        return None;
    }
    let v: Value = r.json().ok()?;
    Some(v.get("state").and_then(Value::as_str) == Some("open"))
}

pub fn prune(name: &str, everything: bool) -> Result<IndexMap<u64, String>> {
    let (prj, h) = (config::project(name)?, config::host()?);
    let mut out = IndexMap::new();
    for pr in list_previews(name)?.keys() {
        if everything || pr_open(&prj.repo, *pr) == Some(false) {
            down(&Target::new(name, prj.clone(), h.clone(), Some(*pr)))?;
            out.insert(*pr, "supprimée".to_string());
        }
    }
    Ok(out)
}

pub fn workflow_snippet(name: &str) -> Result<String> {
    let h = config::host()?;
    Ok(format!(
        "# .github/workflows/preview.yml  (à committer dans le repo du projet)
name: preview
on:
  pull_request:
    types: [opened, synchronize, reopened, closed]
jobs:
  preview:
    uses: AnMaLeNo/infra/.github/workflows/preview.yml@main
    with:
      project: {name}
    secrets:
      TS_OAUTH_CLIENT_ID: ${{{{ secrets.TS_OAUTH_CLIENT_ID }}}}
      TS_OAUTH_SECRET: ${{{{ secrets.TS_OAUTH_SECRET }}}}
      INFRA_HOST: ${{{{ secrets.INFRA_HOST }}}}
      INFRA_SSH_KEY: ${{{{ secrets.INFRA_SSH_KEY }}}}

# Secrets GitHub à créer sur le repo du projet :
#   TS_OAUTH_CLIENT_ID / TS_OAUTH_SECRET : client OAuth Tailscale (scope auth_keys, tag:ci)
#   INFRA_HOST   = {}
#   INFRA_SSH_KEY = clé privée du compte ai-github (sa clé publique est dans <config>/agents/ai-github.pub)
",
        h.tailscale_ip
    ))
}

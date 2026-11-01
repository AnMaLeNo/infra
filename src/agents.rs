//! Comptes ai-* : créés depuis <config>/agents/*.pub (skills/agent-account).
use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use indexmap::IndexMap;

use crate::paths;
use crate::util::{write_private, Cmd};

pub const SUDOERS_FULL: &str = "Defaults:{u} log_input, log_output
{u} ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra
";
/// profil restreint : GitHub Actions ne peut lancer que `infra preview …`
pub const FORCE_CMD: &str =
    "command=\"/usr/local/bin/infra-preview-only\",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty ";

fn sudo(cmd: &[&str]) -> Result<()> {
    let is_root = unsafe { libc::geteuid() } == 0;
    let mut full: Vec<&str> = if is_root { vec![] } else { vec!["sudo", "-n"] };
    full.extend(cmd);
    Cmd::new().quiet().run(&full)?;
    Ok(())
}

/// (home, existe) via getpwnam
fn passwd(user: &str) -> Option<PathBuf> {
    let c = std::ffi::CString::new(user).ok()?;
    let p = unsafe { libc::getpwnam(c.as_ptr()) };
    if p.is_null() {
        return None;
    }
    let dir = unsafe { std::ffi::CStr::from_ptr((*p).pw_dir) };
    Some(PathBuf::from(dir.to_string_lossy().into_owned()))
}

fn all_users() -> Vec<(String, PathBuf)> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() >= 6).then(|| (f[0].to_string(), PathBuf::from(f[5])))
        })
        .collect()
}

/// Script ForceCommand d'ai-github : n'exécute que `infra preview up|down …` (SSH_ORIGINAL_COMMAND), rien d'autre.
pub const PREVIEW_ONLY: &str = "#!/bin/sh
# Installé par `infra agents sync` : seule commande autorisée au compte ai-github (GitHub Actions).
set -- $SSH_ORIGINAL_COMMAND
case \"$1 $2 $3\" in
  \"infra preview up\"|\"infra preview down\") shift; exec /usr/local/bin/infra \"$@\" ;;
  *) echo \"ai-github : seul 'infra preview up|down <projet> <pr>' est autorisé\" >&2; exit 126 ;;
esac
";

fn install_preview_only() -> Result<()> {
    let p = "/usr/local/bin/infra-preview-only";
    if std::fs::read_to_string(p).map(|cur| cur == PREVIEW_ONLY).unwrap_or(false) {
        return Ok(());
    }
    let tmp = std::env::temp_dir().join("infra-preview-only");
    std::fs::write(&tmp, PREVIEW_ONLY)?;
    sudo(&["install", "-m", "0755", "-o", "root", "-g", "root", &tmp.to_string_lossy(), p])?;
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

pub fn sync(prune: bool) -> Result<IndexMap<String, String>> {
    install_preview_only()?;
    let d = paths::config_dir().join("agents");
    let mut out = IndexMap::new();
    let mut keys: IndexMap<String, String> = IndexMap::new();
    if d.exists() {
        let mut files: Vec<PathBuf> = fs::read_dir(&d)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pub") && p.file_name().unwrap().to_string_lossy().starts_with("ai-"))
            .collect();
        files.sort();
        for p in files {
            keys.insert(p.file_stem().unwrap().to_string_lossy().into_owned(), fs::read_to_string(&p)?.trim().to_string());
        }
    }
    for (user, key) in &keys {
        let created = passwd(user).is_none();
        if created {
            sudo(&["useradd", "-m", "-s", "/bin/bash", "-G", "docker,infra", "-c", &format!("Agent IA ({user})"), user])?;
        }
        let home = passwd(user).unwrap_or_else(|| PathBuf::from(format!("/home/{user}")));
        let line = if user == "ai-github" { format!("{FORCE_CMD}{key}") } else { key.clone() };
        let ssh = home.join(".ssh");
        let ak = ssh.join("authorized_keys");
        sudo(&["mkdir", "-p", &ssh.to_string_lossy()])?;
        let tmp = PathBuf::from(format!("/tmp/.ak-{user}"));
        write_private(&tmp, &format!("{line}\n"), 0o600)?;
        sudo(&["install", "-o", user, "-g", user, "-m", "0600", &tmp.to_string_lossy(), &ak.to_string_lossy()])?;
        sudo(&["chmod", "0700", &ssh.to_string_lossy()])?;
        sudo(&["chown", &format!("{user}:{user}"), &ssh.to_string_lossy()])?;
        let _ = fs::remove_file(&tmp);
        if user != "ai-github" {
            let tmp_s = PathBuf::from(format!("/tmp/.sudoers-{user}"));
            write_private(&tmp_s, &SUDOERS_FULL.replace("{u}", user), 0o440)?;
            sudo(&["install", "-o", "root", "-g", "root", "-m", "0440", &tmp_s.to_string_lossy(), &format!("/etc/sudoers.d/{user}")])?;
            let _ = fs::remove_file(&tmp_s);
        }
        if created {
            sudo(&["usermod", "-L", "-aG", "docker,infra", user])?;
        }
        out.insert(user.clone(), if created { "créé" } else { "ok" }.to_string());
    }
    if prune {
        for (name, home) in all_users() {
            if name.starts_with("ai-") && name != "ai-infra" && !keys.contains_key(&name) {
                sudo(&["usermod", "-L", "-s", "/usr/sbin/nologin", &name])?;
                sudo(&["rm", "-f", &format!("/etc/sudoers.d/{name}"), &format!("{}/.ssh/authorized_keys", home.display())])?;
                out.insert(name, "verrouillé (plus de clé)".into());
            }
        }
    }
    Ok(out)
}

/// Verrouille d'anciens comptes partagés (ne supprime pas le home).
pub fn retire(names: &[String]) -> Result<IndexMap<String, String>> {
    let mut out = IndexMap::new();
    for n in names {
        sudo(&["usermod", "-L", "-s", "/usr/sbin/nologin", n])?;
        sudo(&["rm", "-f", &format!("/etc/sudoers.d/{n}")])?;
        sudo(&["bash", "-c", &format!("[ -f ~{n}/.ssh/authorized_keys ] && mv ~{n}/.ssh/authorized_keys ~{n}/.ssh/authorized_keys.retired || true")])?;
        out.insert(n.clone(), "verrouillé".into());
    }
    Ok(out)
}

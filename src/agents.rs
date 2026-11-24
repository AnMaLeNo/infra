//! Comptes agents : un par fichier `<config>/agents/<nom>.pub`, niveau de sudo dans `<nom>.yaml` (skills/agent-account).
//!
//! Le repère « compte géré par infra » est le groupe `infra-agents` (les anciens comptes `ai-*` sont aussi reconnus),
//! pas le nom : c'est lui qui autorise `sync` à modifier un compte existant et `--prune` à le verrouiller. Un compte
//! existant sans ce repère (humain, compte créé à la main) n'est jamais touché.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use serde::Deserialize;

use crate::paths;
use crate::util::{write_private, Cmd};

/// Groupe Unix qui marque les comptes gérés par `infra agents sync`.
pub const MANAGED_GROUP: &str = "infra-agents";
/// Comptes que `agents/` ne peut pas décrire (`ai-infra` est géré par install.sh).
pub const RESERVED: &[&str] = &["root", "ai-infra"];

/// profil restreint : GitHub Actions ne peut lancer que `infra preview …`
pub const FORCE_CMD: &str =
    "command=\"/usr/local/bin/infra-preview-only\",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty ";

/// Niveau de sudo d'un agent (`sudo:` dans `agents/<nom>.yaml`, `limited` si absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sudo {
    /// pas de fichier sudoers
    None,
    /// systemctl, tailscale, apt-get, infra
    #[default]
    Limited,
    /// tout, sans mot de passe
    Full,
}

impl Sudo {
    pub fn label(self) -> &'static str {
        match self {
            Sudo::None => "aucun",
            Sudo::Limited => "limité",
            Sudo::Full => "complet",
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentConf {
    #[serde(default)]
    sudo: Sudo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub key: String,
    pub sudo: Sudo,
}

/// Nom de compte Unix portable : `[a-z_][a-z0-9_-]*`, 32 caractères max (pas de `.` : sudoers.d ignorerait le fichier).
pub fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    n.len() <= 32 && matches!(c.next(), Some('a'..='z' | '_')) && c.all(|ch| matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-'))
}

/// Lit `<dir>/*.pub` (+ `<nom>.yaml` facultatif). Erreur sur tout fichier invalide : mieux vaut ne rien appliquer.
pub fn scan(dir: &Path) -> Result<Vec<Agent>> {
    let mut out = vec![];
    if !dir.exists() {
        return Ok(out);
    }
    let mut files: Vec<PathBuf> =
        fs::read_dir(dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "pub")).collect();
    files.sort();
    for p in files {
        let name = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if !valid_name(&name) {
            bail!("{} : nom de compte invalide ([a-z_][a-z0-9_-]*, 32 caractères max)", p.display());
        }
        if RESERVED.contains(&name.as_str()) {
            bail!("{} : compte réservé, géré hors de agents/", p.display());
        }
        let key = fs::read_to_string(&p)?.trim().to_string();
        if key.is_empty() || key.lines().count() != 1 {
            bail!("{} : une seule clé publique, sur une ligne", p.display());
        }
        let conf_p = p.with_extension("yaml");
        let conf: AgentConf = match fs::read_to_string(&conf_p) {
            Ok(s) if !s.trim().is_empty() => {
                serde_yaml::from_str(&s).with_context(|| format!("{} (attendu : sudo: none|limited|full)", conf_p.display()))?
            }
            _ => AgentConf::default(),
        };
        let sudo = if name == "ai-github" {
            if conf_p.exists() && conf.sudo != Sudo::None {
                bail!("{} : ai-github n'a jamais de sudo (ForceCommand infra preview)", conf_p.display());
            }
            Sudo::None
        } else {
            conf.sudo
        };
        out.push(Agent { name, key, sudo });
    }
    Ok(out)
}

/// Contenu de `/etc/sudoers.d/<nom>` (None : pas de fichier). Toujours journalisé (sudoreplay).
pub fn sudoers(a: &Agent) -> Option<String> {
    let u = &a.name;
    let rule = match a.sudo {
        Sudo::None => return None,
        Sudo::Limited => format!("{u} ALL=(root) NOPASSWD: /usr/bin/systemctl, /usr/bin/tailscale, /usr/bin/apt-get, /usr/local/bin/infra"),
        Sudo::Full => format!("{u} ALL=(ALL:ALL) NOPASSWD: ALL"),
    };
    Some(format!(
        "# Installé par `infra agents sync` (agents/{u}.pub, sudo {}). Rejouer : sudo sudoreplay -l user {u}\nDefaults:{u} log_input, log_output\n{rule}\n",
        a.sudo.label()
    ))
}

/// Membres (secondaires) d'un groupe, d'après le contenu de /etc/group.
pub fn group_members(etc_group: &str, group: &str) -> Vec<String> {
    etc_group
        .lines()
        .find_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() >= 4 && f[0] == group).then(|| f[3].split(',').filter(|s| !s.is_empty()).map(String::from).collect())
        })
        .unwrap_or_default()
}

/// Compte géré par infra : membre de `infra-agents`, ou ancien compte `ai-*` (hors réservés).
pub fn is_managed(name: &str, members: &[String]) -> bool {
    !RESERVED.contains(&name) && (members.iter().any(|m| m == name) || name.starts_with("ai-"))
}

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

/// Écrit `/etc/sudoers.d/<user>` après validation par visudo (un sudoers invalide casserait sudo pour tout l'hôte).
/// Le fichier est d'abord posé sous un nom contenant un `.`, que sudo ignore, puis renommé.
fn install_sudoers(user: &str, content: &str) -> Result<()> {
    let tmp = PathBuf::from(format!("/tmp/.sudoers-{user}"));
    write_private(&tmp, content, 0o440)?;
    let staged = format!("/etc/sudoers.d/.{user}.infra-new");
    let res = sudo(&["install", "-o", "root", "-g", "root", "-m", "0440", &tmp.to_string_lossy(), &staged])
        .and_then(|_| sudo(&["visudo", "-c", "-q", "-f", &staged]))
        .and_then(|_| sudo(&["mv", "-f", &staged, &format!("/etc/sudoers.d/{user}")]));
    let _ = fs::remove_file(&tmp);
    if res.is_err() {
        let _ = sudo(&["rm", "-f", &staged]);
    }
    res.with_context(|| format!("sudoers de {user} refusé par visudo"))
}

pub fn sync(prune: bool) -> Result<IndexMap<String, String>> {
    let agents = scan(&paths::config_dir().join("agents"))?;
    install_preview_only()?;
    sudo(&["groupadd", "-f", MANAGED_GROUP])?;
    let members = group_members(&fs::read_to_string("/etc/group").unwrap_or_default(), MANAGED_GROUP);
    let groups = format!("docker,infra,{MANAGED_GROUP}");
    let mut out = IndexMap::new();
    for a in &agents {
        let user = a.name.as_str();
        let created = passwd(user).is_none();
        if !created && !is_managed(user, &members) {
            out.insert(
                user.to_string(),
                format!("ignoré : compte existant non géré par infra (pas dans le groupe {MANAGED_GROUP}) ; le supprimer, ou l'y ajouter s'il doit l'être"),
            );
            continue;
        }
        if created {
            sudo(&["useradd", "-m", "-s", "/bin/bash", "-G", &groups, "-c", &format!("Agent ({user})"), user])?;
            // pas de mot de passe : connexion par clé uniquement
            sudo(&["usermod", "-L", user])?;
        } else {
            // rétablit aussi un compte verrouillé par un --prune précédent dont la clé est revenue
            sudo(&["usermod", "-s", "/bin/bash", "-a", "-G", &groups, user])?;
        }
        let home = passwd(user).unwrap_or_else(|| PathBuf::from(format!("/home/{user}")));
        sudo(&["chmod", "0700", &home.to_string_lossy()])?;
        let line = if user == "ai-github" { format!("{FORCE_CMD}{}", a.key) } else { a.key.clone() };
        let ssh = home.join(".ssh");
        let ak = ssh.join("authorized_keys");
        sudo(&["mkdir", "-p", &ssh.to_string_lossy()])?;
        let tmp = PathBuf::from(format!("/tmp/.ak-{user}"));
        write_private(&tmp, &format!("{line}\n"), 0o600)?;
        sudo(&["install", "-o", user, "-g", user, "-m", "0600", &tmp.to_string_lossy(), &ak.to_string_lossy()])?;
        sudo(&["chmod", "0700", &ssh.to_string_lossy()])?;
        sudo(&["chown", &format!("{user}:{user}"), &ssh.to_string_lossy()])?;
        let _ = fs::remove_file(&tmp);
        match sudoers(a) {
            Some(content) => install_sudoers(user, &content)?,
            None => sudo(&["rm", "-f", &format!("/etc/sudoers.d/{user}")])?,
        }
        out.insert(user.to_string(), format!("{} (sudo {})", if created { "créé" } else { "ok" }, a.sudo.label()));
    }
    if prune {
        for (name, home) in all_users() {
            if is_managed(&name, &members) && !agents.iter().any(|a| a.name == name) {
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

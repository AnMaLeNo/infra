//! Stacks d'infra de l'outil (tool/stack/<nom>, surchargeables dans <config>/stack/<nom>) : traefik, cloudflared, ui…
//! Les services consommés par les projets (broker MQTT, base…) ne sont pas des stacks : voir shared.yaml.
use std::fs;
use std::os::unix::fs::{chown, PermissionsExt};
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

use crate::util::{dump_dotenv, run_json, write_private, Cmd};
use crate::{config, paths, secrets, VERSION};

pub const REQUIRED: [&str; 2] = ["traefik", "ui"];
pub const NETWORK: &str = "traefik-net";
/// fixe à la création : la passerelle sert aux ponts vers l'hôte
pub const NETWORK_SUBNET: &str = "172.30.0.0/24";
/// l'image officielle cloudflared tourne en `nonroot` (uid 65532) : le fichier de credentials doit lui appartenir
pub const CLOUDFLARED_UID: u32 = 65532;

/// Stack de l'hôte (<config>/stack/<nom>) prioritaire sur celle de l'outil (tool/stack/<nom>).
pub fn stack_dir(name: &str) -> Result<PathBuf> {
    [paths::config_dir().join("stack").join(name), paths::tool_dir().join("stack").join(name)]
        .into_iter()
        .find(|d| d.join("docker-compose.yml").exists())
        .ok_or_else(|| anyhow!("stack inconnue : {name}"))
}

pub fn state_dir(name: &str) -> Result<PathBuf> {
    let d = paths::stack_state_dir().join(name);
    fs::create_dir_all(&d)?;
    Ok(d)
}

pub fn ensure_network() -> Result<()> {
    let nets = Cmd::new().quiet().run(&["docker", "network", "ls", "--format", "{{.Name}}"])?.stdout;
    if !nets.split_whitespace().any(|n| n == NETWORK) {
        Cmd::new().quiet().run(&["docker", "network", "create", "--subnet", NETWORK_SUBNET, NETWORK])?;
    }
    Ok(())
}

/// IP de l'hôte vue depuis traefik-net (cible des services legacy qui n'écoutent que sur l'hôte).
pub fn network_gateway() -> Result<String> {
    ensure_network()?;
    let out = Cmd::new()
        .quiet()
        .run(&["docker", "network", "inspect", NETWORK, "-f", "{{(index .IPAM.Config 0).Gateway}}"])?
        .stdout
        .trim()
        .to_string();
    Ok(if out.is_empty() { format!("{}.1", NETWORK_SUBNET.rsplit_once('.').unwrap().0) } else { out })
}

fn uid(user: &str) -> String {
    let c = std::ffi::CString::new(user).unwrap();
    let p = unsafe { libc::getpwnam(c.as_ptr()) };
    if p.is_null() { "0".into() } else { unsafe { (*p).pw_uid }.to_string() }
}

fn gid(group: &str) -> String {
    let c = std::ffi::CString::new(group).unwrap();
    let g = unsafe { libc::getgrnam(c.as_ptr()) };
    if g.is_null() { "0".into() } else { unsafe { (*g).gr_gid }.to_string() }
}

pub fn stack_env(name: &str) -> Result<IndexMap<String, String>> {
    let h = config::host()?;
    let ienv = secrets::infra_env();
    let get = |k: &str| ienv.get(k).cloned().unwrap_or_default();
    Ok(IndexMap::from([
        ("TAILSCALE_IP".to_string(), h.tailscale_ip.clone()),
        ("PRIVATE_ZONE".into(), h.private_zone.clone()),
        ("PUBLIC_ZONE".into(), h.public_zone.clone()),
        ("PUBLIC_INGRESS".into(), h.public_ingress.clone()),
        ("STACK_STATE".into(), state_dir(name)?.to_string_lossy().into_owned()),
        ("ACME_EMAIL".into(), get("ACME_EMAIL")),
        ("CF_API_TOKEN".into(), get("CF_API_TOKEN")),
        ("TUNNEL_ID".into(), h.cloudflare.tunnel_id.clone()),
        ("TRAEFIK_NET_GATEWAY".into(), network_gateway()?),
        ("INFRA_VERSION".into(), std::env::var("INFRA_VERSION").unwrap_or_else(|_| VERSION.to_string())),
        // l'UI écrit registry.yaml et runs/queue : même identité que le runner, sinon les fichiers
        // créés par root dans le conteneur sont illisibles/insupprimables pour ai-infra
        ("INFRA_UID".into(), uid("ai-infra")),
        ("INFRA_GID".into(), gid("infra")),
        ("DOCKER_GID".into(), gid("docker")),
    ]))
}

static ENV_TPL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\{\s*env\s+`([A-Z0-9_]+)`\s*\}\}").unwrap());

/// Config statique Traefik et config.yml de cloudflared ne sont templatées ni par Traefik (seule la
/// dynamique l'est) ni par cloudflared (`${X}` est pris littéralement) : on remplace nous-mêmes
/// les `{{ env `X` }}` avant d'écrire le fichier dans STACK_STATE.
pub fn render_static(text: &str, env: &IndexMap<String, String>) -> String {
    ENV_TPL.replace_all(text, |c: &regex::Captures| env.get(&c[1]).cloned().unwrap_or_default()).into_owned()
}

/// Config dynamique Traefik servie = fichiers de l'outil + ceux de l'hôte (<config>/stack/traefik/dynamic).
fn sync_dynamic(dst: &std::path::Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    let mut wanted: IndexMap<String, String> = IndexMap::new();
    for src in [paths::tool_dir().join("stack/traefik/dynamic"), paths::config_dir().join("stack/traefik/dynamic")] {
        if src.is_dir() {
            let mut files: Vec<PathBuf> =
                fs::read_dir(&src)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yml")).collect();
            files.sort();
            for f in files {
                wanted.insert(f.file_name().unwrap().to_string_lossy().into_owned(), fs::read_to_string(&f)?);
            }
        }
    }
    for old in fs::read_dir(dst)?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yml")) {
        if !wanted.contains_key(&*old.file_name().unwrap().to_string_lossy()) {
            fs::remove_file(old)?;
        }
    }
    for (name, content) in &wanted {
        write_private(&dst.join(name), content, 0o644)?;
    }
    Ok(())
}

/// Rend les fichiers de la stack dans STACK_STATE. Retourne true si un fichier a changé : les conteneurs
/// ne voient pas un fichier remplacé (rename) derrière un bind mount, il faut alors les recréer.
pub fn prepare(name: &str) -> Result<bool> {
    let mut changed = false;
    let sd = state_dir(name)?;
    if name == "traefik" {
        let acme = sd.join("acme.json");
        if !acme.exists() {
            fs::write(&acme, "{}")?;
        }
        fs::set_permissions(&acme, fs::Permissions::from_mode(0o600))?;
        fs::create_dir_all(sd.join("certs"))?;
        sync_dynamic(&sd.join("dynamic"))?;
        let src = stack_dir("traefik")?.join(format!("traefik.{}.yml", config::host()?.public_ingress));
        changed |= write_private(&sd.join("traefik.yml"), &render_static(&fs::read_to_string(src)?, &stack_env("traefik")?), 0o644)?;
    }
    if name == "cloudflared" {
        if !secrets::exists("cloudflared.json") {
            bail!("secrets/cloudflared.json absent : lance `infra tunnel create`");
        }
        let creds = sd.join("credentials.json");
        changed |= write_private(&creds, &secrets::decrypt("cloudflared.json")?, 0o400)?;
        chown(&creds, Some(CLOUDFLARED_UID), Some(0))?;
        let src = stack_dir("cloudflared")?.join("config.yml");
        changed |= write_private(&sd.join("config.yml"), &render_static(&fs::read_to_string(src)?, &stack_env("cloudflared")?), 0o644)?;
    }
    Ok(changed)
}

pub fn compose_cmd(name: &str) -> Result<Vec<String>> {
    let d = stack_dir(name)?;
    let env_file = state_dir(name)?.join(".env");
    write_private(&env_file, &dump_dotenv(&stack_env(name)?), 0o600)?;
    let mut cmd: Vec<String> = vec!["docker".into(), "compose".into(), "-p".into(), format!("infra-{name}"), "--env-file".into()];
    cmd.push(env_file.to_string_lossy().into_owned());
    cmd.extend(["-f".to_string(), d.join("docker-compose.yml").to_string_lossy().into_owned()]);
    if name == "traefik" && config::host()?.public_ingress == "direct" {
        cmd.extend(["-f".to_string(), d.join("docker-compose.direct.yml").to_string_lossy().into_owned()]);
    }
    Ok(cmd)
}

pub fn up(name: &str) -> Result<()> {
    ensure_network()?;
    let recreate = prepare(name)?;
    let mut cmd = compose_cmd(name)?;
    cmd.extend(["up", "-d", "--remove-orphans", "--quiet-pull"].map(String::from));
    if recreate {
        cmd.push("--force-recreate".into());
    }
    Cmd::new().no_capture().run(&cmd)?;
    Ok(())
}

pub fn down(name: &str) -> Result<()> {
    let mut cmd = compose_cmd(name)?;
    cmd.extend(["down", "--remove-orphans"].map(String::from));
    Cmd::new().no_capture().run(&cmd)?;
    Ok(())
}

pub fn status(name: &str) -> Result<IndexMap<String, String>> {
    let rows = run_json(&["docker", "compose", "-p", &format!("infra-{name}"), "ps", "-a", "--format", "json"], None, None)?;
    Ok(rows
        .iter()
        .map(|c| {
            let s = |k: &str| c.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
            (s("Service").unwrap_or_default(), s("Health").or_else(|| s("State")).unwrap_or_default())
        })
        .collect())
}

pub fn all_names() -> Result<Vec<String>> {
    let h = config::host()?;
    let mut names: Vec<String> = h.stacks.clone();
    for r in REQUIRED {
        if !names.iter().any(|n| n == r) {
            names.push(r.to_string());
        }
    }
    let order = |n: &str| ["traefik", "cloudflared", "ui"].iter().position(|x| *x == n).unwrap_or(9);
    names.sort_by_key(|n| (order(n), n.clone()));
    names.dedup();
    Ok(names)
}

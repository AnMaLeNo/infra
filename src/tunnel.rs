//! Création du Cloudflare Tunnel géré localement (credentials chiffrés dans secrets/cloudflared.json).
use anyhow::{bail, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::dns::Cf;
use crate::{config, paths, secrets};

fn random_secret() -> Result<String> {
    use std::io::Read;
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buf)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(buf))
}

fn ids(cf: &Cf, h: &crate::models::Host) -> Result<(String, String)> {
    let zid = if h.cloudflare.zone_id.is_empty() { cf.zone_id(&h.public_zone)? } else { h.cloudflare.zone_id.clone() };
    let acct = if h.cloudflare.account_id.is_empty() { cf.account_id(&zid)? } else { h.cloudflare.account_id.clone() };
    Ok((zid, acct))
}

pub fn create(name: Option<&str>) -> Result<String> {
    let h = config::host()?;
    if secrets::exists("cloudflared.json") && !h.cloudflare.tunnel_id.is_empty() {
        return Ok(h.cloudflare.tunnel_id.clone());
    }
    let cf = Cf::new(None)?;
    let (zid, acct) = ids(&cf, &h)?;
    let tunnel_name = name.map(str::to_string).unwrap_or_else(|| format!("infra-{}", h.name));
    let existing = cf.get(&format!("/accounts/{acct}/cfd_tunnel"), &[("name", &tunnel_name), ("is_deleted", "false")])?;
    if let Some(t) = existing.get(0) {
        bail!(
            "un tunnel `{tunnel_name}` existe déjà ({}) : supprime-le ou choisis un autre nom (ses credentials ne sont pas récupérables)",
            t["id"].as_str().unwrap_or("")
        );
    }
    let secret = random_secret()?;
    let t = cf.post(&format!("/accounts/{acct}/cfd_tunnel"), &json!({"name": tunnel_name, "tunnel_secret": secret, "config_src": "local"}))?;
    let tid = t["id"].as_str().unwrap_or("").to_string();
    let creds = json!({"AccountTag": acct, "TunnelSecret": secret, "TunnelID": tid, "TunnelName": tunnel_name});
    secrets::encrypt_write("cloudflared.json", &format!("{}\n", serde_json::to_string_pretty(&creds)?))?;
    let hf = paths::config_dir().join("host.yaml");
    let mut raw = config::load_yaml(&hf)?;
    let key = serde_yaml::Value::String("cloudflare".into());
    let cfm = raw.entry(key).or_insert_with(|| serde_yaml::Value::Mapping(Default::default()));
    if let serde_yaml::Value::Mapping(m) = cfm {
        for (k, v) in [("zone_id", &zid), ("account_id", &acct), ("tunnel_id", &tid)] {
            m.insert(serde_yaml::Value::String(k.into()), serde_yaml::Value::String(v.clone()));
        }
    }
    config::dump_yaml(&raw, &hf)?;
    Ok(tid)
}

pub fn info() -> Result<Value> {
    let h = config::host()?;
    if h.cloudflare.tunnel_id.is_empty() {
        return Ok(json!({"tunnel_id": null}));
    }
    let cf = Cf::new(None)?;
    let (_, acct) = ids(&cf, &h)?;
    let t = cf.get(&format!("/accounts/{acct}/cfd_tunnel/{}", h.cloudflare.tunnel_id), &[])?;
    Ok(json!({
        "tunnel_id": t["id"], "name": t["name"], "status": t.get("status"),
        "connections": t.get("connections").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0),
    }))
}

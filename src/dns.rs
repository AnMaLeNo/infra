//! Enregistrements DNS Cloudflare de la zone publique (docs/conventions.md § DNS).
use anyhow::{anyhow, bail, Result};
use indexmap::IndexMap;
use reqwest::blocking::{Client, Response};
use serde_json::{json, Value};

use crate::models::{Host, Project};
use crate::{config, secrets};

pub const API: &str = "https://api.cloudflare.com/client/v4";

pub struct Cf {
    c: Client,
}

/// (name, type, content, proxied)
pub type Record = (String, String, String, bool);

impl Cf {
    pub fn new(token: Option<String>) -> Result<Self> {
        let token = token
            .or_else(|| secrets::infra_env().get("CF_API_TOKEN").cloned())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| anyhow!("CF_API_TOKEN absent de secrets/infra.env"))?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Authorization", format!("Bearer {token}").parse()?);
        let c = Client::builder().default_headers(headers).timeout(std::time::Duration::from_secs(30)).build()?;
        Ok(Cf { c })
    }

    fn url(path: &str) -> String {
        format!("{API}{path}")
    }

    fn ok(r: Response) -> Result<Value> {
        let mut d: Value = r.json()?;
        if d.get("success").and_then(Value::as_bool) != Some(true) {
            bail!("Cloudflare : {}", d.get("errors").unwrap_or(&Value::Null));
        }
        Ok(d["result"].take())
    }

    pub fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Value> {
        Self::ok(self.c.get(Self::url(path)).query(params).send()?)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value> {
        Self::ok(self.c.post(Self::url(path)).json(body).send()?)
    }

    fn put(&self, path: &str, body: &Value) -> Result<Value> {
        Self::ok(self.c.put(Self::url(path)).json(body).send()?)
    }

    fn delete(&self, path: &str) -> Result<Value> {
        Self::ok(self.c.delete(Self::url(path)).send()?)
    }

    pub fn zone_id(&self, zone: &str) -> Result<String> {
        let res = self.get("/zones", &[("name", zone)])?;
        res.get(0)
            .and_then(|z| z.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("zone {zone} introuvable pour ce token"))
    }

    pub fn account_id(&self, zone_id: &str) -> Result<String> {
        Ok(self.get(&format!("/zones/{zone_id}"), &[])?["account"]["id"].as_str().unwrap_or("").to_string())
    }

    pub fn records(&self, zone_id: &str) -> Result<Vec<Value>> {
        Ok(self.get(&format!("/zones/{zone_id}/dns_records"), &[("per_page", "500")])?.as_array().cloned().unwrap_or_default())
    }

    /// Crée/corrige l'enregistrement. Retourne 'ok' | 'created' | 'updated' | 'conflict'.
    pub fn ensure(&self, zone_id: &str, name: &str, type_: &str, content: &str, proxied: bool, force: bool) -> Result<String> {
        let s = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let existing: Vec<Value> = self
            .records(zone_id)?
            .into_iter()
            .filter(|r| s(r, "name") == name && matches!(s(r, "type").as_str(), "A" | "AAAA" | "CNAME"))
            .collect();
        let body = json!({"type": type_, "name": name, "content": content, "proxied": proxied, "ttl": 1});
        let Some(r) = existing.first() else {
            self.post(&format!("/zones/{zone_id}/dns_records"), &body)?;
            return Ok("created".into());
        };
        if s(r, "type") == type_ && s(r, "content") == content && r["proxied"].as_bool().unwrap_or(false) == proxied && existing.len() == 1 {
            return Ok("ok".into());
        }
        if !force {
            return Ok("conflict".into());
        }
        for extra in &existing[1..] {
            self.delete(&format!("/zones/{zone_id}/dns_records/{}", s(extra, "id")))?;
        }
        self.put(&format!("/zones/{zone_id}/dns_records/{}", s(r, "id")), &body)?;
        Ok("updated".into())
    }

    pub fn delete_name(&self, zone_id: &str, name: &str) -> Result<usize> {
        let mut n = 0;
        for r in self.records(zone_id)? {
            if r.get("name").and_then(Value::as_str) == Some(name) {
                self.delete(&format!("/zones/{zone_id}/dns_records/{}", r["id"].as_str().unwrap_or("")))?;
                n += 1;
            }
        }
        Ok(n)
    }
}

pub fn zone(cf: &Cf, h: &Host) -> Result<String> {
    if h.cloudflare.zone_id.is_empty() { cf.zone_id(&h.public_zone) } else { Ok(h.cloudflare.zone_id.clone()) }
}

/// (name, type, content, proxied) voulus pour un projet.
pub fn wanted_records(name: &str, prj: &Project, h: &Host) -> Result<Vec<Record>> {
    let d = config::domain_of(name, prj);
    let pub_ = if h.public_ingress == "cloudflare-tunnel" {
        if h.cloudflare.tunnel_id.is_empty() {
            bail!("host.yaml: cloudflare.tunnel_id manquant (infra tunnel create)");
        }
        ("CNAME".to_string(), format!("{}.cfargotunnel.com", h.cloudflare.tunnel_id), true)
    } else {
        ("A".to_string(), h.public_ip.clone().unwrap_or_default(), false)
    };
    let p = |n: String| (n, pub_.0.clone(), pub_.1.clone(), pub_.2);
    let mut out = vec![];
    if prj.expose == "public" {
        out.push(p(format!("{d}.{}", h.public_zone)));
    }
    if prj.previews == "public" {
        out.push(p(format!("*.{d}.{}", h.public_zone)));
    }
    if prj.previews == "private" {
        out.push((format!("*.{d}.{}", h.private_zone), "A".into(), h.tailscale_ip.clone(), false));
    }
    for (label, exp) in &prj.entries {
        if exp == "public" {
            out.push(p(format!("{label}.{}", h.public_zone)));
        }
    }
    Ok(out)
}

pub fn project_names(name: &str, prj: &Project, h: &Host) -> Vec<String> {
    let d = config::domain_of(name, prj);
    let mut v = vec![format!("{d}.{}", h.public_zone), format!("*.{d}.{}", h.public_zone), format!("*.{d}.{}", h.private_zone)];
    v.extend(prj.entries.keys().map(|l| format!("{l}.{}", h.public_zone)));
    v
}

pub fn ensure_project(name: &str, prj: &Project, h: &Host, force: bool) -> Result<IndexMap<String, String>> {
    let cf = Cf::new(None)?;
    let zid = zone(&cf, h)?;
    let wanted = wanted_records(name, prj, h)?;
    let mut result = IndexMap::new();
    for (n, t, c, p) in &wanted {
        result.insert(n.clone(), cf.ensure(&zid, n, t, c, *p, force)?);
    }
    // supprime ce qui n'est plus voulu (ex. previews désactivées)
    for n in project_names(name, prj, h) {
        if !wanted.iter().any(|w| w.0 == n) && cf.delete_name(&zid, &n)? > 0 {
            result.insert(n, "deleted".into());
        }
    }
    Ok(result)
}

pub fn prune_project(name: &str, prj: &Project, h: &Host, public_only: bool) -> Result<IndexMap<String, String>> {
    let cf = Cf::new(None)?;
    let zid = zone(&cf, h)?;
    let mut names = project_names(name, prj, h);
    if public_only {
        names.retain(|n| n.ends_with(&h.public_zone) && !n.starts_with("*."));
    }
    let mut out = IndexMap::new();
    for n in names {
        if cf.delete_name(&zid, &n)? > 0 {
            out.insert(n, "deleted".into());
        }
    }
    Ok(out)
}

pub fn bootstrap(h: &Host) -> Result<IndexMap<String, String>> {
    let cf = Cf::new(None)?;
    let zid = zone(&cf, h)?;
    let want = format!("*.{}", h.private_zone);
    let r = cf.ensure(&zid, &want, "A", &h.tailscale_ip, false, true)?;
    Ok(IndexMap::from([(want, r)]))
}

pub fn list_records(h: &Host) -> Result<Vec<Value>> {
    let cf = Cf::new(None)?;
    let zid = zone(&cf, h)?;
    let mut recs = cf.records(&zid)?;
    recs.sort_by_key(|r| r["name"].as_str().unwrap_or("").to_string());
    Ok(recs
        .iter()
        .map(|r| json!({"type": r["type"], "name": r["name"], "content": r["content"], "proxied": r.get("proxied").and_then(Value::as_bool).unwrap_or(false)}))
        .collect())
}

pub fn check(h: &Host) -> Result<IndexMap<String, bool>> {
    let recs = list_records(h)?;
    let want = format!("*.{}", h.private_zone);
    let ok = recs.iter().any(|r| r["name"].as_str() == Some(&want) && r["content"].as_str() == Some(&h.tailscale_ip));
    Ok(IndexMap::from([(want, ok)]))
}

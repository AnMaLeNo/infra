//! Génère compose.override.yml (+ compose complet pour `static`) et le .env déchiffré d'un projet ou d'une preview.
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_yaml::{Mapping, Value as Y};

use crate::models::{Host, Project};
use crate::util::{dump_dotenv, parse_dotenv, write_private, Cmd};
use crate::{config, paths, secrets};

/// Un déploiement concret : le projet lui-même ou une de ses previews.
#[derive(Debug, Clone)]
pub struct Target {
    pub name: String,
    pub project: Project,
    pub host: Host,
    pub pr: Option<u64>,
}

/// (suffixe du routeur/service Traefik, domaine, expose, port interne)
pub struct Route {
    pub suffix: String,
    pub domain: String,
    pub expose: String,
    pub port: Option<u16>,
}

impl Target {
    pub fn new(name: &str, project: Project, host: Host, pr: Option<u64>) -> Self {
        Target { name: name.to_string(), project, host, pr }
    }

    pub fn compose_project(&self) -> String {
        match self.pr {
            None => self.name.clone(),
            Some(pr) => format!("{}-pr-{pr}", self.name),
        }
    }

    pub fn dir(&self) -> PathBuf {
        match self.pr {
            None => paths::app_dir(&self.name),
            Some(pr) => paths::preview_dir(&self.name, pr),
        }
    }

    pub fn repo_dir(&self) -> PathBuf {
        self.dir().join("repo")
    }

    pub fn domain(&self) -> &str {
        config::domain_of(&self.name, &self.project)
    }

    pub fn fqdn(&self, zone: &str, domain: Option<&str>) -> String {
        let base = format!("{}.{zone}", domain.unwrap_or(self.domain()));
        match self.pr {
            None => base,
            Some(pr) => format!("pr-{pr}.{base}"),
        }
    }

    /// L'entrée principale puis `entries:` (les previews n'ont que l'entrée principale).
    pub fn routes(&self) -> Vec<Route> {
        let main_port = if self.project.service.as_deref() == Some("static") { Some(80) } else { self.project.port };
        let mut out = vec![Route { suffix: String::new(), domain: self.domain().to_string(), expose: self.expose().to_string(), port: main_port }];
        if self.pr.is_none() {
            for (label, exp) in &self.project.entries {
                out.push(Route {
                    suffix: format!("-{label}"),
                    domain: label.clone(),
                    expose: exp.clone(),
                    port: self.project.entry_ports.get(label).copied(),
                });
            }
        }
        out
    }

    pub fn expose(&self) -> &str {
        if self.pr.is_none() { &self.project.expose } else { &self.project.previews }
    }

    pub fn private_fqdn(&self) -> String {
        self.fqdn(&self.host.private_zone, None)
    }

    pub fn public_fqdn(&self) -> String {
        self.fqdn(&self.host.public_zone, None)
    }

    pub fn urls(&self) -> IndexMap<String, String> {
        let mut out = IndexMap::new();
        let exp = self.expose();
        if exp == "private" || exp == "public" {
            out.insert("private".into(), format!("https://{}", self.private_fqdn()));
        }
        if exp == "public" {
            out.insert("public".into(), format!("https://{}", self.public_fqdn()));
        }
        for r in self.routes().into_iter().skip(1) {
            let label = &r.suffix[1..];
            if r.expose == "private" || r.expose == "public" {
                out.insert(format!("{label} (private)"), format!("https://{}", self.fqdn(&self.host.private_zone, Some(&r.domain))));
            }
            if r.expose == "public" {
                out.insert(format!("{label} (public)"), format!("https://{}", self.fqdn(&self.host.public_zone, Some(&r.domain))));
            }
        }
        out
    }

    pub fn compose_file(&self) -> PathBuf {
        if self.project.service.as_deref() == Some("static") {
            self.dir().join("compose.static.yml")
        } else {
            self.repo_dir().join(&self.project.compose_file)
        }
    }

    pub fn override_file(&self) -> PathBuf {
        self.dir().join("compose.override.yml")
    }

    pub fn env_file(&self) -> PathBuf {
        self.dir().join(".env")
    }
}

fn ys(s: &str) -> Y {
    Y::String(s.to_string())
}

pub fn load_compose(t: &Target) -> Result<Mapping> {
    let p = t.compose_file();
    if !p.exists() {
        bail!("compose introuvable : {}", p.display());
    }
    match serde_yaml::from_str::<Y>(&fs::read_to_string(&p)?)? {
        Y::Mapping(m) => Ok(m),
        _ => Ok(Mapping::new()),
    }
}

fn service_names(services: &Mapping) -> Vec<String> {
    let mut v: Vec<String> = services.keys().filter_map(|k| k.as_str().map(str::to_string)).collect();
    v.sort();
    v
}

pub fn traefik_labels(t: &Target) -> Result<Vec<String>> {
    let routes: Vec<Route> = t.routes().into_iter().filter(|r| r.expose != "none").collect();
    if routes.is_empty() {
        return Ok(vec![]);
    }
    let mut labels = vec!["traefik.enable=true".to_string(), "traefik.docker.network=traefik-net".to_string()];
    for r in routes {
        let rn = format!("{}{}", t.compose_project(), r.suffix);
        let private_fqdn = t.fqdn(&t.host.private_zone, Some(&r.domain));
        labels.extend([
            format!("traefik.http.routers.{rn}-private.rule=Host(`{private_fqdn}`)"),
            format!("traefik.http.routers.{rn}-private.entrypoints=tailnet"),
            format!("traefik.http.routers.{rn}-private.tls=true"),
            format!("traefik.http.routers.{rn}-private.tls.certresolver=cf"),
            format!("traefik.http.routers.{rn}-private.service={rn}"),
        ]);
        if r.expose == "public" {
            let public_fqdn = t.fqdn(&t.host.public_zone, Some(&r.domain));
            if t.host.public_ingress == "cloudflare-tunnel" {
                labels.extend([
                    format!("traefik.http.routers.{rn}-public.rule=Host(`{public_fqdn}`)"),
                    format!("traefik.http.routers.{rn}-public.entrypoints=tunnel"),
                    format!("traefik.http.routers.{rn}-public.service={rn}"),
                ]);
            } else {
                labels.extend([
                    format!("traefik.http.routers.{rn}-public.rule=Host(`{public_fqdn}`)"),
                    format!("traefik.http.routers.{rn}-public.entrypoints=websecure"),
                    format!("traefik.http.routers.{rn}-public.tls=true"),
                    format!("traefik.http.routers.{rn}-public.tls.certresolver=cf"),
                    format!("traefik.http.routers.{rn}-public.service={rn}"),
                ]);
            }
        }
        let Some(port) = r.port else {
            let what = if r.suffix.is_empty() { "`port`".to_string() } else { format!("`entry_ports.{}`", &r.suffix[1..]) };
            bail!("{}: {what} manquant dans le registre (port d'écoute interne du container)", t.name);
        };
        labels.push(format!("traefik.http.services.{rn}.loadbalancer.server.port={port}"));
    }
    Ok(labels)
}

fn with_traefik_net(orig: Option<&Y>) -> Y {
    match orig {
        Some(Y::Mapping(m)) => {
            let mut m = m.clone();
            m.insert(ys("traefik-net"), Y::Null);
            Y::Mapping(m)
        }
        Some(Y::Sequence(s)) => {
            let mut s = s.clone();
            if !s.iter().any(|v| v.as_str() == Some("traefik-net")) {
                s.push(ys("traefik-net"));
            }
            Y::Sequence(s)
        }
        _ => Y::Sequence(vec![ys("default"), ys("traefik-net")]),
    }
}

/// `!reset []` : compose v2.24+ vide la liste héritée (ports publiés).
fn reset_list() -> Y {
    Y::Tagged(Box::new(serde_yaml::value::TaggedValue { tag: serde_yaml::value::Tag::new("reset"), value: Y::Sequence(vec![]) }))
}

fn svc_mut<'a>(ov_services: &'a mut Mapping, name: &str) -> &'a mut Mapping {
    ov_services.entry(ys(name)).or_insert_with(|| Y::Mapping(Mapping::new())).as_mapping_mut().unwrap()
}

pub fn build_override(t: &Target, compose: &Mapping) -> Result<Mapping> {
    let empty = Mapping::new();
    let services = compose.get("services").and_then(|v| v.as_mapping()).unwrap_or(&empty);
    if services.is_empty() {
        bail!("{}: aucun service dans {}", t.name, t.compose_file().display());
    }
    let names = service_names(services);
    let has = |n: &str| services.contains_key(ys(n));
    let mut container: Option<String> =
        if t.project.service.as_deref() == Some("static") { Some("web".into()) } else { t.project.container.clone() };
    let routed = t.expose() != "none" || t.routes().iter().skip(1).any(|r| r.expose != "none");
    if routed {
        if container.is_none() {
            if services.len() == 1 {
                container = services.keys().next().and_then(|k| k.as_str()).map(str::to_string);
            } else {
                bail!("{}: `container` manquant ; services disponibles : {names:?}", t.name);
            }
        }
        if let Some(c) = &container {
            if !has(c) {
                bail!("{}: container `{c}` absent du compose ; services : {names:?}", t.name);
            }
        }
    }

    let mut ov_services = Mapping::new();
    for k in services.keys() {
        let mut s = Mapping::new();
        s.insert(ys("ports"), reset_list());
        s.insert(ys("restart"), ys("unless-stopped"));
        if t.project.env && t.env_file().exists() {
            s.insert(ys("env_file"), Y::Sequence(vec![ys(&t.env_file().to_string_lossy())]));
        }
        ov_services.insert(k.clone(), Y::Mapping(s));
    }

    for spec in &t.project.data {
        let (svc, cpath) = spec.split_once(':').unwrap_or((spec.as_str(), ""));
        if !has(svc) || !cpath.starts_with('/') {
            bail!("{}: data `{spec}` invalide (attendu <service>:/chemin)", t.name);
        }
        let hpath = paths::data_dir().join(t.compose_project()).join(svc).join(cpath.trim_matches('/').replace('/', "_"));
        let s = svc_mut(&mut ov_services, svc);
        let vols = s.entry(ys("volumes")).or_insert_with(|| Y::Sequence(vec![]));
        if let Y::Sequence(v) = vols {
            v.push(ys(&format!("{}:{cpath}", hpath.display())));
        }
    }

    let mut networks: Option<Y> = None;
    let mut traefik_networks = Mapping::new();
    traefik_networks.insert(ys("traefik-net"), Y::Mapping([(ys("external"), Y::Bool(true))].into_iter().collect()));

    if routed {
        let c = container.as_deref().unwrap();
        let labels = traefik_labels(t)?;
        let orig_nets = services.get(ys(c)).and_then(|s| s.as_mapping()).and_then(|m| m.get("networks")).cloned();
        let s = svc_mut(&mut ov_services, c);
        s.insert(ys("labels"), Y::Sequence(labels.iter().map(|l| ys(l)).collect()));
        s.insert(ys("networks"), with_traefik_net(orig_nets.as_ref()));
        networks = Some(Y::Mapping(traefik_networks.clone()));
    }

    if !t.project.uses.is_empty() {
        let declared = config::shared()?.services;
        let unknown: Vec<&String> = t.project.uses.iter().filter(|u| !declared.contains_key(*u)).collect();
        if !unknown.is_empty() {
            bail!(
                "{}: `uses` référence des services partagés non déclarés dans shared.yaml : {unknown:?}. \
                 Ne les crée pas dans ce projet : l'utilisateur doit les déclarer (front → Services partagés).",
                t.name
            );
        }
        if t.project.consumers.is_empty() {
            bail!(
                "{}: `uses: {:?}` mais `consumers` est vide : lis le code du projet et écris via registry_set quel(s) \
                 service(s) compose consomme(nt) le service partagé et avec quelles variables d'env, ex. \
                 {{\"consumers\": {{\"api\": {{\"MQTT_HOST\": \"mqtt\", \"MQTT_PORT\": \"1883\"}}}}}}.",
                t.name, t.project.uses
            );
        }
    }
    for (svc, env) in &t.project.consumers {
        if !has(svc) {
            bail!("{}: consumers `{svc}` absent du compose ; services : {names:?}", t.name);
        }
        let orig_nets = services.get(ys(svc)).and_then(|s| s.as_mapping()).and_then(|m| m.get("networks")).cloned();
        let s = svc_mut(&mut ov_services, svc);
        let e = s.entry(ys("environment")).or_insert_with(|| Y::Mapping(Mapping::new()));
        if let Y::Mapping(m) = e {
            for (k, v) in env {
                m.insert(ys(k), ys(v));
            }
        }
        s.insert(ys("networks"), with_traefik_net(orig_nets.as_ref()));
        networks = Some(Y::Mapping(traefik_networks.clone()));
    }

    for spec in &t.project.expose_ports {
        let (port, proto) = spec.split_once('/').map(|(p, pr)| (p, Some(pr))).unwrap_or((spec.as_str(), None));
        let svc = container.clone().unwrap_or_else(|| names[0].clone());
        let svc = if has(&svc) { svc } else { services.keys().next().and_then(|k| k.as_str()).unwrap().to_string() };
        let s = svc_mut(&mut ov_services, &svc);
        let bind = format!("{}:{port}:{port}{}", t.host.tailscale_ip, proto.map(|p| format!("/{p}")).unwrap_or_default());
        s.insert(ys("ports"), Y::Sequence(vec![ys(&bind)]));
    }

    let mut ov = Mapping::new();
    ov.insert(ys("services"), Y::Mapping(ov_services));
    if let Some(n) = networks {
        ov.insert(ys("networks"), n);
    }
    Ok(ov)
}

pub fn static_compose(t: &Target) -> Mapping {
    let serve_dir = t.repo_dir().join(&t.project.static_dir);
    let serve_dir = serve_dir.canonicalize().unwrap_or(serve_dir);
    let mut web = Mapping::new();
    web.insert(ys("image"), ys("nginx:1.27-alpine"));
    web.insert(ys("restart"), ys("unless-stopped"));
    web.insert(ys("volumes"), Y::Sequence(vec![ys(&format!("{}:/usr/share/nginx/html:ro", serve_dir.display()))]));
    let mut services = Mapping::new();
    services.insert(ys("web"), Y::Mapping(web));
    let mut m = Mapping::new();
    m.insert(ys("services"), Y::Mapping(services));
    m
}

pub fn run_static_build(t: &Target) -> Result<()> {
    let Some(build) = &t.project.build else { return Ok(()) };
    let stamp = t.dir().join(".build-commit");
    let repo = t.repo_dir();
    let head = Cmd::new().quiet().cwd(&repo).run(&["git", "rev-parse", "HEAD"])?.stdout.trim().to_string();
    if stamp.exists() && fs::read_to_string(&stamp)?.trim() == head && repo.join(&t.project.static_dir).exists() {
        return Ok(());
    }
    let vol = format!("{}:/app", repo.display());
    Cmd::new().no_capture().run(&["docker", "run", "--rm", "-v", &vol, "-w", "/app", "node:22-bookworm", "sh", "-lc", build])?;
    fs::write(&stamp, format!("{head}\n"))?;
    Ok(())
}

/// Déchiffre le .env ; retourne les valeurs (pour masquage). Supprime le fichier si env: false.
pub fn render_env(t: &Target) -> Result<Vec<String>> {
    if !t.project.env {
        let _ = fs::remove_file(t.env_file());
        return Ok(vec![]);
    }
    let mut env = secrets::dotenv(&secrets::project_env_path(&t.name, false))?;
    if let Some(pr) = t.pr {
        let prev = secrets::project_env_path(&t.name, true);
        if secrets::exists(&prev) {
            env.extend(secrets::dotenv(&prev)?);
        }
        env.insert("PR_NUMBER".into(), pr.to_string());
        env.insert("PR_DOMAIN".into(), if t.expose() == "public" { t.public_fqdn() } else { t.private_fqdn() });
    }
    write_private(&t.env_file(), &dump_dotenv(&env), 0o600)?;
    Ok(env.values().cloned().collect())
}

/// Écrit .env + override (+ compose static). Retourne true si quelque chose a changé.
pub fn render(t: &Target) -> Result<bool> {
    fs::create_dir_all(t.dir())?;
    let mut changed = false;
    let before = fs::read_to_string(t.env_file()).ok();
    render_env(t)?;
    let after = fs::read_to_string(t.env_file()).ok();
    changed |= before != after;
    if t.project.service.as_deref() == Some("static") {
        run_static_build(t)?;
        changed |= write_private(&t.compose_file(), &serde_yaml::to_string(&Y::Mapping(static_compose(t)))?, 0o644)?;
    }
    let compose = load_compose(t)?;
    let ov = build_override(t, &compose)?;
    let text = format!("# Généré par `infra render` : ne pas éditer.\n{}", serde_yaml::to_string(&Y::Mapping(ov))?);
    changed |= write_private(&t.override_file(), &text, 0o644)?;
    Ok(changed)
}

static VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(:?[-?+][^}]*)?\}").unwrap());

/// Variables ${VAR} référencées par le compose sans valeur dans .env ni défaut.
pub fn missing_env_vars(t: &Target) -> Result<Vec<String>> {
    let text = fs::read_to_string(t.compose_file()).map_err(|e| anyhow!("{} : {e}", t.compose_file().display()))?;
    let env = fs::read_to_string(t.env_file()).map(|s| parse_dotenv(&s)).unwrap_or_default();
    let mut missing: Vec<String> = VAR_RE
        .captures_iter(&text)
        .filter(|c| {
            let var = &c[1];
            let default = c.get(2).map(|m| m.as_str());
            !env.contains_key(var) && !default.is_some_and(|d| d.starts_with(":-") || d.starts_with('-'))
        })
        .map(|c| c[1].to_string())
        .collect();
    missing.sort();
    missing.dedup();
    Ok(missing)
}

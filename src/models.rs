//! Schéma de host.yaml, registry.yaml et shared.yaml. Mêmes champs et mêmes défauts que la version Python :
//! les fichiers du Pi se relisent tels quels, et les empreintes « modifié depuis le dernier run » restent valables.
use anyhow::{bail, Result};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub static NAME_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap());

pub const EXPOSES: [&str; 3] = ["none", "private", "public"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cloudflare {
    pub zone_id: String,
    pub account_id: String,
    pub tunnel_id: String,
}

/// Paramètres de `claude -p`, communs à tous les runs de l'hôte (réglables dans le front).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Llm {
    pub model: String,
    pub effort: String,
}

impl Default for Llm {
    fn default() -> Self {
        Llm { model: "sonnet".into(), effort: "high".into() }
    }
}

pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Exception {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Host {
    pub name: String,
    pub tailscale_ip: String,
    pub public_zone: String,
    pub private_zone: String,
    #[serde(default = "d_ingress")]
    pub public_ingress: String,
    #[serde(default)]
    pub public_ip: Option<String>,
    /// IP LAN de l'hôte (clients sans Tailscale : ESP, autres machines) ; requis par `listen: [lan]`
    #[serde(default)]
    pub lan_ip: Option<String>,
    #[serde(default)]
    pub cloudflare: Cloudflare,
    #[serde(default)]
    pub llm: Llm,
    #[serde(default = "d_stacks")]
    pub stacks: Vec<String>,
    #[serde(default)]
    pub exceptions: Vec<Exception>,
    #[serde(default)]
    pub notes: String,
}

fn d_ingress() -> String {
    "cloudflare-tunnel".into()
}
fn d_stacks() -> Vec<String> {
    vec!["traefik".into(), "cloudflared".into(), "ui".into()]
}

impl Host {
    pub fn validate(&self) -> Result<()> {
        if !["cloudflare-tunnel", "direct"].contains(&self.public_ingress.as_str()) {
            bail!("public_ingress doit être cloudflare-tunnel|direct (reçu {:?})", self.public_ingress);
        }
        if self.public_ingress == "direct" && self.public_ip.as_deref().unwrap_or("").is_empty() {
            bail!("public_ingress: direct requiert public_ip");
        }
        if !EFFORTS.contains(&self.llm.effort.as_str()) {
            bail!("llm.effort doit être low|medium|high|xhigh|max (reçu {:?})", self.llm.effort);
        }
        Ok(())
    }
}

/// Champs du *comment*, déterminés et écrits par le LLM pendant le run (registry_set), jamais saisis
/// par l'utilisateur ; ils n'entrent pas dans le hash « modifié depuis le dernier run ».
pub const LLM_DECIDED: [&str; 8] =
    ["container", "port", "compose_file", "build", "static_dir", "consumers", "entry_ports", "pr_proposed"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub repo: String,
    #[serde(default = "d_main")]
    pub r#ref: String,
    #[serde(default = "d_true")]
    pub enabled: bool,
    /// → skills/service/<service> ; None = le LLM choisit (compose|static) et l'écrit
    #[serde(default)]
    pub service: Option<String>,
    /// → skills/expose/<expose>  (none|private|public|custom)
    #[serde(default = "d_private")]
    pub expose: String,
    /// défaut : nom du projet
    #[serde(default)]
    pub domain: Option<String>,
    /// (LLM) service compose qui reçoit le trafic
    #[serde(default)]
    pub container: Option<String>,
    /// (LLM) port d'écoute interne
    #[serde(default)]
    pub port: Option<u16>,
    /// none|private|public
    #[serde(default = "d_none")]
    pub previews: String,
    /// services partagés consommés (shared.yaml)
    #[serde(default)]
    pub uses: Vec<String>,
    /// secrets/projects/<P>.env
    #[serde(default)]
    pub env: bool,
    /// "1883/tcp" → lié à tailscale_ip
    #[serde(default)]
    pub expose_ports: Vec<String>,
    /// "<svc>:<chemin container>" → /srv/data/<P>/<svc>/...
    #[serde(default)]
    pub data: Vec<String>,
    /// (LLM)
    #[serde(default = "d_compose_file")]
    pub compose_file: String,
    /// (LLM) service static : commande de build
    #[serde(default)]
    pub build: Option<String>,
    /// (LLM) service static : dossier à servir
    #[serde(default = "d_dot")]
    pub static_dir: String,
    /// (LLM) service compose → variables d'env vers les `uses`, ex. {"api": {"MQTT_HOST": "mqtt"}}
    #[serde(default)]
    pub consumers: IndexMap<String, IndexMap<String, String>>,
    /// Points d'entrée supplémentaires du même `container` : label → expose (none|private|public)
    /// → hostname <label>.<zone>, port constaté par le LLM dans `entry_ports`
    #[serde(default)]
    pub entries: IndexMap<String, String>,
    /// (LLM) label → port interne
    #[serde(default)]
    pub entry_ports: IndexMap<String, u16>,
    /// (LLM) PR ouverte sur le repo du projet pour le dockeriser ; `ref` pointe sur sa branche tant qu'elle n'est pas mergée
    #[serde(default)]
    pub pr_proposed: Option<String>,
    #[serde(default)]
    pub legacy: Option<String>,
    #[serde(default)]
    pub notes: String,
}

fn d_main() -> String {
    "main".into()
}
fn d_true() -> bool {
    true
}
fn d_private() -> String {
    "private".into()
}
fn d_none() -> String {
    "none".into()
}
fn d_compose_file() -> String {
    "docker-compose.yml".into()
}
fn d_dot() -> String {
    ".".into()
}

fn one_label(v: &str) -> bool {
    !v.contains('.') && NAME_RE.is_match(v)
}

impl Project {
    pub fn template(repo: &str) -> Project {
        serde_json::from_value(serde_json::json!({ "repo": repo })).unwrap()
    }

    pub fn validate(&self) -> Result<()> {
        for (label, exp) in &self.entries {
            if !one_label(label) {
                bail!("entries: `{label}` doit être un seul label [a-z0-9-]");
            }
            if !EXPOSES.contains(&exp.as_str()) {
                bail!("entries.{label}: expose doit être none|private|public (reçu {exp:?})");
            }
        }
        if let Some(d) = &self.domain {
            if !one_label(d) {
                bail!("domain doit être un seul label [a-z0-9-]");
            }
        }
        Ok(())
    }

    /// `model_dump()` : tous les champs, dans l'ordre de déclaration.
    pub fn dump(&self) -> Map<String, Value> {
        to_map(self)
    }

    /// L'entrée sans les champs décidés par le LLM : ce que l'utilisateur a voulu.
    pub fn intent(&self) -> Map<String, Value> {
        let mut m = self.dump();
        m.retain(|k, _| !LLM_DECIDED.contains(&k.as_str()));
        m
    }

    /// Les constats écrits par le LLM (valeurs non par défaut seulement).
    pub fn decided(&self) -> Map<String, Value> {
        let d = exclude_defaults(self.dump(), Project::template("").dump());
        LLM_DECIDED.iter().filter_map(|k| d.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
    }

    /// `model_dump(exclude_defaults=True)`.
    pub fn dump_non_default(&self) -> Map<String, Value> {
        let mut defaults = Project::template("").dump();
        defaults.remove("repo");
        exclude_defaults(self.dump(), defaults)
    }
}

pub const SHARED_LLM_DECIDED: [&str; 3] = ["image", "container", "compose_file"];

/// Service partagé entre projets (shared.yaml) : broker MQTT, base de données… Un seul exemplaire par hôte,
/// joignable par son nom sur traefik-net ; l'intention (kind, listen, port, auth) est humaine, le reste est constaté.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Shared {
    /// ce que c'est : mqtt-broker, postgres… (libre, sert au LLM et à l'humain)
    pub kind: String,
    #[serde(default = "d_true")]
    pub enabled: bool,
    /// IPs d'écoute sur l'hôte ; jamais 0.0.0.0
    #[serde(default = "d_listen")]
    pub listen: Vec<String>,
    /// port publié sur ces IPs ; None = joignable seulement par les conteneurs (traefik-net)
    #[serde(default)]
    pub port: Option<u16>,
    /// anonymous | users (identifiants dans secrets/shared/<nom>.env)
    #[serde(default = "d_anonymous")]
    pub auth: String,
    /// données persistantes sous /srv/shared/<nom>/data
    #[serde(default = "d_true")]
    pub data: bool,
    /// (LLM) image:tag retenue
    #[serde(default)]
    pub image: Option<String>,
    /// (LLM) nom du conteneur
    #[serde(default)]
    pub container: Option<String>,
    /// (LLM) dans <config>/shared/<nom>/
    #[serde(default = "d_compose_file")]
    pub compose_file: String,
    #[serde(default)]
    pub notes: String,
}

fn d_listen() -> Vec<String> {
    vec!["tailnet".into()]
}
fn d_anonymous() -> String {
    "anonymous".into()
}

impl Shared {
    pub fn template(kind: &str) -> Shared {
        serde_json::from_value(serde_json::json!({ "kind": kind })).unwrap()
    }

    /// Dédoublonne `listen` (validateur Python) et vérifie les valeurs.
    pub fn normalize(&mut self) -> Result<()> {
        let mut seen = Vec::new();
        for l in &self.listen {
            if !["lan", "tailnet"].contains(&l.as_str()) {
                bail!("listen: valeurs possibles lan|tailnet (reçu {l:?})");
            }
            if !seen.contains(l) {
                seen.push(l.clone());
            }
        }
        self.listen = seen;
        Ok(())
    }

    pub fn dump(&self) -> Map<String, Value> {
        to_map(self)
    }

    pub fn intent(&self) -> Map<String, Value> {
        let mut m = self.dump();
        m.retain(|k, _| !SHARED_LLM_DECIDED.contains(&k.as_str()));
        m
    }

    pub fn decided(&self) -> Map<String, Value> {
        let d = exclude_defaults(self.dump(), Shared::template("").dump());
        SHARED_LLM_DECIDED.iter().filter_map(|k| d.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
    }

    pub fn dump_non_default(&self) -> Map<String, Value> {
        let mut defaults = Shared::template("").dump();
        defaults.remove("kind");
        exclude_defaults(self.dump(), defaults)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Registry {
    pub projects: IndexMap<String, Project>,
}

impl Registry {
    pub fn validate(&self) -> Result<()> {
        let bad: Vec<&String> = self.projects.keys().filter(|n| !NAME_RE.is_match(n)).collect();
        if !bad.is_empty() {
            bail!("noms de projet invalides : {bad:?}");
        }
        for (n, p) in &self.projects {
            p.validate().map_err(|e| anyhow::anyhow!("{n}: {e}"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedRegistry {
    pub services: IndexMap<String, Shared>,
}

impl SharedRegistry {
    pub fn validate(&mut self) -> Result<()> {
        let bad: Vec<String> = self.services.keys().filter(|n| !NAME_RE.is_match(n)).cloned().collect();
        if !bad.is_empty() {
            bail!("noms de service partagé invalides : {bad:?}");
        }
        for (n, s) in self.services.iter_mut() {
            s.normalize().map_err(|e| anyhow::anyhow!("{n}: {e}"))?;
        }
        Ok(())
    }
}

pub fn to_map<T: Serialize>(v: &T) -> Map<String, Value> {
    match serde_json::to_value(v) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn exclude_defaults(mut m: Map<String, Value>, defaults: Map<String, Value>) -> Map<String, Value> {
    m.retain(|k, v| defaults.get(k) != Some(v));
    m
}

/// Relit une entrée enregistrée (dict JSON) dans le modèle courant ; les champs ajoutés depuis prennent leur défaut.
pub fn project_from_value(v: &Value) -> Result<Project> {
    let p: Project = serde_json::from_value(v.clone())?;
    p.validate()?;
    Ok(p)
}

pub fn shared_from_value(v: &Value) -> Result<Shared> {
    let mut s: Shared = serde_json::from_value(v.clone())?;
    s.normalize()?;
    Ok(s)
}

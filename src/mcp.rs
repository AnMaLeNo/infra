//! Serveur MCP (stdio, JSON-RPC 2.0) : expose tools.rs à Claude Code. Lancé par le runner via --mcp-config.
use std::io::{BufRead, Write};

use anyhow::Result;
use serde_json::{json, Map, Value};

use crate::tools;

pub const INSTRUCTIONS: &str = "Tools atomiques de l'outil infra : synchroniser un repo, générer l'override compose, \
calculer les labels Traefik, déchiffrer un .env, gérer le DNS Cloudflare, lire l'état. Chaque tool échoue \
clairement quand il ne s'applique pas : fais alors autrement.";

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub call: fn(&Map<String, Value>) -> Result<Value>,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn project_pr() -> Value {
    obj(json!({"project": {"type": "string"}, "pr": {"type": ["integer", "null"]}}), &["project"])
}

fn arg_str<'a>(a: &'a Map<String, Value>, k: &str) -> Result<&'a str> {
    a.get(k).and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("argument `{k}` manquant"))
}

fn arg_opt_str<'a>(a: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn arg_pr(a: &Map<String, Value>) -> Option<u64> {
    a.get("pr").and_then(Value::as_u64)
}

fn arg_bool(a: &Map<String, Value>, k: &str) -> bool {
    a.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn arg_fields(a: &Map<String, Value>) -> Result<&Map<String, Value>> {
    a.get("fields").and_then(Value::as_object).ok_or_else(|| anyhow::anyhow!("argument `fields` (objet) manquant"))
}

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "host_info",
        description: "Décrit l'hôte : host.yaml (IP Tailscale, zones DNS, mode d'ingress public, tunnel, stacks, exceptions), chemins conventionnels.",
        schema: || obj(json!({}), &[]),
        call: |_| tools::host_info(),
    },
    Tool {
        name: "registry_get",
        description: "Lit registry.yaml : l'entrée d'un projet, ou toutes.",
        schema: || obj(json!({"project": {"type": ["string", "null"]}}), &[]),
        call: |a| tools::registry_get(arg_opt_str(a, "project")),
    },
    Tool {
        name: "registry_set",
        description: "Modifie des champs d'un projet dans registry.yaml. C'est ainsi que tu consignes tes constats (`service` si absent, `container`, `port`, `compose_file`, `build`, `static_dir`), ex. {\"container\": \"web\", \"port\": 8420}. Valeur null = supprimer le champ.",
        schema: || obj(json!({"project": {"type": "string"}, "fields": {"type": "object"}}), &["project", "fields"]),
        call: |a| tools::registry_set(arg_str(a, "project")?, arg_fields(a)?),
    },
    Tool {
        name: "repo_sync",
        description: "Clone (première fois) ou met à jour le repo du projet au bon endroit (/srv/apps/<P>/repo, ou previews/pr-<N>/repo), sur `ref` du registre (ou la tête de la PR). Retourne le chemin et le commit.",
        schema: project_pr,
        call: |a| tools::repo_sync(arg_str(a, "project")?, arg_pr(a)),
    },
    Tool {
        name: "compose_override",
        description: "Génère /srv/apps/<P>/compose.override.yml (ports publiés neutralisés, restart, labels Traefik + réseau traefik-net sur `container`, bind mounts `data:`, env_file si env:true) et le .env déchiffré. Pour `service: static`, génère aussi compose.static.yml (nginx) après le `build:`. Échoue si le repo n'a pas de compose, si `container`/`port` manquent (lis le compose/Dockerfile et fixe-les d'abord via registry_set) ou si le container n'existe pas. Retourne le contenu généré, la commande `docker compose` à utiliser et les variables manquantes.",
        schema: project_pr,
        call: |a| tools::compose_override(arg_str(a, "project")?, arg_pr(a)),
    },
    Tool {
        name: "traefik_labels",
        description: "Calcule les labels Traefik voulus pour le projet (routeurs private/public selon `expose`, port) sans rien écrire. Utile pour écrire un override à la main.",
        schema: project_pr,
        call: |a| tools::traefik_labels_tool(arg_str(a, "project")?, arg_pr(a)),
    },
    Tool {
        name: "env_decrypt",
        description: "Déchiffre secrets/projects/<P>.env vers /srv/apps/<P>/.env (0600). Retourne uniquement les NOMS de variables. Supprime le fichier si env: false.",
        schema: project_pr,
        call: |a| tools::env_decrypt(arg_str(a, "project")?, arg_pr(a)),
    },
    Tool {
        name: "secrets_keys",
        description: "Noms des variables d'un secret chiffré (ex. 'projects/catacombes.env', 'infra.env'). Jamais les valeurs.",
        schema: || obj(json!({"rel": {"type": "string"}}), &["rel"]),
        call: |a| tools::secrets_keys_tool(arg_str(a, "rel")?),
    },
    Tool {
        name: "project_status",
        description: "État réel d'un projet : containers et santé, ports publiés, présence dans traefik-net, URLs attendues, commit déployé, et l'état voulu du registre.",
        schema: || obj(json!({"project": {"type": "string"}}), &["project"]),
        call: |a| tools::project_status(arg_str(a, "project")?),
    },
    Tool {
        name: "dns_ensure",
        description: "Crée/corrige les enregistrements DNS voulus pour le projet d'après `expose`/`previews` (CNAME vers le tunnel ou A vers l'IP publique ; wildcards previews) et supprime ceux devenus inutiles. Retourne ok|created|updated|deleted|conflict par nom. `conflict` = un enregistrement existe avec une autre cible : vérifie avant de relancer avec force=True.",
        schema: || obj(json!({"project": {"type": "string"}, "force": {"type": "boolean", "default": false}}), &["project"]),
        call: |a| tools::dns_ensure(arg_str(a, "project")?, arg_bool(a, "force")),
    },
    Tool {
        name: "dns_prune",
        description: "Supprime les enregistrements DNS du projet (tous, ou seulement le CNAME public).",
        schema: || obj(json!({"project": {"type": "string"}, "public_only": {"type": "boolean", "default": false}}), &["project"]),
        call: |a| tools::dns_prune(arg_str(a, "project")?, arg_bool(a, "public_only")),
    },
    Tool {
        name: "dns_list",
        description: "Tous les enregistrements de la zone publique (type, nom, cible, proxied).",
        schema: || obj(json!({}), &[]),
        call: |_| tools::dns_list(),
    },
    Tool {
        name: "stack_status",
        description: "État des stacks d'infra (traefik, cloudflared, ui, …).",
        schema: || obj(json!({}), &[]),
        call: |_| tools::stack_status(),
    },
    Tool {
        name: "host_addresses",
        description: "Adresses de l'hôte pour décider où un service écoute : IP Tailscale, IP LAN déclarée (host.yaml: lan_ip) et détectée (route par défaut), passerelle traefik-net. Un service partagé n'écoute jamais sur 0.0.0.0 : il lie explicitement ces IPs.",
        schema: || obj(json!({}), &[]),
        call: |_| tools::host_addresses(),
    },
    Tool {
        name: "shared_get",
        description: "Lit shared.yaml : un service partagé (avec `users` = projets qui le déclarent dans `uses:`, calculé) ou tous. Un service partagé n'existe qu'en un exemplaire par hôte ; s'il n'est pas déclaré ici, il ne doit pas être créé (ni dans un projet).",
        schema: || obj(json!({"name": {"type": ["string", "null"]}}), &[]),
        call: |a| tools::shared_get(arg_opt_str(a, "name")),
    },
    Tool {
        name: "shared_set",
        description: "Consigne tes constats sur un service partagé déjà déclaré (`image`, `container`, `compose_file`, `notes`). Les choix humains (kind, listen, port, auth, data) ne se changent pas ici : termine `bloqué` avec la question. Valeur null = supprimer.",
        schema: || obj(json!({"name": {"type": "string"}, "fields": {"type": "object"}}), &["name", "fields"]),
        call: |a| tools::shared_set(arg_str(a, "name")?, arg_fields(a)?),
    },
    Tool {
        name: "shared_status",
        description: "État réel d'un service partagé : conteneurs du projet compose `shared-<nom>`, sockets réellement écoutés sur le port déclaré (ss), alias `<nom>` sur traefik-net, clients TCP connectés, projets consommateurs attendus.",
        schema: || obj(json!({"name": {"type": "string"}}), &["name"]),
        call: |a| tools::shared_status(arg_str(a, "name")?),
    },
    Tool {
        name: "skills_list",
        description: "Skills disponibles (chemin → origine tool|custom).",
        schema: || obj(json!({}), &[]),
        call: |_| tools::skills_list(),
    },
    Tool {
        name: "run_report",
        description: "Clôture ton run : à appeler UNE fois, en dernier. `ok` = état cible atteint et vérifié ; `bloqué` = il manque quelque chose qu'un humain doit fournir (dis quoi dans `needs_human`), rien n'est cassé ; `échec` = tu n'as pas réussi ou tu as laissé quelque chose en mauvais état. Sans cet appel, le run compte comme échec. `summary` = ce qui a été fait, les URLs, ce que tu as dû faire à la main (candidats à devenir des tools).",
        schema: || obj(json!({"project": {"type": "string"}, "status": {"type": "string", "enum": ["ok", "bloqué", "échec"]},
                              "summary": {"type": "string"}, "needs_human": {"type": "string", "default": ""}}),
                       &["project", "status", "summary"]),
        call: |a| tools::run_report(arg_str(a, "project")?, arg_str(a, "status")?, arg_str(a, "summary")?, a.get("needs_human").and_then(Value::as_str).unwrap_or("")),
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

pub fn list_tools() -> Value {
    json!(TOOLS.iter().map(|t| json!({"name": t.name, "description": t.description, "inputSchema": (t.schema)()})).collect::<Vec<_>>())
}

/// Appelle un tool ; une erreur devient un résultat `isError` (le LLM lit le message et fait autrement).
pub fn call_tool(name: &str, args: &Map<String, Value>) -> Value {
    let Some(t) = find(name) else {
        return json!({"content": [{"type": "text", "text": format!("tool inconnu : {name}")}], "isError": true});
    };
    match (t.call)(args) {
        Ok(v) => json!({"content": [{"type": "text", "text": serde_json::to_string(&v).unwrap_or_default()}], "structuredContent": v, "isError": false}),
        Err(e) => json!({"content": [{"type": "text", "text": format!("{e}")}], "isError": true}),
    }
}

pub fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
    let result = match method {
        "initialize" => {
            let v = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2024-11-05");
            json!({"protocolVersion": v, "capabilities": {"tools": {}}, "serverInfo": {"name": "infra", "version": crate::VERSION}, "instructions": INSTRUCTIONS})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": list_tools()}),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").and_then(Value::as_object).cloned().unwrap_or_default();
            call_tool(name, &args)
        }
        m if m.starts_with("notifications/") => return None,
        _ => {
            return id.map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("méthode inconnue : {method}")}}));
        }
    };
    id.map(|id| json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Boucle stdio : une requête JSON par ligne, une réponse par ligne.
pub fn serve_stdio() -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                writeln!(out, "{}", json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("JSON invalide : {e}")}}))?;
                out.flush()?;
                continue;
            }
        };
        if let Some(resp) = handle(&req) {
            writeln!(out, "{}", serde_json::to_string(&resp)?)?;
            out.flush()?;
        }
    }
    Ok(())
}

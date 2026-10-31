mod common;

use infra::state;
use infra::{config, mcp, observations, paths, runner, tools};
use serde_json::json;

#[test]
fn mcp_tools_registered_and_jsonrpc() {
    let names: Vec<&str> = mcp::TOOLS.iter().map(|t| t.name).collect();
    for n in ["repo_sync", "compose_override", "traefik_labels", "dns_ensure", "project_status", "registry_set", "run_report"] {
        assert!(names.contains(&n), "{n}");
    }
    assert!(mcp::find("compose_override").unwrap().description.contains("Échoue si le repo n'a pas de compose"));
    let init = mcp::handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "infra");
    assert!(mcp::handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
    let list = mcp::handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), mcp::TOOLS.len());
    let unknown = mcp::handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "nope", "arguments": {}}})).unwrap();
    assert_eq!(unknown["result"]["isError"], true);
}

#[test]
fn compose_override_requires_sync() {
    let _e = common::env();
    let err = tools::compose_override("catacombes", None).unwrap_err().to_string();
    assert!(err.contains("repo_sync"), "{err}");
}

#[test]
fn compose_override_without_compose() {
    let _e = common::env();
    let repo = paths::app_dir("catacombes").join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("server.py"), "").unwrap();
    let err = tools::compose_override("catacombes", None).unwrap_err().to_string();
    assert!(err.contains("pas de docker-compose.yml"), "{err}");
}

#[test]
fn traefik_labels_tool_and_registry_set() {
    let _e = common::env();
    let out = tools::traefik_labels_tool("catacombes", None).unwrap();
    assert_eq!(out["urls"]["public"], "https://catacombes.antoine42.com");
    assert!(out["labels"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("catacombes-public.entrypoints=tunnel")));
    tools::registry_set("catacombes", json!({"port": 9999, "notes": null}).as_object().unwrap()).unwrap();
    assert_eq!(tools::registry_get(Some("catacombes")).unwrap()["catacombes"]["port"], 9999);
    assert_eq!(tools::host_info().unwrap()["host"]["name"], "pi");
    let r = mcp::call_tool("run_report", json!({"project": "catacombes", "status": "ok", "summary": "fait"}).as_object().unwrap());
    assert_eq!(r["isError"], false);
    assert_eq!(runner::read_report("catacombes")["status"], "ok");
    assert_eq!(runner::read_report("autre")["status"], "inconnu");
    let bad = mcp::call_tool("shared_set", json!({"name": "mqtt", "fields": {"port": 1}}).as_object().unwrap());
    assert!(bad["isError"] == true && bad["content"][0]["text"].as_str().unwrap().contains("choix humains"));
}

#[test]
fn dry_run_consumes_queue_and_builds_prompt() {
    let _e = common::env();
    state::enqueue("catacombes").unwrap();
    state::enqueue("catacombes").unwrap();
    assert_eq!(state::queued().unwrap(), ["catacombes"]);
    let rec = runner::process_one(true).unwrap().unwrap();
    assert_eq!(rec["project"], "catacombes");
    assert_eq!(rec["dry"], true);
    assert!(rec["prompt"].as_str().unwrap().contains("run_report(project='catacombes'"));
    assert!(rec["prompt"].as_str().unwrap().contains("expose/public"));
    assert!(rec.get("git").is_none());
    assert!(state::queued().unwrap().is_empty() && state::running().is_none());
    assert!(runner::process_one(true).unwrap().is_none());
    state::enqueue("shared.mqtt").unwrap();
    let rec = runner::process_one(true).unwrap().unwrap();
    assert_eq!(rec["project"], "shared.mqtt");
    assert!(rec["prompt"].as_str().unwrap().contains("SERVICE PARTAGÉ `mqtt`"));
    assert!(rec["prompt"].as_str().unwrap().contains("fan-controller"));
    assert_eq!(rec["skills"], json!(["shared-service"]));
    let p = runner::build_prompt("unused", Some(&config::project("unused").unwrap()), None).unwrap();
    assert!(p.contains("`service` n'est pas fixé"));
}

#[test]
fn observe_tool_records_for_card_and_lan_ip_is_a_list() {
    let _e = common::env();
    // host.yaml d'exemple : lan_ip en liste ; une chaîne seule reste acceptée
    let h = config::host().unwrap();
    assert_eq!(h.lan_ip, vec!["192.168.1.100", "192.168.1.101"]);
    let one: infra::models::Host = serde_yaml::from_str("name: x\ntailscale_ip: 100.0.0.1\npublic_zone: a\nprivate_zone: b\nlan_ip: 10.0.0.5\n").unwrap();
    assert_eq!(one.lan_ip, vec!["10.0.0.5"]);
    let none: infra::models::Host = serde_yaml::from_str("name: x\ntailscale_ip: 100.0.0.1\npublic_zone: a\nprivate_zone: b\nlan_ip: null\n").unwrap();
    assert!(none.lan_ip.is_empty());
    let addr = tools::host_addresses().unwrap();
    assert_eq!(addr["lan_ip"].as_array().unwrap().len(), 2);
    assert!(addr.get("lan_ip_missing").is_some() && addr.get("host_ips").is_some());
    // shared_status : `lan` → une adresse d'écoute attendue par IP déclarée
    let st = tools::shared_status("mqtt").unwrap();
    let wanted: Vec<&str> = st["listen_wanted"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(wanted.contains(&"192.168.1.100:1883") && wanted.contains(&"192.168.1.101:1883"), "{wanted:?}");

    // observe : cible connue (projet ou shared.<S>), texte non vide ; visible via observations::list, effaçable
    let bad = mcp::call_tool("observe", json!({"project": "inconnu", "text": "x"}).as_object().unwrap());
    assert!(bad["isError"] == true && bad["content"][0]["text"].as_str().unwrap().contains("cible inconnue"));
    std::env::set_var("INFRA_RUN", "2026-01-01T000000Z-shared.mqtt");
    let r = mcp::call_tool("observe", json!({"project": "shared.mqtt", "text": "l'ESP 192.168.1.78 ne répond pas au ping"}).as_object().unwrap());
    std::env::remove_var("INFRA_RUN");
    assert_eq!(r["isError"], false, "{r}");
    mcp::call_tool("observe", json!({"project": "catacombes", "text": "reste /tmp/x à supprimer"}).as_object().unwrap());
    let l = observations::list("shared.mqtt");
    assert_eq!(l.len(), 1);
    assert_eq!(l[0]["source"], "observe");
    assert_eq!(l[0]["run"], "2026-01-01T000000Z-shared.mqtt");
    assert!(l[0]["text"].as_str().unwrap().contains("ESP"));
    assert_eq!(observations::list("catacombes").len(), 1);
    assert!(paths::runs_dir().join("observations/shared.mqtt.jsonl").exists());
    observations::clear("shared.mqtt").unwrap();
    assert!(observations::list("shared.mqtt").is_empty());
    assert_eq!(observations::list("catacombes").len(), 1);
    assert!(observations::add("x/y", "t", "observe", None, None).is_err());
    assert!(observations::add("catacombes", "   ", "observe", None, None).is_err());
}

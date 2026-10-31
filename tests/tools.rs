mod common;

use infra::state;
use infra::{config, mcp, paths, runner, tools};
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

mod common;

use infra::state::{self, Entry};
use infra::{config, history};
use serde_json::{json, Value};

fn fields(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn hashes_identical_to_python_version() {
    let _e = common::env();
    let reg = config::registry().unwrap();
    let h = |n: &str| state::hash(Some(&Entry::Project(reg.projects[n].clone())));
    assert_eq!(h("catacombes"), "327d03d6f939fe1b");
    assert_eq!(h("fan-controller"), "2620bbb66449ba3f");
    assert_eq!(h("site-client"), "1ba94d42734b449d");
    assert_eq!(h("unused"), "16a3dc568671a148");
    assert_eq!(h("immich"), "ca2e96d5a8ca5eb5");
    let mqtt = config::shared_service("mqtt").unwrap();
    assert_eq!(state::hash(Some(&Entry::Shared(mqtt))), "52ba5bf3ea1cda3a");
    assert_eq!(state::hash(None), "absent");
}

#[test]
fn dump_decided_and_non_default_match_pydantic() {
    let _e = common::env();
    let cata = config::project("catacombes").unwrap();
    assert_eq!(Value::Object(cata.decided()), json!({"container": "web", "port": 8420}));
    let fan = config::project("fan-controller").unwrap();
    let nd = fan.dump_non_default();
    let mut keys: Vec<&String> = nd.keys().collect();
    keys.sort();
    assert_eq!(keys, ["consumers", "container", "env", "port", "repo", "service", "uses"]);
    assert_eq!(fan.dump().len(), 22);
    let mqtt = config::shared_service("mqtt").unwrap();
    assert_eq!(mqtt.decided().len(), 0);
    assert_eq!(mqtt.listen, ["lan", "tailnet"]);
}

#[test]
fn notes_do_not_mark_project_changed_and_changed_fields() {
    let _e = common::env();
    let mut st = state::load_state();
    let prj = Entry::Project(config::project("catacombes").unwrap());
    assert!(state::changed_fields(&prj, st.get("catacombes")).is_empty());
    state::mark_applied(&mut st, "catacombes", Some(&prj), true, Some("ok")).unwrap();
    let before = state::hash(Some(&prj));
    config::set_project_fields("catacombes", "ui", &fields(json!({"notes": "constat de Claude après le run"}))).unwrap();
    let now = Entry::Project(config::project("catacombes").unwrap());
    assert_eq!(state::hash(Some(&now)), before);
    assert!(state::changed_fields(&now, st.get("catacombes")).is_empty());
    assert!(!state::changed_since_last_run(&st).unwrap().contains(&"catacombes".to_string()));
    config::set_project_fields("catacombes", "ui", &fields(json!({"domain": "cata", "previews": "private", "notes": "autre"}))).unwrap();
    let now = Entry::Project(config::project("catacombes").unwrap());
    assert_ne!(state::hash(Some(&now)), before);
    assert_eq!(state::changed_fields(&now, st.get("catacombes")), ["domain", "previews"]);
    assert!(state::changed_since_last_run(&st).unwrap().contains(&"catacombes".to_string()));
    let hist = history::for_project("catacombes").unwrap();
    assert_eq!(hist.len(), 2);
    assert_eq!(hist[1]["entry"]["domain"], "cata");
}

#[test]
fn shared_notes_ignored_and_changed_fields() {
    let _e = common::env();
    let mut st = state::load_state();
    let svc = Entry::Shared(config::shared_service("mqtt").unwrap());
    state::mark_applied(&mut st, "shared.mqtt", Some(&svc), true, Some("ok")).unwrap();
    config::set_shared_fields("mqtt", "ui", &fields(json!({"notes": "2 ESP sur le LAN"}))).unwrap();
    assert!(!state::changed_since_last_run(&state::load_state()).unwrap().contains(&"shared.mqtt".to_string()));
    config::set_shared_fields("mqtt", "ui", &fields(json!({"port": 1884}))).unwrap();
    let now = Entry::Shared(config::shared_service("mqtt").unwrap());
    assert_eq!(state::changed_fields(&now, st.get("shared.mqtt")), ["port"]);
    assert!(config::remove_shared("mqtt", "ui").is_err(), "encore utilisé par fan-controller");
}

#[test]
fn registry_edits_preserve_other_keys_and_validate() {
    let e = common::env();
    config::set_project_fields("catacombes", "ui", &fields(json!({"domain": null, "enabled": false}))).unwrap();
    let text = std::fs::read_to_string(e.config().join("registry.yaml")).unwrap();
    assert!(text.contains("site-client:") && !text.contains("domain: catacombes"));
    assert!(config::set_project_fields("catacombes", "ui", &fields(json!({"domain": "a.b"}))).is_err());
    assert!(config::set_project_fields("Bad Name", "ui", &fields(json!({"repo": "x"}))).is_err());
    config::remove_project("unused", "ui").unwrap();
    assert!(config::project("unused").is_err());
    assert_eq!(history::for_project("unused").unwrap()[0]["entry"], Value::Null);
    let llm = config::set_llm(&fields(json!({"model": "opus", "effort": null}))).unwrap();
    assert_eq!((llm.model.as_str(), llm.effort.as_str()), ("opus", "high"));
    assert!(config::set_llm(&fields(json!({"effort": "ultra"}))).is_err());
}

#[test]
fn queue_and_running() {
    let _e = common::env();
    assert!(state::queued().unwrap().is_empty());
    state::enqueue("catacombes").unwrap();
    state::enqueue("shared.mqtt").unwrap();
    state::enqueue("catacombes").unwrap();
    assert_eq!(state::queued().unwrap(), ["catacombes", "shared.mqtt"]);
    assert_eq!(state::running(), None);
    state::set_running(Some("catacombes")).unwrap();
    assert_eq!(state::running().as_deref(), Some("catacombes"));
    state::set_running(None).unwrap();
    assert_eq!(state::running(), None);
}

#[test]
fn py_json_matches_python_dumps() {
    let v = json!({"b": [1, "é", true, null], "a": {"z": 1.5, "y": "ligne\n\"x\""}});
    assert_eq!(
        infra::util::py_json_sorted(&v),
        r#"{"a": {"y": "ligne\n\"x\"", "z": 1.5}, "b": [1, "\u00e9", true, null]}"#
    );
}

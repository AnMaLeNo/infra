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
    let host = config::host().unwrap();
    let h = |n: &str| state::hash(Some(&Entry::Project(reg.projects[n].clone())));
    let deps = |n: &str| Value::Object(state::host_deps(&Entry::Project(reg.projects[n].clone()), &host));
    // sans dépendance à l'hôte (expose none) : l'empreinte ne dépend que de l'entrée
    assert_eq!(deps("unused"), json!({}));
    assert_eq!(h("unused"), "5fe5e1425db42fd1");
    // avec : l'empreinte inclut les valeurs de host.yaml dont dépend le rendu (stable tant qu'elles ne bougent pas)
    assert_eq!(deps("catacombes"), json!({"public_zone": "antoine42.com", "public_ingress": "cloudflare-tunnel"}));
    assert_eq!(h("catacombes"), "589049b6829538f4");
    assert_eq!(deps("fan-controller"), json!({"private_zone": "pi.antoine42.com", "tailscale_ip": "100.72.6.43"}));
    assert_eq!(h("fan-controller"), "984de928478d8e80");
    assert_eq!(h("site-client"), "a8fb4a28b54b93bd");
    assert_eq!(h("immich"), "af00563d638070e5");
    let mqtt = config::shared_service("mqtt").unwrap();
    assert_eq!(
        Value::Object(state::host_deps(&Entry::Shared(mqtt.clone()), &host)),
        json!({"lan_ip": ["192.168.1.100", "192.168.1.101"], "tailscale_ip": "100.72.6.43"})
    );
    assert_eq!(state::hash(Some(&Entry::Shared(mqtt))), "0da7550be9fd7617");
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
    assert_eq!(fan.dump().len(), 21);
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

#[test]
fn cli_set_parses_list_and_dict() {
    let _e = common::env();
    let kv = infra::cli::parse_kv(&["entries={fan-admin: public}".to_string(), "data=[api:/data]".into(), "port=8080".into(), "notes=".into()]);
    assert_eq!(Value::Object(kv.clone()), json!({"entries": {"fan-admin": "public"}, "data": ["api:/data"], "port": 8080, "notes": null}));
    config::set_project_fields("fan-controller", "cli", &kv).unwrap();
    let prj = config::project("fan-controller").unwrap();
    assert_eq!(prj.entries.get("fan-admin").map(String::as_str), Some("public"));
    assert_eq!(prj.data, ["api:/data"]);
    assert_eq!(prj.port, Some(8080));
}

#[test]
fn host_fields_the_entry_depends_on_mark_it_changed_and_rehash_recomputes() {
    let _e = common::env();
    let mut st = state::load_state();
    let svc = Entry::Shared(config::shared_service("mqtt").unwrap());
    state::mark_applied(&mut st, "shared.mqtt", Some(&svc), true, Some("ok")).unwrap();
    let cata = Entry::Project(config::project("catacombes").unwrap());
    state::mark_applied(&mut st, "catacombes", Some(&cata), true, Some("ok")).unwrap();
    assert_eq!(st["shared.mqtt"]["host"]["lan_ip"], json!(["192.168.1.100", "192.168.1.101"]));
    // seuls mqtt et catacombes ont un run ici ; les autres cibles sont « jamais lancées », donc toujours listées
    let changed = || {
        let all = state::changed_since_last_run(&state::load_state()).unwrap();
        all.into_iter().filter(|n| n == "shared.mqtt" || n == "catacombes").collect::<Vec<_>>()
    };
    assert!(changed().is_empty());

    // le modèle LLM ou une note de l'hôte : aucune cible ne bouge
    let host_yaml = infra::paths::config_dir().join("host.yaml");
    let orig = std::fs::read_to_string(&host_yaml).unwrap();
    std::fs::write(&host_yaml, orig.replace("model: sonnet", "model: opus").replace("notes: \"\"", "notes: \"maj\"")).unwrap();
    assert!(changed().is_empty(), "{:?}", changed());

    // lan_ip change : seul le broker (listen lan + port) est « modifié », champ nommé host.lan_ip
    std::fs::write(&host_yaml, orig.replace("lan_ip: [192.168.1.100, 192.168.1.101]", "lan_ip: 192.168.1.101")).unwrap();
    assert_eq!(changed(), ["shared.mqtt"]);
    let now = Entry::Shared(config::shared_service("mqtt").unwrap());
    assert_eq!(state::changed_fields(&now, st.get("shared.mqtt")), ["host.lan_ip"]);
    assert!(state::changed_fields(&Entry::Project(config::project("catacombes").unwrap()), st.get("catacombes")).is_empty());

    // zone publique : catacombes (public) bouge, pas le broker
    std::fs::write(&host_yaml, orig.replace("public_zone: antoine42.com", "public_zone: autre.fr")).unwrap();
    assert_eq!(changed(), ["catacombes"]);
    let now = Entry::Project(config::project("catacombes").unwrap());
    assert_eq!(state::changed_fields(&now, st.get("catacombes")), ["host.public_zone"]);

    // un état d'avant cette version (sans `host`) : pas de faux host.* ; rehash le complète et remet tout au vert
    let mut old = state::load_state();
    old["catacombes"].as_object_mut().unwrap().remove("host");
    old["catacombes"]["hash"] = json!("ancienne-formule");
    state::save_state(&old).unwrap();
    assert!(changed().contains(&"catacombes".to_string()));
    assert!(state::changed_fields(&now, old.get("catacombes")).is_empty());
    let mut st2 = state::load_state();
    let done = state::rehash(&mut st2).unwrap();
    assert!(done.contains(&"catacombes".to_string()) && done.contains(&"shared.mqtt".to_string()));
    assert_eq!(st2["catacombes"]["host"]["public_zone"], "autre.fr");
    assert_eq!(st2["catacombes"]["ok"], true);
    assert!(changed().is_empty(), "{:?}", changed());
    std::fs::write(&host_yaml, orig).unwrap();
}

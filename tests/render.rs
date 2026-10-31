mod common;

use infra::models::Project;
use infra::render::{build_override, traefik_labels, Target};
use infra::state::{self, Entry};
use infra::{config, skills};
use serde_json::{json, Value};
use serde_yaml::{Mapping, Value as Y};

fn compose() -> Mapping {
    serde_yaml::from_str(
        r#"
services:
  frontend: {image: x, ports: ["18081:80"]}
  api: {build: ., ports: ["3000:3000"], networks: [back]}
  db: {image: postgres, volumes: ["pgdata:/var/lib/postgresql/data"]}
networks: {back: {}}
volumes: {pgdata: {}}
"#,
    )
    .unwrap()
}

fn svc<'a>(ov: &'a Mapping, name: &str) -> &'a Mapping {
    ov["services"][name].as_mapping().unwrap()
}

fn fields(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn private_override() {
    let _e = common::env();
    let (h, reg) = (config::host().unwrap(), config::registry().unwrap());
    let t = Target::new("fan-controller", reg.projects["fan-controller"].clone(), h, None);
    let ov = build_override(&t, &compose()).unwrap();
    let text = serde_yaml::to_string(&Y::Mapping(ov.clone())).unwrap();
    assert_eq!(text.matches("!reset []").count(), 3);
    let fe = svc(&ov, "frontend");
    let nets: Vec<&str> = fe["networks"].as_sequence().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(nets.contains(&"traefik-net") && nets.contains(&"default"));
    let labels: Vec<&str> = fe["labels"].as_sequence().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(labels.contains(&"traefik.http.routers.fan-controller-private.rule=Host(`fan-controller.pi.antoine42.com`)"));
    assert!(!labels.iter().any(|l| l.contains("-public.")));
    assert!(labels.contains(&"traefik.http.services.fan-controller.loadbalancer.server.port=80"));
    assert!(!svc(&ov, "api").contains_key("labels"));
    let api_env = svc(&ov, "api")["environment"].as_mapping().unwrap();
    assert_eq!(api_env["MQTT_HOST"].as_str(), Some("mqtt"));
}

#[test]
fn public_tunnel_labels_and_urls() {
    let _e = common::env();
    let (h, reg) = (config::host().unwrap(), config::registry().unwrap());
    let t = Target::new("catacombes", reg.projects["catacombes"].clone(), h, None);
    let labels = traefik_labels(&t).unwrap();
    assert!(labels.contains(&"traefik.http.routers.catacombes-public.entrypoints=tunnel".to_string()));
    assert!(labels.contains(&"traefik.http.routers.catacombes-private.entrypoints=tailnet".to_string()));
    assert_eq!(
        json!(t.urls()),
        json!({"private": "https://catacombes.pi.antoine42.com", "public": "https://catacombes.antoine42.com"})
    );
}

#[test]
fn preview_target() {
    let _e = common::env();
    let (h, reg) = (config::host().unwrap(), config::registry().unwrap());
    let t = Target::new("site-client", reg.projects["site-client"].clone(), h, Some(12));
    assert_eq!(t.compose_project(), "site-client-pr-12");
    assert_eq!(t.public_fqdn(), "pr-12.naturo.antoine42.com");
    assert!(traefik_labels(&t)
        .unwrap()
        .contains(&"traefik.http.routers.site-client-pr-12-public.rule=Host(`pr-12.naturo.antoine42.com`)".to_string()));
}

#[test]
fn none_has_no_labels() {
    let _e = common::env();
    let (h, reg) = (config::host().unwrap(), config::registry().unwrap());
    let t = Target::new("unused", reg.projects["unused"].clone(), h, None);
    let c: Mapping = serde_yaml::from_str("services: {daemon: {image: x, ports: ['1:1']}}").unwrap();
    let ov = build_override(&t, &c).unwrap();
    assert!(!svc(&ov, "daemon").contains_key("labels") && !ov.contains_key("networks"));
}

#[test]
fn skills_resolution() {
    let _e = common::env();
    let reg = config::registry().unwrap();
    assert_eq!(skills::for_project(Some(&reg.projects["site-client"])), ["service/static", "expose/public", "previews"]);
    assert_eq!(skills::for_project(Some(&reg.projects["immich"])), ["undeploy"]);
    assert_eq!(skills::for_project(Some(&reg.projects["unused"])), ["service/compose", "service/static", "expose/none"]);
    assert!(skills::paths_for(Some(&reg.projects["catacombes"])).values().all(|p| p.is_some()));
    assert!(skills::list_all().contains_key("expose/public"));
}

#[test]
fn llm_decided_fields_do_not_mark_project_changed() {
    let _e = common::env();
    let h = |p: Project| state::hash(Some(&Entry::Project(p)));
    let before = h(config::project("catacombes").unwrap());
    config::set_project_fields("catacombes", "llm", &fields(json!({"container": "api", "port": 1234, "build": "make"}))).unwrap();
    assert_eq!(h(config::project("catacombes").unwrap()), before);
    assert_eq!(
        Value::Object(config::project("catacombes").unwrap().decided()),
        json!({"container": "api", "port": 1234, "build": "make"})
    );
    config::set_project_fields("catacombes", "ui", &fields(json!({"expose": "private"}))).unwrap();
    assert_ne!(h(config::project("catacombes").unwrap()), before);
}

#[test]
fn changed_since_last_run_all_when_never_run() {
    let _e = common::env();
    let mut st = state::load_state();
    let mut all: Vec<String> = config::registry().unwrap().projects.keys().cloned().collect();
    all.push("shared.mqtt".into());
    let mut got = state::changed_since_last_run(&st).unwrap();
    got.sort();
    all.sort();
    assert_eq!(got, all);
    for (n, p) in config::registry().unwrap().projects {
        state::mark_applied(&mut st, &n, Some(&Entry::Project(p)), true, Some("ok")).unwrap();
    }
    state::mark_applied(&mut st, "shared.mqtt", Some(&Entry::Shared(config::shared_service("mqtt").unwrap())), true, None).unwrap();
    assert!(state::changed_since_last_run(&state::load_state()).unwrap().is_empty());
    config::set_project_fields("catacombes", "ui", &fields(json!({"domain": "cata"}))).unwrap();
    assert_eq!(state::changed_since_last_run(&state::load_state()).unwrap(), ["catacombes"]);
}

#[test]
fn entries_add_routers_and_urls() {
    let _e = common::env();
    let h = config::host().unwrap();
    let prj = config::set_project_fields(
        "catacombes",
        "test",
        &fields(json!({"entries": {"catacombes-admin": "public", "catacombes-ops": "private"},
                       "entry_ports": {"catacombes-admin": 8788, "catacombes-ops": 9000}})),
    )
    .unwrap();
    let t = Target::new("catacombes", prj.clone(), h.clone(), None);
    let labels = traefik_labels(&t).unwrap();
    let has = |s: &str| labels.iter().any(|l| l == s);
    assert!(has("traefik.http.routers.catacombes-catacombes-admin-public.rule=Host(`catacombes-admin.antoine42.com`)"));
    assert!(has("traefik.http.routers.catacombes-catacombes-admin-public.service=catacombes-catacombes-admin"));
    assert!(has("traefik.http.services.catacombes-catacombes-admin.loadbalancer.server.port=8788"));
    assert!(has("traefik.http.routers.catacombes-catacombes-ops-private.rule=Host(`catacombes-ops.pi.antoine42.com`)"));
    assert!(!labels.iter().any(|l| l.contains("catacombes-ops-public")));
    let urls = t.urls();
    assert_eq!(urls["catacombes-admin (public)"], "https://catacombes-admin.antoine42.com");
    assert_eq!(urls["catacombes-ops (private)"], "https://catacombes-ops.pi.antoine42.com");
    assert!(!urls.contains_key("catacombes-ops (public)"));
    let preview = traefik_labels(&Target::new("catacombes", prj, h, Some(3))).unwrap();
    assert!(!preview.iter().any(|l| l.contains("catacombes-admin")));
}

#[test]
fn entry_without_port_is_explicit_error() {
    let _e = common::env();
    let prj = config::set_project_fields("catacombes", "test", &fields(json!({"entries": {"catacombes-admin": "public"}}))).unwrap();
    let err = traefik_labels(&Target::new("catacombes", prj, config::host().unwrap(), None)).unwrap_err();
    assert!(err.to_string().contains("entry_ports.catacombes-admin"));
}

#[test]
fn entries_validation() {
    let bad = infra::models::project_from_value(&json!({"repo": "https://github.com/x/y", "entries": {"a.b": "public"}}));
    assert!(bad.unwrap_err().to_string().contains("un seul label"));
    let bad = infra::models::project_from_value(&json!({"repo": "https://github.com/x/y", "entries": {"ok": "lan"}}));
    assert!(bad.unwrap_err().to_string().contains("none|private|public"));
    let p = infra::models::project_from_value(&json!({"repo": "https://github.com/x/y", "entries": {"ok": "public"},
        "entry_ports": {"ok": 1}, "pr_proposed": "https://github.com/x/y/pull/1"}))
    .unwrap();
    let intent = p.intent();
    assert!(!intent.contains_key("entry_ports") && !intent.contains_key("pr_proposed") && intent.contains_key("entries"));
    assert_eq!(Value::Object(p.decided()), json!({"entry_ports": {"ok": 1}, "pr_proposed": "https://github.com/x/y/pull/1"}));
}

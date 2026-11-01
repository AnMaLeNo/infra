mod common;

use infra::config;
use infra::dns::{project_names, wanted_records};
use serde_json::json;

#[test]
fn wanted() {
    let _e = common::env();
    let mut h = config::host().unwrap();
    h.cloudflare.tunnel_id = "abc".into();
    let reg = config::registry().unwrap();
    let recs = wanted_records("site-client", &reg.projects["site-client"], &h).unwrap();
    let cname = |n: &str| (n.to_string(), "CNAME".to_string(), "abc.cfargotunnel.com".to_string(), true);
    assert!(recs.contains(&cname("naturo.antoine42.com")));
    assert!(!recs.iter().any(|r| r.0.starts_with("*.") && r.0.ends_with("antoine42.com") && !r.0.contains(".pi.")), "pas de wildcard public : {recs:?}");
    let t = infra::render::Target::new("site-client", reg.projects["site-client"].clone(), h.clone(), Some(7));
    assert_eq!(infra::dns::preview_name(&t).as_deref(), Some("pr-7-naturo.antoine42.com"));
    let t0 = infra::render::Target::new("site-client", reg.projects["site-client"].clone(), h.clone(), None);
    assert_eq!(infra::dns::preview_name(&t0), None);
    assert!(wanted_records("fan-controller", &reg.projects["fan-controller"], &h).unwrap().is_empty());
    assert!(project_names("fan-controller", &reg.projects["fan-controller"], &h).contains(&"fan-controller.antoine42.com".to_string()));
}

#[test]
fn entries_public_cname() {
    let _e = common::env();
    let mut h = config::host().unwrap();
    h.cloudflare.tunnel_id = "abc".into();
    let f = json!({"entries": {"fan-admin": "public", "fan-ops": "private"}});
    let prj = config::set_project_fields("fan-controller", "test", f.as_object().unwrap()).unwrap();
    assert_eq!(
        wanted_records("fan-controller", &prj, &h).unwrap(),
        vec![("fan-admin.antoine42.com".to_string(), "CNAME".to_string(), "abc.cfargotunnel.com".to_string(), true)]
    );
    let names = project_names("fan-controller", &prj, &h);
    assert!(names.contains(&"fan-admin.antoine42.com".to_string()) && names.contains(&"fan-ops.antoine42.com".to_string()));
}

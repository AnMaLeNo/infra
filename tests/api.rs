mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use infra::{api, paths};
use serde_json::{json, Value};
use std::process::Command;
use tower::ServiceExt;

async fn call(method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
    let req = Request::builder().method(method).uri(path).header("content-type", "application/json");
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let resp = api::router().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, v)
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let o = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn init_repo(e: &common::Env) -> std::path::PathBuf {
    let cfg = paths::config_dir();
    let bare = e.root.path().join("remote.git");
    Command::new("git").args(["init", "-q", "--bare"]).arg(&bare).status().unwrap();
    git(&cfg, &["init", "-q", "-b", "main"]);
    git(&cfg, &["remote", "add", "origin", &bare.to_string_lossy()]);
    std::fs::write(cfg.join(".gitignore"), "runs/\ntool\n").unwrap();
    git(&cfg, &["add", "-A"]);
    git(&cfg, &["commit", "-q", "-m", "init"]);
    git(&cfg, &["push", "-q", "-u", "origin", "main"]);
    bare
}

#[tokio::test]
async fn projects_and_shared_views() {
    let _e = common::env();
    let (st, v) = call("GET", "/api/projects", None).await;
    assert_eq!(st, StatusCode::OK);
    let cata = &v["catacombes"];
    assert_eq!(cata["urls"]["public"], "https://catacombes.antoine42.com");
    assert!(cata["changed"] == true && cata["never_run"] == true && cata["changed_fields"] == json!([]));
    assert_eq!(cata["skills"], json!(["service/compose", "expose/public"]));
    let (_, s) = call("GET", "/api/shared", None).await;
    assert_eq!(s["mqtt"]["key"], "shared.mqtt");
    assert_eq!(s["mqtt"]["users"], json!(["fan-controller"]));
    let (_, h) = call("GET", "/api/host", None).await;
    assert_eq!(h["name"], "pi");
    assert_eq!(h["version"], infra::VERSION);
    let (st, _) = call("GET", "/", None).await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = call("POST", "/api/projects/inconnu/run", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, q) = call("POST", "/api/projects/catacombes/run", None).await;
    assert!(st == StatusCode::OK && q["project"] == "catacombes");
    let (_, r) = call("GET", "/api/runner", None).await;
    assert_eq!(r["queued"], json!(["catacombes"]));
    let (st, e) = call("PUT", "/api/projects/catacombes", Some(json!({"repo": "https://github.com/x/y", "domain": "a.b"}))).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    let (st, _) = call("PUT", "/api/settings", Some(json!({"effort": "ultra"}))).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn ui_writes_are_committed() {
    let e = common::env();
    let bare = init_repo(&e);
    let (st, r) = call("PUT", "/api/projects/catacombes", Some(json!({"repo": "https://github.com/AnMaLeNo/catacombes", "expose": "public", "domain": "cata"}))).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert!(r["git"]["committed"] == true && r["git"]["pushed"] == true, "{r}");
    let (_, r) = call("PUT", "/api/settings", Some(json!({"model": "opus", "effort": "max"}))).await;
    assert_eq!(r["git"]["committed"], true);
    let (_, r) = call("PUT", "/api/shared/pg", Some(json!({"kind": "postgres"}))).await;
    assert_eq!(r["git"]["committed"], true, "{r}");
    let (_, r) = call("DELETE", "/api/shared/pg", None).await;
    assert_eq!(r["git"]["committed"], true);
    let (st, _) = call("DELETE", "/api/shared/mqtt", None).await;
    assert_eq!(st, StatusCode::CONFLICT);
    let (_, r) = call("DELETE", "/api/projects/catacombes", None).await;
    assert_eq!(r["git"]["committed"], true);
    let log = git(&bare, &["log", "--format=%s", "main"]);
    let lines: Vec<&str> = log.lines().take(5).collect();
    assert_eq!(
        lines,
        ["ui : projet catacombes retiré du registre", "ui : service partagé pg retiré", "ui : service partagé pg déclaré/modifié", "ui : settings LLM", "ui : projet catacombes modifié"]
    );
}

#[tokio::test]
async fn editor_route_requests_the_session_of_a_run() {
    let _e = common::env();
    let runs = paths::runs_dir();
    std::fs::create_dir_all(&runs).unwrap();
    std::fs::write(runs.join("a-p.json"), r#"{"session_id": "fd3acd69-38c1-4560-9a1a-f04596739d38"}"#).unwrap();
    std::fs::write(runs.join("b-p.json"), "{}").unwrap();
    std::fs::write(runs.join("c-p.json"), "{}").unwrap();
    std::fs::write(runs.join("c-p.log"), "{\"type\": \"system\", \"session_id\": \"from-log\"}\n{\"type\": \"assistant\"}\n").unwrap();
    assert_eq!(infra::runner::session_of("c-p.json").as_deref(), Some("from-log"));
    let (st, v) = call("POST", "/api/runs/a-p.json/editor", None).await;
    assert!(st == StatusCode::OK && v == json!({"ok": true}));
    assert_eq!(std::fs::read_to_string(runs.join(".editor-open")).unwrap(), "fd3acd69-38c1-4560-9a1a-f04596739d38");
    assert_eq!(call("POST", "/api/runs/b-p.json/editor", None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(call("POST", "/api/runs/absent.json/editor", None).await.0, StatusCode::NOT_FOUND);
    let st = call("POST", "/api/runs/..%2Fx.json/editor", None).await.0;
    assert!(st == StatusCode::BAD_REQUEST || st == StatusCode::NOT_FOUND);
    let (st, text) = call("GET", "/api/runs/c-p.log/log", None).await;
    assert!(st == StatusCode::OK && text.as_str().unwrap().contains("from-log"));
}

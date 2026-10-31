//! API + UI web : édite registry.yaml, montre l'état et les runs. Ne lance pas claude (c'est le runner, via la file).
use std::collections::HashMap;

use axum::body::Body;
use axum::extract::{Path, Query};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rust_embed::Embed;
use serde_json::{json, Map, Value};

use crate::render::Target;
use crate::state::{self, Entry};
use crate::{compose, config, editor, gitsync, history, models, paths, runner, skills, VERSION};

#[derive(Embed)]
#[folder = "web/dist/"]
struct Front;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"detail": self.1}))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

type Res<T = Json<Value>> = Result<T, ApiError>;

fn unprocessable(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
}

fn not_found(msg: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, msg.to_string())
}

/// Le travail (fichiers, docker, git) est synchrone : on le sort du réacteur tokio.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, ApiError> + Send + 'static) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

fn ok<T: serde::Serialize>(v: T) -> Res {
    Ok(Json(serde_json::to_value(v).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?))
}

fn with_git(mut m: Map<String, Value>, msg: &str) -> Value {
    m.insert("git".into(), gitsync::commit_push(msg));
    Value::Object(m)
}

async fn index() -> Response {
    serve_front("index.html").await
}

async fn front_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    serve_front(path).await
}

/// Front React embarqué ; les routes inconnues (sans extension) renvoient index.html pour le routeur côté client.
async fn serve_front(path: &str) -> Response {
    let file = Front::get(path).or_else(|| if path.contains('.') { None } else { Front::get("index.html") });
    match file {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            ([(header::CONTENT_TYPE, mime.as_ref()), (header::CACHE_CONTROL, cache)], Body::from(f.data.into_owned())).into_response()
        }
        None if path == "index.html" => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            format!("<!doctype html><meta charset=utf-8><title>infra</title><p>infra {VERSION} : front non construit (web/dist vide). API : /api/projects"),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn host() -> Res {
    blocking(|| {
        let h = config::host()?;
        let mut m = models::to_map(&h);
        m.insert("version".into(), json!(VERSION));
        m.insert("config_dir".into(), json!(paths::config_dir().to_string_lossy()));
        Ok(Json(Value::Object(m)))
    })
    .await
}

/// Paramètres LLM (host.yaml → llm), communs à tous les runs.
async fn settings() -> Res {
    blocking(|| ok(config::host()?.llm)).await
}

async fn put_settings(Json(body): Json<Value>) -> Res {
    blocking(move || {
        let fields = body.as_object().ok_or_else(|| unprocessable("objet attendu"))?;
        let llm = config::set_llm(fields).map_err(unprocessable)?;
        Ok(Json(with_git(models::to_map(&llm), "ui : settings LLM")))
    })
    .await
}

fn run_state(st: Option<&Value>, entry: &Entry) -> Map<String, Value> {
    let g = |k: &str| st.and_then(|s| s.get(k)).cloned().unwrap_or(Value::Null);
    let mut m = Map::new();
    m.insert("changed".into(), json!(st.and_then(|s| s.get("hash")).and_then(Value::as_str) != Some(&state::hash(Some(entry)))));
    m.insert("never_run".into(), json!(st.and_then(|s| s.get("hash")).is_none()));
    m.insert("changed_fields".into(), json!(state::changed_fields(entry, st)));
    m.insert("last_ok".into(), g("ok"));
    m.insert("last_status".into(), g("status"));
    m.insert("last_at".into(), g("at"));
    m
}

pub fn projects_view() -> anyhow::Result<Value> {
    let (reg, h) = (config::registry()?, config::host()?);
    let st = state::load_state();
    let (queued, running) = (state::queued()?, state::running());
    let mut out = Map::new();
    for (name, prj) in &reg.projects {
        let t = Target::new(name, prj.clone(), h.clone(), None);
        let e = Entry::Project(prj.clone());
        let mut m = run_state(st.get(name), &e);
        m.insert("entry".into(), Value::Object(prj.dump()));
        m.insert("decided".into(), Value::Object(prj.decided()));
        m.insert("urls".into(), json!(t.urls()));
        m.insert("skills".into(), json!(skills::for_project(Some(prj))));
        m.insert("queued".into(), json!(queued.contains(name)));
        m.insert("running".into(), json!(running.as_deref() == Some(name)));
        out.insert(name.clone(), Value::Object(m));
    }
    Ok(Value::Object(out))
}

async fn projects() -> Res {
    blocking(|| Ok(Json(projects_view()?))).await
}

/// Bouton « Lancer Claude » : met le projet dans la file du runner. Rien ne se lance sans ça.
async fn request_run(Path(name): Path<String>) -> Res {
    blocking(move || {
        if !config::registry()?.projects.contains_key(&name) && !state::load_state().contains_key(&name) {
            return Err(not_found("projet inconnu"));
        }
        Ok(Json(state::enqueue(&name)?))
    })
    .await
}

/// Tout ce qui est arrivé à un projet : versions de sa config (runs/history) + ses runs (runs/*-<P>.json).
async fn project_history(Path(name): Path<String>) -> Res {
    blocking(move || Ok(Json(json!({"config": history::for_project(&name)?, "runs": runner::runs_for_project(&name)?})))).await
}

pub fn shared_view() -> anyhow::Result<Value> {
    let reg = config::shared()?;
    let st = state::load_state();
    let (queued, running) = (state::queued()?, state::running());
    let mut out = Map::new();
    for (name, svc) in &reg.services {
        let key = config::shared_key(name);
        let e = Entry::Shared(svc.clone());
        let mut m = run_state(st.get(&key), &e);
        m.insert("entry".into(), Value::Object(svc.dump()));
        m.insert("decided".into(), Value::Object(svc.decided()));
        m.insert("users".into(), json!(config::users_of(name)?));
        m.insert("key".into(), json!(key));
        m.insert("queued".into(), json!(queued.contains(&key)));
        m.insert("running".into(), json!(running.as_deref() == Some(key.as_str())));
        out.insert(name.clone(), Value::Object(m));
    }
    Ok(Value::Object(out))
}

/// Services partagés (shared.yaml) avec leur état de run, comme /api/projects.
async fn shared_services() -> Res {
    blocking(|| Ok(Json(shared_view()?))).await
}

async fn put_shared(Path(name): Path<String>, Json(body): Json<Value>) -> Res {
    blocking(move || {
        models::shared_from_value(&body).map_err(unprocessable)?;
        let fields = body.as_object().ok_or_else(|| unprocessable("objet attendu"))?;
        let svc = config::set_shared_fields(&name, "ui", fields).map_err(unprocessable)?;
        Ok(Json(with_git(svc.dump(), &format!("ui : service partagé {name} déclaré/modifié"))))
    })
    .await
}

async fn delete_shared(Path(name): Path<String>) -> Res {
    blocking(move || {
        config::remove_shared(&name, "ui").map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
        Ok(Json(json!({"ok": true, "git": gitsync::commit_push(&format!("ui : service partagé {name} retiré"))})))
    })
    .await
}

/// Bouton « Lancer Claude » d'un service partagé : file du runner, clé `shared.<nom>`.
async fn request_shared_run(Path(name): Path<String>) -> Res {
    blocking(move || {
        let key = config::shared_key(&name);
        if !config::shared()?.services.contains_key(&name) && !state::load_state().contains_key(&key) {
            return Err(not_found("service partagé inconnu"));
        }
        Ok(Json(state::enqueue(&key)?))
    })
    .await
}

async fn shared_history(Path(name): Path<String>) -> Res {
    blocking(move || {
        let key = config::shared_key(&name);
        Ok(Json(json!({"config": history::for_project(&key)?, "runs": runner::runs_for_project(&key)?})))
    })
    .await
}

async fn runner_state() -> Res {
    blocking(|| Ok(Json(json!({"queued": state::queued()?, "running": state::running()})))).await
}

async fn project(Path(name): Path<String>) -> Res {
    blocking(move || {
        let prj = config::project(&name).map_err(|_| not_found("projet inconnu"))?;
        let t = Target::new(&name, prj.clone(), config::host()?, None);
        // docker indisponible dans le container ?
        let status = compose::status(&t).unwrap_or_else(|e| json!({"error": e.to_string()}));
        Ok(Json(json!({"entry": prj.dump(), "urls": t.urls(), "status": status, "skills": skills::for_project(Some(&prj))})))
    })
    .await
}

async fn put_project(Path(name): Path<String>, Json(body): Json<Value>) -> Res {
    blocking(move || {
        models::project_from_value(&body).map_err(unprocessable)?;
        let fields = body.as_object().ok_or_else(|| unprocessable("objet attendu"))?;
        let prj = config::set_project_fields(&name, "ui", fields).map_err(unprocessable)?;
        Ok(Json(with_git(prj.dump(), &format!("ui : projet {name} modifié"))))
    })
    .await
}

async fn delete_project(Path(name): Path<String>) -> Res {
    blocking(move || {
        config::remove_project(&name, "ui")?;
        Ok(Json(json!({"ok": true, "git": gitsync::commit_push(&format!("ui : projet {name} retiré du registre"))})))
    })
    .await
}

async fn runs(Query(q): Query<HashMap<String, String>>) -> Res {
    let limit = q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(50);
    blocking(move || Ok(Json(json!(runner::list_runs(limit)?)))).await
}

fn check_file(file: &str, suffix: &str) -> Result<(), ApiError> {
    if file.contains('/') || file.starts_with('.') || !file.ends_with(suffix) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "nom de fichier invalide".into()));
    }
    Ok(())
}

fn get_run(file: &str) -> Result<Value, ApiError> {
    runner::get_run(file).map_err(|_| not_found("run introuvable"))
}

async fn run_detail(Path(file): Path<String>) -> Result<String, ApiError> {
    check_file(&file, ".json")?;
    blocking(move || {
        let r = get_run(&file)?;
        let mut out = r.get("result").and_then(Value::as_str).unwrap_or("").to_string();
        if let Some(err) = r.get("stderr").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            out.push_str("\n\n--- stderr ---\n");
            out.push_str(err);
        }
        Ok(out)
    })
    .await
}

/// Le run complet (prompt, entrée avant/après, résultat, coût…).
async fn run_full(Path(file): Path<String>) -> Res {
    check_file(&file, ".json")?;
    blocking(move || {
        let mut r = get_run(&file)?;
        let log = file.replace(".json", ".log");
        r["file"] = json!(file);
        r["log"] = if paths::runs_dir().join(&log).exists() { json!(log) } else { Value::Null };
        Ok(Json(r))
    })
    .await
}

/// Demande à l'éditeur (`infra editor`) d'ouvrir la session Claude de ce run ; le front ouvre ensuite l'éditeur.
async fn run_in_editor(Path(file): Path<String>) -> Res {
    check_file(&file, ".json")?;
    blocking(move || {
        get_run(&file)?;
        let session = runner::session_of(&file).ok_or_else(|| not_found("pas de session enregistrée pour ce run"))?;
        editor::request_session(&session).map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
        Ok(Json(json!({"ok": true})))
    })
    .await
}

/// Déroulé complet du run (stream-json de claude : textes, appels d'outils, résultat).
async fn run_log(Path(file): Path<String>) -> Result<String, ApiError> {
    check_file(&file, ".log")?;
    blocking(move || Ok(runner::get_log(&file))).await
}

async fn skills_list() -> Res {
    blocking(|| Ok(Json(json!(skills::list_all())))).await
}

async fn registry_raw() -> Result<String, ApiError> {
    blocking(|| Ok(std::fs::read_to_string(paths::config_dir().join("registry.yaml")).unwrap_or_default())).await
}

pub fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/host", get(host))
        .route("/api/settings", get(settings).put(put_settings))
        .route("/api/projects", get(projects))
        .route("/api/projects/{name}", get(project).put(put_project).delete(delete_project))
        .route("/api/projects/{name}/run", post(request_run))
        .route("/api/projects/{name}/history", get(project_history))
        .route("/api/shared", get(shared_services))
        .route("/api/shared/{name}", axum::routing::put(put_shared).delete(delete_shared))
        .route("/api/shared/{name}/run", post(request_shared_run))
        .route("/api/shared/{name}/history", get(shared_history))
        .route("/api/runner", get(runner_state))
        .route("/api/runs", get(runs))
        .route("/api/runs/{file}", get(run_detail))
        .route("/api/runs/{file}/full", get(run_full))
        .route("/api/runs/{file}/editor", post(run_in_editor))
        .route("/api/runs/{file}/log", get(run_log))
        .route("/api/skills", get(skills_list))
        .route("/api/registry", get(registry_raw))
        .fallback(front_file)
}

pub async fn serve(host: &str, port: u16) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("infra {VERSION} : http://{host}:{port}");
    axum::serve(listener, router()).await?;
    Ok(())
}

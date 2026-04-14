use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;

use crate::state_file::{self, SavedState};

// ============================================================================
// Shared state
// ============================================================================

struct AppState {
    config: BuilderConfig,
    saved: Mutex<SavedState>,
    cancel_flag: Arc<AtomicBool>,
    build_tx: broadcast::Sender<String>,
}

// ============================================================================
// Entry point
// ============================================================================

fn build_router(config: BuilderConfig) -> (Router, Arc<AppState>) {
    let (build_tx, _) = broadcast::channel::<String>(256);
    let state = Arc::new(AppState {
        config,
        saved: Mutex::new(SavedState::default()),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        build_tx,
    });

    let router = Router::new()
        // Read-only
        .route("/api/versions", get(get_versions))
        .route("/api/models", get(get_models))
        .route("/api/models/{id}", get(get_model))
        .route("/api/config-schema", get(get_config_schema))
        .route("/api/system-info", get(get_system_info))
        .route("/api/ollama/models", get(get_ollama_models))
        .route("/api/lmstudio/models", get(get_lmstudio_models))
        // State
        .route("/api/state", get(get_state))
        .route("/api/state/version", post(set_version))
        .route("/api/state/model", post(set_model))
        .route("/api/state/model-file", post(set_model_file))
        .route("/api/state/quantization", post(set_quantization))
        .route("/api/state/config", post(set_config))
        .route("/api/state/output", post(set_output))
        .route("/api/state/args-file", post(set_args_file))
        .route("/api/state/gpu", post(modify_gpu))
        .route("/api/state/runtime", post(set_runtime))
        .route("/api/state/zipalign", post(set_zipalign))
        .route("/api/state/reset", post(reset_state))
        .route("/api/state/args", get(get_args))
        .route("/api/state/validate", get(get_validate))
        .route("/api/state/components", get(get_components))
        // Build
        .route("/api/build", post(start_build))
        .route("/api/cancel", post(cancel_build))
        .with_state(state.clone())
        .layer(tower_http::cors::CorsLayer::permissive());

    (router, state)
}

pub async fn run(
    config: BuilderConfig,
    port: u16,
    web_dir: Option<PathBuf>,
    no_open: bool,
) -> Result<(), String> {
    let (router, _state) = build_router(config);

    let mut app = router;

    // Static file serving for web UI
    if let Some(dir) = &web_dir {
        if dir.exists() {
            tracing::info!(dir = %dir.display(), "serving static files");
            app = app.fallback_service(
                tower_http::services::ServeDir::new(dir)
                    .fallback(tower_http::services::ServeFile::new(dir.join("index.html"))),
            );
        }
    }

    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("failed to bind to {addr}: {e}"))?;

    tracing::info!("server listening on http://{addr}");

    if !no_open {
        let url = format!("http://{addr}");
        if open::that(&url).is_err() {
            tracing::warn!("could not open browser, visit {url} manually");
        }
    }

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

// ============================================================================
// Error helper
// ============================================================================

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg.into() })))
}

// ============================================================================
// Read-only endpoints
// ============================================================================

async fn get_versions(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::to_value(&st.config.versions).unwrap())
}

#[derive(Deserialize, Default)]
struct ModelQuery {
    search: Option<String>,
    tag: Option<String>,
}

async fn get_models(State(st): State<Arc<AppState>>, Query(q): Query<ModelQuery>) -> Json<Value> {
    let models = llamafile_core::search_models(
        &st.config.models,
        q.search.as_deref(),
        q.tag.as_deref(),
    );
    Json(serde_json::to_value(&models).unwrap())
}

async fn get_model(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let model = st.config.models.iter().find(|m| m.id == id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, format!("model not found: {id}")))?;
    Ok(Json(serde_json::to_value(model).unwrap()))
}

async fn get_config_schema(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::to_value(&st.config.config_schema).unwrap())
}

async fn get_system_info() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::system_info()).unwrap())
}

async fn get_ollama_models() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::ollama::get_ollama_models(None)).unwrap())
}

async fn get_lmstudio_models() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::lmstudio::get_lmstudio_models(None)).unwrap())
}

// ============================================================================
// State endpoints
// ============================================================================

async fn get_state(State(st): State<Arc<AppState>>) -> Json<Value> {
    let saved = st.saved.lock().await;
    Json(serde_json::to_value(&*saved).unwrap())
}

async fn set_version(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let group = body.get("group").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'group'"))?;
    let version = body.get("version").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'version'"))?;
    let mut saved = st.saved.lock().await;
    saved.version_group = Some(group.to_string());
    saved.version = Some(version.to_string());
    saved.custom_runtime = None;
    Ok(Json(json!("ok")))
}

async fn set_model(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let id = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'id'"))?;
    if !st.config.models.iter().any(|m| m.id == id) {
        return Err(api_err(StatusCode::NOT_FOUND, format!("model not found: {id}")));
    }
    let mut saved = st.saved.lock().await;
    saved.model_id = Some(id.to_string());
    saved.custom_model_file = None;
    saved.quantizations.clear();
    Ok(Json(json!("ok")))
}

async fn set_model_file(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?;
    let mut saved = st.saved.lock().await;
    saved.custom_model_file = Some(path.to_string());
    saved.model_id = None;
    saved.quantizations.clear();
    Ok(Json(json!("ok")))
}

async fn set_quantization(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let role = body.get("role").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'role'"))?;
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'name'"))?;
    let mut saved = st.saved.lock().await;
    saved.quantizations.insert(role.to_string(), name.to_string());
    Ok(Json(json!("ok")))
}

async fn set_config(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let key = body.get("key").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'key'"))?;
    let value = body.get("value").cloned()
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'value'"))?;
    let mut saved = st.saved.lock().await;
    saved.config_values.insert(key.to_string(), value);
    Ok(Json(json!("ok")))
}

async fn set_output(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?;
    let mut saved = st.saved.lock().await;
    saved.output_path = Some(path.to_string());
    Ok(Json(json!("ok")))
}

async fn set_args_file(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?;
    let mut saved = st.saved.lock().await;
    saved.args_file = Some(path.to_string());
    Ok(Json(json!("ok")))
}

async fn modify_gpu(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let action = body.get("action").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'action' (add|remove)"))?;
    let spec = body.get("spec").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'spec'"))?;
    // Validate the spec
    crate::commands::build::parse_gpu_spec(spec)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?;
    let mut saved = st.saved.lock().await;
    match action {
        "add" => {
            if !saved.gpu_libs.contains(&spec.to_string()) {
                saved.gpu_libs.push(spec.to_string());
            }
        }
        "remove" => saved.gpu_libs.retain(|s| s != spec),
        _ => return Err(api_err(StatusCode::BAD_REQUEST, "action must be 'add' or 'remove'")),
    }
    Ok(Json(json!("ok")))
}

async fn set_runtime(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?;
    let mut saved = st.saved.lock().await;
    saved.custom_runtime = Some(path.to_string());
    saved.version = None;
    saved.version_group = None;
    Ok(Json(json!("ok")))
}

async fn set_zipalign(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?;
    let mut saved = st.saved.lock().await;
    saved.custom_zipalign = Some(path.to_string());
    Ok(Json(json!("ok")))
}

async fn reset_state(State(st): State<Arc<AppState>>) -> Json<Value> {
    let mut saved = st.saved.lock().await;
    *saved = SavedState::default();
    Json(json!("ok"))
}

// ============================================================================
// Derived state queries (create a Builder to compute)
// ============================================================================

fn make_builder(config: &BuilderConfig, saved: &SavedState) -> Result<llamafile_core::Builder, String> {
    let cache = Cache::new(crate::get_cache_dir());
    state_file::build_from_state(saved, config.clone(), cache)
}

async fn get_args(State(st): State<Arc<AppState>>) -> ApiResult {
    let saved = st.saved.lock().await;
    let builder = make_builder(&st.config, &saved)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(Value::String(builder.resolved_args())))
}

async fn get_validate(State(st): State<Arc<AppState>>) -> ApiResult {
    let saved = st.saved.lock().await;
    let builder = make_builder(&st.config, &saved)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let msgs: Vec<Value> = builder.validate().iter().map(|m| match m {
        llamafile_core::ValidationMessage::Warning(s) => json!({"level": "warning", "message": s}),
        llamafile_core::ValidationMessage::Error(s) => json!({"level": "error", "message": s}),
    }).collect();
    Ok(Json(Value::Array(msgs)))
}

async fn get_components(State(st): State<Arc<AppState>>) -> ApiResult {
    let saved = st.saved.lock().await;
    let builder = make_builder(&st.config, &saved)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let comps: Vec<Value> = builder.resolved_components().iter().map(|c| {
        json!({
            "role": format!("{:?}", c.role),
            "source": c.source_display(),
            "zip_path": c.zip_path,
        })
    }).collect();
    Ok(Json(Value::Array(comps)))
}

// ============================================================================
// Build (SSE)
// ============================================================================

async fn start_build(
    State(st): State<Arc<AppState>>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, Json<Value>)> {
    let saved = st.saved.lock().await.clone();
    let builder = make_builder(&st.config, &saved)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    if !builder.can_build() {
        let errors: Vec<String> = builder.validate().iter().filter_map(|m| match m {
            llamafile_core::ValidationMessage::Error(s) => Some(s.clone()),
            _ => None,
        }).collect();
        return Err(api_err(StatusCode::BAD_REQUEST, errors.join("; ")));
    }

    let cancel_flag = st.cancel_flag.clone();
    cancel_flag.store(false, Ordering::Relaxed);

    let tx = st.build_tx.clone();
    let rx = st.build_tx.subscribe();

    // Spawn the build task
    tokio::spawn(async move {
        let mut builder = builder;
        builder.set_cancel_flag(cancel_flag);

        let _ = tx.send(json!({"stage": "starting"}).to_string());

        match builder.build().await {
            Ok(path) => {
                let size = tokio::fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
                let _ = tx.send(json!({
                    "stage": "complete",
                    "path": path.to_string_lossy(),
                    "size": size,
                }).to_string());
            }
            Err(e) => {
                let _ = tx.send(json!({
                    "stage": "error",
                    "message": format!("{e}"),
                }).to_string());
            }
        }
    });

    // Convert broadcast receiver into SSE stream
    let stream = BroadcastStream::new(rx)
        .filter_map(|msg| msg.ok())
        .map(|data| Ok(Event::default().data(data)));

    Ok(Sse::new(stream))
}

async fn cancel_build(State(st): State<Arc<AppState>>) -> Json<Value> {
    st.cancel_flag.store(true, Ordering::Relaxed);
    tracing::info!("build cancellation requested");
    Json(json!("ok"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_config() -> BuilderConfig {
        BuilderConfig {
            versions: llamafile_core::config_manager::parse_config_data(
                include_str!("../../../data/versions.json"),
            ).unwrap(),
            models: llamafile_core::config_manager::parse_config_data(
                include_str!("../../../data/models.json"),
            ).unwrap(),
            config_schema: llamafile_core::config_manager::parse_config_data(
                include_str!("../../../data/config.json"),
            ).unwrap(),
            config_version: 1,
            min_builder_version: None,
        }
    }

    fn app() -> Router {
        let (router, _) = build_router(test_config());
        router
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, Value) {
        let resp = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&body).unwrap();
        (status, json)
    }

    async fn post_json(app: Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let resp = app
            .oneshot(
                Request::post(uri)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    // ── Read-only endpoints ──

    #[tokio::test]
    async fn versions_returns_array() {
        let (status, json) = get_json(app(), "/api/versions").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.is_array());
        assert!(!json.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn models_returns_array() {
        let (status, json) = get_json(app(), "/api/models").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn models_search_filters() {
        let (_, all) = get_json(app(), "/api/models").await;
        let (_, filtered) = get_json(app(), "/api/models?search=qwen").await;
        assert!(filtered.as_array().unwrap().len() <= all.as_array().unwrap().len());
        assert!(!filtered.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn model_by_id_found() {
        let (status, json) = get_json(app(), "/api/models/Qwen%2FQwen3-0.6B-GGUF").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["id"], "Qwen/Qwen3-0.6B-GGUF");
    }

    #[tokio::test]
    async fn model_by_id_not_found() {
        let (status, _) = get_json(app(), "/api/models/nonexistent%2Fmodel").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn config_schema_returns_array() {
        let (status, json) = get_json(app(), "/api/config-schema").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.is_array());
    }

    #[tokio::test]
    async fn system_info_has_fields() {
        let (status, json) = get_json(app(), "/api/system-info").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.get("os").is_some());
        assert!(json.get("arch").is_some());
        assert!(json.get("memory_gb").is_some());
    }

    // ── State mutations ──

    #[tokio::test]
    async fn state_starts_empty() {
        let (status, json) = get_json(app(), "/api/state").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json, json!({}));
    }

    #[tokio::test]
    async fn set_and_get_version() {
        let (router, state) = build_router(test_config());

        let (status, _) = post_json(
            router.clone(),
            "/api/state/version",
            json!({"group": "llamafile 0.10", "version": "0.10.0"}),
        ).await;
        assert_eq!(status, StatusCode::OK);

        let saved = state.saved.lock().await;
        assert_eq!(saved.version_group.as_deref(), Some("llamafile 0.10"));
        assert_eq!(saved.version.as_deref(), Some("0.10.0"));
    }

    #[tokio::test]
    async fn set_model_validates_id() {
        let (status, _) = post_json(
            app(),
            "/api/state/model",
            json!({"id": "Qwen/Qwen3-0.6B-GGUF"}),
        ).await;
        assert_eq!(status, StatusCode::OK);

        let (status, json) = post_json(
            app(),
            "/api/state/model",
            json!({"id": "nonexistent/model"}),
        ).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(json["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn set_model_clears_quantizations() {
        let (router, state) = build_router(test_config());

        // Set a quantization first
        post_json(router.clone(), "/api/state/quantization", json!({"role": "weights", "name": "Q4_K_M"})).await;
        assert!(!state.saved.lock().await.quantizations.is_empty());

        // Set model clears it
        post_json(router.clone(), "/api/state/model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"})).await;
        assert!(state.saved.lock().await.quantizations.is_empty());
    }

    #[tokio::test]
    async fn set_config_value() {
        let (router, state) = build_router(test_config());
        let (status, _) = post_json(
            router.clone(),
            "/api/state/config",
            json!({"key": "context_length", "value": 8192}),
        ).await;
        assert_eq!(status, StatusCode::OK);
        let saved = state.saved.lock().await;
        assert_eq!(saved.config_values["context_length"], 8192);
    }

    #[tokio::test]
    async fn set_output_path() {
        let (router, state) = build_router(test_config());
        post_json(router.clone(), "/api/state/output", json!({"path": "/tmp/out.llamafile"})).await;
        assert_eq!(state.saved.lock().await.output_path.as_deref(), Some("/tmp/out.llamafile"));
    }

    #[tokio::test]
    async fn gpu_add_and_remove() {
        let (router, state) = build_router(test_config());

        post_json(router.clone(), "/api/state/gpu", json!({"action": "add", "spec": "cuda"})).await;
        assert_eq!(state.saved.lock().await.gpu_libs, vec!["cuda"]);

        // Duplicate add is idempotent
        post_json(router.clone(), "/api/state/gpu", json!({"action": "add", "spec": "cuda"})).await;
        assert_eq!(state.saved.lock().await.gpu_libs, vec!["cuda"]);

        post_json(router.clone(), "/api/state/gpu", json!({"action": "remove", "spec": "cuda"})).await;
        assert!(state.saved.lock().await.gpu_libs.is_empty());
    }

    #[tokio::test]
    async fn gpu_invalid_spec_rejected() {
        let (status, json) = post_json(
            app(),
            "/api/state/gpu",
            json!({"action": "add", "spec": "bogus"}),
        ).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"].as_str().unwrap().contains("Unknown"));
    }

    #[tokio::test]
    async fn reset_clears_state() {
        let (router, state) = build_router(test_config());

        post_json(router.clone(), "/api/state/model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"})).await;
        assert!(state.saved.lock().await.model_id.is_some());

        post_json(router.clone(), "/api/state/reset", json!({})).await;
        assert!(state.saved.lock().await.model_id.is_none());
    }

    #[tokio::test]
    async fn missing_params_returns_400() {
        let (status, _) = post_json(app(), "/api/state/version", json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = post_json(app(), "/api/state/model", json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, _) = post_json(app(), "/api/state/config", json!({"key": "x"})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST); // missing value
    }

    // ── Derived state queries ──

    #[tokio::test]
    async fn validate_empty_state_passes_with_defaults() {
        // build_from_state applies defaults, so empty state is valid
        let (status, json) = get_json(app(), "/api/state/validate").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.as_array().unwrap().is_empty(), "default state should validate clean");
    }

    #[tokio::test]
    async fn args_preview_returns_string() {
        let (status, json) = get_json(app(), "/api/state/args").await;
        assert_eq!(status, StatusCode::OK);
        assert!(json.as_str().unwrap().contains("-m\n"));
    }

    #[tokio::test]
    async fn components_returns_array_with_roles() {
        let (status, json) = get_json(app(), "/api/state/components").await;
        assert_eq!(status, StatusCode::OK);
        let arr = json.as_array().unwrap();
        let roles: Vec<&str> = arr.iter().filter_map(|c| c["role"].as_str()).collect();
        assert!(roles.contains(&"Runtime"));
        assert!(roles.contains(&"Weights"));
        assert!(roles.contains(&"ArgsFile"));
    }

    // ── Cancel ──

    #[tokio::test]
    async fn cancel_sets_flag() {
        let (router, state) = build_router(test_config());
        assert!(!state.cancel_flag.load(Ordering::Relaxed));
        post_json(router.clone(), "/api/cancel", json!({})).await;
        assert!(state.cancel_flag.load(Ordering::Relaxed));
    }
}

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::Json;
use axum::routing::{get, post, put, delete as delete_route};
use axum::Router;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use utoipa::OpenApi;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::config_store::{ConfigStore, SavedState};

use crate::state_file;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "llamafile-builder API",
        version = "0.1.0",
        description = "REST API for building custom llamafiles. \
            State is managed via named recipes (build configurations) \
            identified by UUID."
    ),
    paths(
        get_versions, get_models, get_model, get_config_schema,
        get_system_info, get_ollama_models, get_lmstudio_models,
        create_recipe, list_recipes, get_recipe, delete_recipe, name_recipe,
        get_recipe_state, set_version, set_model, set_model_file,
        set_ollama_model, set_lmstudio_model,
        set_quantization, set_config, set_output, set_args_file,
        modify_gpu, set_runtime, set_zipalign, reset_state,
        get_args, get_validate, get_components,
        start_build, cancel_build,
    ),
)]
struct ApiDoc;

const DOCS_PATH: &str = "/docs";

// ============================================================================
// Shared server state
// ============================================================================

struct AppState {
    config: BuilderConfig,
    store: ConfigStore,
    cancel_flag: Arc<AtomicBool>,
    build_tx: broadcast::Sender<String>,
}

// ============================================================================
// Entry point
// ============================================================================

fn build_router(config: BuilderConfig, store: ConfigStore) -> (Router, Arc<AppState>) {
    let (build_tx, _) = broadcast::channel::<String>(256);
    let state = Arc::new(AppState {
        config,
        store,
        cancel_flag: Arc::new(AtomicBool::new(false)),
        build_tx,
    });

    let router = Router::new()
        // Read-only data
        .route("/api/versions", get(get_versions))
        .route("/api/models", get(get_models))
        .route("/api/models/{id}", get(get_model))
        .route("/api/config-schema", get(get_config_schema))
        .route("/api/system-info", get(get_system_info))
        .route("/api/ollama/models", get(get_ollama_models))
        .route("/api/lmstudio/models", get(get_lmstudio_models))
        // Recipe CRUD
        .route("/api/recipes", post(create_recipe))
        .route("/api/recipes", get(list_recipes))
        .route("/api/recipes/{id}", get(get_recipe))
        .route("/api/recipes/{id}", delete_route(delete_recipe))
        .route("/api/recipes/{id}/name", put(name_recipe))
        // Recipe state
        .route("/api/recipes/{id}/state", get(get_recipe_state))
        .route("/api/recipes/{id}/state/version", post(set_version))
        .route("/api/recipes/{id}/state/model", post(set_model))
        .route("/api/recipes/{id}/state/model-file", post(set_model_file))
        .route("/api/recipes/{id}/state/ollama-model", post(set_ollama_model))
        .route("/api/recipes/{id}/state/lmstudio-model", post(set_lmstudio_model))
        .route("/api/recipes/{id}/state/quantization", post(set_quantization))
        .route("/api/recipes/{id}/state/config", post(set_config))
        .route("/api/recipes/{id}/state/output", post(set_output))
        .route("/api/recipes/{id}/state/args-file", post(set_args_file))
        .route("/api/recipes/{id}/state/gpu", post(modify_gpu))
        .route("/api/recipes/{id}/state/runtime", post(set_runtime))
        .route("/api/recipes/{id}/state/zipalign", post(set_zipalign))
        .route("/api/recipes/{id}/state/reset", post(reset_state))
        .route("/api/recipes/{id}/state/args", get(get_args))
        .route("/api/recipes/{id}/state/validate", get(get_validate))
        .route("/api/recipes/{id}/state/components", get(get_components))
        // Build
        .route("/api/recipes/{id}/build", post(start_build))
        .route("/api/cancel", post(cancel_build))
        .with_state(state.clone())
        .layer(tower_http::cors::CorsLayer::permissive())
        .merge(
            utoipa_swagger_ui::SwaggerUi::new(DOCS_PATH)
                .url("/api-docs/openapi.json", ApiDoc::openapi())
                .config(
                    utoipa_swagger_ui::Config::default()
                        .use_base_layout()
                        .display_request_duration(true)
                        .try_it_out_enabled(true)
                ),
        );

    (router, state)
}

pub async fn run(
    config: BuilderConfig,
    port: u16,
    web_dir: Option<PathBuf>,
    no_open: bool,
) -> Result<(), String> {
    let store = ConfigStore::new(crate::get_recipes_dir());
    let (router, _state) = build_router(config, store);

    let mut app = router;

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
        let url = format!("http://{addr}{DOCS_PATH}");
        if open::that(&url).is_err() {
            tracing::warn!("could not open browser, visit {url} manually");
        }
    }

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

// ============================================================================
// Helpers
// ============================================================================

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg.into() })))
}

fn parse_uuid(id: &str) -> Result<uuid::Uuid, (StatusCode, Json<Value>)> {
    uuid::Uuid::parse_str(id).map_err(|_| api_err(StatusCode::BAD_REQUEST, "invalid UUID"))
}

/// Load a recipe, apply a mutation, save it back, return "ok" with the recipe ID.
async fn mutate_recipe(
    st: &AppState,
    id: &str,
    f: impl FnOnce(&mut SavedState) -> Result<(), (StatusCode, Json<Value>)>,
) -> ApiResult {
    let uuid = parse_uuid(id)?;
    let mut recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    f(&mut recipe.state)?;
    recipe.updated_at = chrono::Utc::now();
    st.store.save(&recipe).map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(json!({"ok": true, "id": recipe.id.to_string()})))
}

fn make_builder(config: &BuilderConfig, saved: &SavedState) -> Result<llamafile_core::Builder, String> {
    let cache = Cache::new(crate::get_cache_dir());
    state_file::build_from_state(saved, config.clone(), cache)
}

// ============================================================================
// Read-only data endpoints
// ============================================================================

#[utoipa::path(get, path = "/api/versions", tag = "Data",
    summary = "List available llamafile versions")]
async fn get_versions(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::to_value(&st.config.versions).unwrap())
}

#[derive(Deserialize, Default)]
struct ModelQuery {
    search: Option<String>,
    tag: Option<String>,
}

#[utoipa::path(get, path = "/api/models", tag = "Data",
    summary = "Search and list models",
    params(("search" = Option<String>, Query, description = "Filter by name/org"),
           ("tag" = Option<String>, Query, description = "Filter by tag")))]
async fn get_models(State(st): State<Arc<AppState>>, Query(q): Query<ModelQuery>) -> Json<Value> {
    let models = llamafile_core::search_models(
        &st.config.models, q.search.as_deref(), q.tag.as_deref(),
    );
    Json(serde_json::to_value(&models).unwrap())
}

#[utoipa::path(get, path = "/api/models/{id}", tag = "Data",
    summary = "Get model details by ID",
    params(("id" = String, Path, description = "Model ID (URL-encoded)")))]
async fn get_model(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let model = st.config.models.iter().find(|m| m.id == id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, format!("model not found: {id}")))?;
    Ok(Json(serde_json::to_value(model).unwrap()))
}

#[utoipa::path(get, path = "/api/config-schema", tag = "Data",
    summary = "Get configuration schema for UI rendering")]
async fn get_config_schema(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(serde_json::to_value(&st.config.config_schema).unwrap())
}

#[utoipa::path(get, path = "/api/system-info", tag = "Data",
    summary = "Get host system information")]
async fn get_system_info() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::system_info()).unwrap())
}

#[utoipa::path(get, path = "/api/ollama/models", tag = "Data",
    summary = "Scan local Ollama models")]
async fn get_ollama_models() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::ollama::get_ollama_models(None)).unwrap())
}

#[utoipa::path(get, path = "/api/lmstudio/models", tag = "Data",
    summary = "Scan local LM Studio models")]
async fn get_lmstudio_models() -> Json<Value> {
    Json(serde_json::to_value(&llamafile_core::lmstudio::get_lmstudio_models(None)).unwrap())
}

// ============================================================================
// Recipe CRUD
// ============================================================================

#[utoipa::path(post, path = "/api/recipes", tag = "Recipes",
    summary = "Create a new build recipe")]
async fn create_recipe(State(st): State<Arc<AppState>>) -> ApiResult {
    let recipe = st.store.create().map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(json!({"id": recipe.id.to_string()})))
}

#[utoipa::path(get, path = "/api/recipes", tag = "Recipes",
    summary = "List all saved recipes")]
async fn list_recipes(State(st): State<Arc<AppState>>) -> ApiResult {
    let recipes = st.store.list().map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    let items: Vec<Value> = recipes.iter().map(|r| json!({
        "id": r.id.to_string(),
        "name": r.name,
        "model": r.state.model_id,
        "updated_at": r.updated_at.to_rfc3339(),
    })).collect();
    Ok(Json(Value::Array(items)))
}

#[utoipa::path(get, path = "/api/recipes/{id}", tag = "Recipes",
    summary = "Get a recipe by UUID",
    params(("id" = String, Path, description = "Recipe UUID")))]
async fn get_recipe(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    Ok(Json(serde_json::to_value(&recipe).unwrap()))
}

#[utoipa::path(delete, path = "/api/recipes/{id}", tag = "Recipes",
    summary = "Delete a recipe",
    params(("id" = String, Path, description = "Recipe UUID")))]
async fn delete_recipe(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    st.store.delete(uuid).map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(json!("ok")))
}

#[utoipa::path(put, path = "/api/recipes/{id}/name", tag = "Recipes",
    summary = "Name or rename a recipe (resolves defaults for full snapshot)",
    params(("id" = String, Path, description = "Recipe UUID")),
    request_body = Value)]
async fn name_recipe(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'name'"))?;
    let uuid = parse_uuid(&id)?;
    let mut recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    recipe.name = Some(name.to_string());
    recipe.state.resolve_defaults(&st.config);
    recipe.updated_at = chrono::Utc::now();
    st.store.save(&recipe).map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    Ok(Json(json!({"ok": true, "id": recipe.id.to_string()})))
}

// ============================================================================
// Recipe state endpoints
// ============================================================================

#[utoipa::path(get, path = "/api/recipes/{id}/state", tag = "State",
    summary = "Get recipe state",
    params(("id" = String, Path, description = "Recipe UUID")))]
async fn get_recipe_state(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    Ok(Json(serde_json::to_value(&recipe.state).unwrap()))
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/version", tag = "State",
    summary = "Set version", params(("id" = String, Path)), request_body = Value)]
async fn set_version(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let group = body.get("group").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'group'"))?.to_string();
    let version = body.get("version").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'version'"))?.to_string();
    mutate_recipe(&st, &id, |s| {
        s.version_group = Some(group); s.version = Some(version); s.custom_runtime = None; Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/model", tag = "State",
    summary = "Select a model", params(("id" = String, Path)), request_body = Value)]
async fn set_model(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let mid = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'id'"))?.to_string();
    if !st.config.models.iter().any(|m| m.id == mid) {
        return Err(api_err(StatusCode::NOT_FOUND, format!("model not found: {mid}")));
    }
    mutate_recipe(&st, &id, |s| {
        s.model_id = Some(mid); s.custom_model_file = None;
        s.ollama_model = None; s.lmstudio_model = None; s.quantizations.clear(); Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/model-file", tag = "State",
    summary = "Use custom model file", params(("id" = String, Path)), request_body = Value)]
async fn set_model_file(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?.to_string();
    mutate_recipe(&st, &id, |s| {
        s.custom_model_file = Some(path); s.model_id = None;
        s.ollama_model = None; s.lmstudio_model = None; s.quantizations.clear(); Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/ollama-model", tag = "State",
    summary = "Select a local Ollama model", params(("id" = String, Path)), request_body = Value)]
async fn set_ollama_model(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'name'"))?.to_string();
    let models = llamafile_core::ollama::get_ollama_models(None);
    let model = models.into_iter().find(|m| m.display_name == name)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, format!("Ollama model not found: {name}")))?;
    mutate_recipe(&st, &id, |s| {
        s.ollama_model = Some(model); s.model_id = None;
        s.custom_model_file = None; s.lmstudio_model = None; s.quantizations.clear(); Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/lmstudio-model", tag = "State",
    summary = "Select a local LM Studio model", params(("id" = String, Path)), request_body = Value)]
async fn set_lmstudio_model(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'name'"))?.to_string();
    let models = llamafile_core::lmstudio::get_lmstudio_models(None);
    let model = models.into_iter().find(|m| m.dir_name == name || m.display_name == name)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, format!("LM Studio model not found: {name}")))?;
    mutate_recipe(&st, &id, |s| {
        s.lmstudio_model = Some(model); s.model_id = None;
        s.custom_model_file = None; s.ollama_model = None; s.quantizations.clear(); Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/quantization", tag = "State",
    summary = "Set quantization", params(("id" = String, Path)), request_body = Value)]
async fn set_quantization(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let role = body.get("role").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'role'"))?.to_string();
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'name'"))?.to_string();
    mutate_recipe(&st, &id, |s| { s.quantizations.insert(role, name); Ok(()) }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/config", tag = "State",
    summary = "Set config value", params(("id" = String, Path)), request_body = Value)]
async fn set_config(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let key = body.get("key").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'key'"))?.to_string();
    let value = body.get("value").cloned()
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'value'"))?;
    mutate_recipe(&st, &id, |s| { s.config_values.insert(key, value); Ok(()) }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/output", tag = "State",
    summary = "Set output path", params(("id" = String, Path)), request_body = Value)]
async fn set_output(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?.to_string();
    mutate_recipe(&st, &id, |s| { s.output_path = Some(path); Ok(()) }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/args-file", tag = "State",
    summary = "Set custom .args file", params(("id" = String, Path)), request_body = Value)]
async fn set_args_file(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?.to_string();
    mutate_recipe(&st, &id, |s| { s.args_file = Some(path); Ok(()) }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/gpu", tag = "State",
    summary = "Add or remove GPU library", params(("id" = String, Path)), request_body = Value)]
async fn modify_gpu(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let action = body.get("action").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'action'"))?.to_string();
    let spec = body.get("spec").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'spec'"))?.to_string();
    crate::commands::build::parse_gpu_spec(&spec)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?;
    mutate_recipe(&st, &id, |s| {
        match action.as_str() {
            "add" => { if !s.gpu_libs.contains(&spec) { s.gpu_libs.push(spec); } }
            "remove" => { s.gpu_libs.retain(|g| g != &spec); }
            _ => return Err(api_err(StatusCode::BAD_REQUEST, "action must be 'add' or 'remove'")),
        }
        Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/runtime", tag = "State",
    summary = "Set custom runtime", params(("id" = String, Path)), request_body = Value)]
async fn set_runtime(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?.to_string();
    mutate_recipe(&st, &id, |s| {
        s.custom_runtime = Some(path); s.version = None; s.version_group = None; Ok(())
    }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/zipalign", tag = "State",
    summary = "Set custom zipalign", params(("id" = String, Path)), request_body = Value)]
async fn set_zipalign(State(st): State<Arc<AppState>>, Path(id): Path<String>, Json(body): Json<Value>) -> ApiResult {
    let path = body.get("path").and_then(|v| v.as_str())
        .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, "missing 'path'"))?.to_string();
    mutate_recipe(&st, &id, |s| { s.custom_zipalign = Some(path); Ok(()) }).await
}

#[utoipa::path(post, path = "/api/recipes/{id}/state/reset", tag = "State",
    summary = "Reset recipe state", params(("id" = String, Path)))]
async fn reset_state(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    mutate_recipe(&st, &id, |s| { *s = SavedState::default(); Ok(()) }).await
}

// ============================================================================
// Derived state queries
// ============================================================================

#[utoipa::path(get, path = "/api/recipes/{id}/state/args", tag = "State",
    summary = "Preview .args content", params(("id" = String, Path)))]
async fn get_args(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    let builder = make_builder(&st.config, &recipe.state)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(Value::String(builder.resolved_args())))
}

#[utoipa::path(get, path = "/api/recipes/{id}/state/validate", tag = "State",
    summary = "Validate recipe state", params(("id" = String, Path)))]
async fn get_validate(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    let builder = make_builder(&st.config, &recipe.state)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let msgs: Vec<Value> = builder.validate().iter().map(|m| match m {
        llamafile_core::ValidationMessage::Warning(s) => json!({"level": "warning", "message": s}),
        llamafile_core::ValidationMessage::Error(s) => json!({"level": "error", "message": s}),
    }).collect();
    Ok(Json(Value::Array(msgs)))
}

#[utoipa::path(get, path = "/api/recipes/{id}/state/components", tag = "State",
    summary = "List resolved components", params(("id" = String, Path)))]
async fn get_components(State(st): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    let builder = make_builder(&st.config, &recipe.state)
        .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let comps: Vec<Value> = builder.resolved_components().iter().map(|c| {
        json!({ "role": format!("{:?}", c.role), "source": c.source_display(), "zip_path": c.zip_path })
    }).collect();
    Ok(Json(Value::Array(comps)))
}

// ============================================================================
// Build (SSE)
// ============================================================================

#[utoipa::path(post, path = "/api/recipes/{id}/build", tag = "Build",
    summary = "Start a build (SSE stream)", params(("id" = String, Path)))]
async fn start_build(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, Json<Value>)> {
    let uuid = parse_uuid(&id)?;
    let recipe = st.store.get(uuid).map_err(|e| api_err(StatusCode::NOT_FOUND, format!("{e}")))?;
    let builder = make_builder(&st.config, &recipe.state)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, e))?;

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

    tokio::spawn(async move {
        let mut builder = builder;
        builder.set_cancel_flag(cancel_flag);
        let _ = tx.send(json!({"stage": "starting"}).to_string());
        match builder.build().await {
            Ok(path) => {
                let size = tokio::fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
                let _ = tx.send(json!({"stage": "complete", "path": path.to_string_lossy(), "size": size}).to_string());
            }
            Err(e) => {
                let _ = tx.send(json!({"stage": "error", "message": format!("{e}")}).to_string());
            }
        }
    });

    let stream = BroadcastStream::new(rx)
        .filter_map(|msg| msg.ok())
        .map(|data| Ok(Event::default().data(data)));
    Ok(Sse::new(stream))
}

#[utoipa::path(post, path = "/api/cancel", tag = "Build",
    summary = "Cancel a running build")]
async fn cancel_build(State(st): State<Arc<AppState>>) -> Json<Value> {
    st.cancel_flag.store(true, Ordering::Relaxed);
    tracing::info!("build cancellation requested");
    Json(json!("ok"))
}

// ============================================================================
// Tests
// ============================================================================

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

    fn app() -> (Router, Arc<AppState>) {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.into_path());
        build_router(test_config(), store)
    }

    async fn get_json(router: &Router, uri: &str) -> (StatusCode, Value) {
        let resp = router.clone()
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await.unwrap();
        let status = resp.status();
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn post_json(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let resp = router.clone()
            .oneshot(Request::post(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap())).unwrap())
            .await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn delete_req(router: &Router, uri: &str) -> StatusCode {
        router.clone()
            .oneshot(Request::delete(uri).body(Body::empty()).unwrap())
            .await.unwrap().status()
    }

    async fn put_json(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let resp = router.clone()
            .oneshot(Request::put(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap())).unwrap())
            .await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    // Helper: create a recipe and return its UUID string
    async fn create(router: &Router) -> String {
        let (status, json) = post_json(router, "/api/recipes", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        json["id"].as_str().unwrap().to_string()
    }

    // ── Data endpoints ──

    #[tokio::test]
    async fn versions_returns_array() {
        let (router, _) = app();
        let (s, j) = get_json(&router, "/api/versions").await;
        assert_eq!(s, StatusCode::OK);
        assert!(j.is_array() && !j.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn models_search() {
        let (router, _) = app();
        let (_, all) = get_json(&router, "/api/models").await;
        let (_, filtered) = get_json(&router, "/api/models?search=qwen").await;
        assert!(!filtered.as_array().unwrap().is_empty());
        assert!(filtered.as_array().unwrap().len() <= all.as_array().unwrap().len());
    }

    #[tokio::test]
    async fn system_info_has_fields() {
        let (router, _) = app();
        let (s, j) = get_json(&router, "/api/system-info").await;
        assert_eq!(s, StatusCode::OK);
        assert!(j.get("os").is_some());
    }

    // ── Recipe CRUD ──

    #[tokio::test]
    async fn create_list_delete_recipe() {
        let (router, _) = app();

        // Create
        let id = create(&router).await;
        assert!(!id.is_empty());

        // List
        let (_, list) = get_json(&router, "/api/recipes").await;
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["id"].as_str().unwrap(), id);

        // Get
        let (s, recipe) = get_json(&router, &format!("/api/recipes/{id}")).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(recipe["id"].as_str().unwrap(), id);

        // Delete
        let s = delete_req(&router, &format!("/api/recipes/{id}")).await;
        assert_eq!(s, StatusCode::OK);

        // Gone
        let (_, list) = get_json(&router, "/api/recipes").await;
        assert!(list.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn name_recipe_resolves_defaults() {
        let (router, _) = app();
        let id = create(&router).await;

        let (s, _) = put_json(&router, &format!("/api/recipes/{id}/name"), json!({"name": "test"})).await;
        assert_eq!(s, StatusCode::OK);

        let (_, recipe) = get_json(&router, &format!("/api/recipes/{id}")).await;
        assert_eq!(recipe["name"].as_str(), Some("test"));
        // After naming, defaults should be resolved
        assert!(recipe["state"]["version_group"].is_string());
        assert!(recipe["state"]["model_id"].is_string());
    }

    // ── State mutations ──

    #[tokio::test]
    async fn set_model_on_recipe() {
        let (router, _) = app();
        let id = create(&router).await;

        let (s, resp) = post_json(&router, &format!("/api/recipes/{id}/state/model"),
            json!({"id": "Qwen/Qwen3-0.6B-GGUF"})).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(resp["id"].as_str().unwrap(), id);

        let (_, state) = get_json(&router, &format!("/api/recipes/{id}/state")).await;
        assert_eq!(state["model_id"].as_str(), Some("Qwen/Qwen3-0.6B-GGUF"));
    }

    #[tokio::test]
    async fn set_model_invalid_returns_404() {
        let (router, _) = app();
        let id = create(&router).await;
        let (s, _) = post_json(&router, &format!("/api/recipes/{id}/state/model"),
            json!({"id": "nonexistent"})).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_config_value() {
        let (router, _) = app();
        let id = create(&router).await;
        post_json(&router, &format!("/api/recipes/{id}/state/config"),
            json!({"key": "context_length", "value": 8192})).await;
        let (_, state) = get_json(&router, &format!("/api/recipes/{id}/state")).await;
        assert_eq!(state["config_values"]["context_length"], 8192);
    }

    #[tokio::test]
    async fn gpu_add_remove() {
        let (router, _) = app();
        let id = create(&router).await;

        post_json(&router, &format!("/api/recipes/{id}/state/gpu"),
            json!({"action": "add", "spec": "cuda"})).await;
        let (_, state) = get_json(&router, &format!("/api/recipes/{id}/state")).await;
        assert_eq!(state["gpu_libs"], json!(["cuda"]));

        post_json(&router, &format!("/api/recipes/{id}/state/gpu"),
            json!({"action": "remove", "spec": "cuda"})).await;
        let (_, state) = get_json(&router, &format!("/api/recipes/{id}/state")).await;
        let gpu = state.get("gpu_libs").and_then(|v| v.as_array());
        assert!(gpu.is_none() || gpu.unwrap().is_empty());
    }

    #[tokio::test]
    async fn reset_clears_state() {
        let (router, _) = app();
        let id = create(&router).await;
        post_json(&router, &format!("/api/recipes/{id}/state/model"),
            json!({"id": "Qwen/Qwen3-0.6B-GGUF"})).await;
        post_json(&router, &format!("/api/recipes/{id}/state/reset"), json!({})).await;
        let (_, state) = get_json(&router, &format!("/api/recipes/{id}/state")).await;
        assert!(state["model_id"].is_null());
    }

    // ── Derived queries ──

    #[tokio::test]
    async fn args_and_components() {
        let (router, _) = app();
        let id = create(&router).await;
        post_json(&router, &format!("/api/recipes/{id}/state/model"),
            json!({"id": "Qwen/Qwen3-0.6B-GGUF"})).await;

        let (s, args) = get_json(&router, &format!("/api/recipes/{id}/state/args")).await;
        assert_eq!(s, StatusCode::OK);
        assert!(args.as_str().unwrap().contains("Qwen3"));

        let (s, comps) = get_json(&router, &format!("/api/recipes/{id}/state/components")).await;
        assert_eq!(s, StatusCode::OK);
        let roles: Vec<&str> = comps.as_array().unwrap().iter()
            .filter_map(|c| c["role"].as_str()).collect();
        assert!(roles.contains(&"Weights"));
    }

    #[tokio::test]
    async fn validate_empty_recipe() {
        let (router, _) = app();
        let id = create(&router).await;
        let (s, msgs) = get_json(&router, &format!("/api/recipes/{id}/state/validate")).await;
        assert_eq!(s, StatusCode::OK);
        assert!(msgs.as_array().unwrap().is_empty(), "defaults should validate clean");
    }

    // ── Missing recipe ──

    #[tokio::test]
    async fn operations_on_missing_recipe_return_404() {
        let (router, _) = app();
        let fake = uuid::Uuid::new_v4();
        let (s, _) = get_json(&router, &format!("/api/recipes/{fake}/state")).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }

    // ── Cancel ──

    #[tokio::test]
    async fn cancel_sets_flag() {
        let (router, st) = app();
        assert!(!st.cancel_flag.load(Ordering::Relaxed));
        post_json(&router, "/api/cancel", json!({})).await;
        assert!(st.cancel_flag.load(Ordering::Relaxed));
    }
}

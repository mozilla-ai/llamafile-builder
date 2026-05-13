use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};

use llamafile_builder_core::cache::Cache;
use llamafile_builder_core::config_manager::BuilderConfig;
use llamafile_builder_core::config_store::{ConfigStore, SavedState};

use crate::state_file;

// ============================================================================
// JSON-RPC types
// ============================================================================

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Serialize)]
struct Notification {
    jsonrpc: &'static str,
    method: &'static str,
    params: Value,
}

impl Response {
    fn ok(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }
    fn err(id: Value, code: i32, message: String) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(RpcError { code, message }) }
    }
}

// ============================================================================
// Session
// ============================================================================

struct Session {
    config: BuilderConfig,
    store: ConfigStore,
    current_recipe_id: Option<uuid::Uuid>,
}

impl Session {
    fn new(config: BuilderConfig, store: ConfigStore) -> Self {
        Self { config, store, current_recipe_id: None }
    }

    /// Get the current recipe's state, or an empty default if none loaded.
    fn current_state(&self) -> SavedState {
        self.current_recipe_id
            .and_then(|id| self.store.get(id).ok())
            .map(|r| r.state)
            .unwrap_or_default()
    }

    /// Mutate the current recipe's state. Creates a recipe if none exists.
    fn mutate_state(&mut self, f: impl FnOnce(&mut SavedState)) -> Result<Value, (i32, String)> {
        let id = match self.current_recipe_id {
            Some(id) => id,
            None => {
                let recipe = self.store.create().map_err(|e| (-32000, format!("{e}")))?;
                self.current_recipe_id = Some(recipe.id);
                recipe.id
            }
        };
        let mut recipe = self.store.get(id).map_err(|e| (-32000, format!("{e}")))?;
        f(&mut recipe.state);
        recipe.updated_at = chrono::Utc::now();
        self.store.save(&recipe).map_err(|e| (-32000, format!("{e}")))?;
        Ok(serde_json::json!({"ok": true, "recipe_id": id.to_string()}))
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, (i32, String)> {
        match method {
            // ── Data queries ──
            "list_versions" => Ok(serde_json::to_value(&self.config.versions).unwrap()),
            "list_models" => {
                let search = params.get("search").and_then(|v| v.as_str());
                let tag = params.get("tag").and_then(|v| v.as_str());
                let models = llamafile_builder_core::search_models(&self.config.models, search, tag);
                Ok(serde_json::to_value(&models).unwrap())
            }
            "show_model" => {
                let id = str_param(params, "id")?;
                let model = self.config.models.iter().find(|m| m.id == id)
                    .ok_or((-32000, format!("model not found: {id}")))?;
                Ok(serde_json::to_value(model).unwrap())
            }
            "system_info" => Ok(serde_json::to_value(&llamafile_builder_core::system_info()).unwrap()),
            "config_schema" => Ok(serde_json::to_value(&self.config.config_schema).unwrap()),
            "list_ollama_models" => {
                Ok(serde_json::to_value(&llamafile_builder_core::ollama::get_ollama_models(None)).unwrap())
            }
            "list_lmstudio_models" => {
                Ok(serde_json::to_value(&llamafile_builder_core::lmstudio::get_lmstudio_models(None)).unwrap())
            }

            // ── Recipe management ──
            "create_recipe" => {
                let recipe = self.store.create().map_err(|e| (-32000, format!("{e}")))?;
                self.current_recipe_id = Some(recipe.id);
                Ok(serde_json::json!({"id": recipe.id.to_string()}))
            }
            "list_recipes" => {
                let recipes = self.store.list().map_err(|e| (-32000, format!("{e}")))?;
                let items: Vec<Value> = recipes.iter().map(|r| serde_json::json!({
                    "id": r.id.to_string(),
                    "name": r.name,
                    "model": r.state.model_id,
                    "updated_at": r.updated_at.to_rfc3339(),
                    "current": Some(r.id) == self.current_recipe_id,
                })).collect();
                Ok(Value::Array(items))
            }
            "load_recipe" => {
                let query = str_param(params, "id")?;
                let recipe = self.store.find_by_name_or_id(&query)
                    .map_err(|e| (-32000, format!("{e}")))?
                    .ok_or((-32000, format!("recipe not found: {query}")))?;
                self.current_recipe_id = Some(recipe.id);
                Ok(serde_json::json!({"id": recipe.id.to_string(), "name": recipe.name}))
            }
            "save_recipe" => {
                let name = str_param(params, "name")?;
                let id = self.current_recipe_id
                    .ok_or((-32000, "no active recipe to save".to_string()))?;
                let mut recipe = self.store.get(id).map_err(|e| (-32000, format!("{e}")))?;
                recipe.name = Some(name.clone());
                recipe.state.resolve_defaults(&self.config);
                recipe.updated_at = chrono::Utc::now();
                self.store.save(&recipe).map_err(|e| (-32000, format!("{e}")))?;
                Ok(serde_json::json!({"ok": true, "id": id.to_string(), "name": name}))
            }
            "delete_recipe" => {
                let query = str_param(params, "id")?;
                let recipe = self.store.find_by_name_or_id(&query)
                    .map_err(|e| (-32000, format!("{e}")))?
                    .ok_or((-32000, format!("recipe not found: {query}")))?;
                if Some(recipe.id) == self.current_recipe_id {
                    self.current_recipe_id = None;
                }
                self.store.delete(recipe.id).map_err(|e| (-32000, format!("{e}")))?;
                Ok(Value::String("ok".to_string()))
            }

            // ── State mutations (on current recipe) ──
            "set_version" => {
                let group = str_param(params, "group")?;
                let version = str_param(params, "version")?;
                self.mutate_state(|s| {
                    s.version_group = Some(group); s.version = Some(version); s.custom_runtime = None;
                })
            }
            "set_model" => {
                let id = str_param(params, "id")?;
                if !self.config.models.iter().any(|m| m.id == id) {
                    return Err((-32000, format!("model not found: {id}")));
                }
                self.mutate_state(|s| {
                    s.model_id = Some(id); s.custom_model_file = None;
                    s.ollama_model = None; s.lmstudio_model = None; s.quantizations.clear();
                })
            }
            "set_model_file" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| {
                    s.custom_model_file = Some(path); s.model_id = None;
                    s.ollama_model = None; s.lmstudio_model = None; s.quantizations.clear();
                })
            }
            "set_ollama_model" => {
                let name = str_param(params, "name")?;
                let models = llamafile_builder_core::ollama::get_ollama_models(None);
                let model = models.into_iter().find(|m| m.display_name == name)
                    .ok_or((-32000, format!("Ollama model not found: {name}")))?;
                self.mutate_state(|s| {
                    s.ollama_model = Some(model); s.model_id = None;
                    s.custom_model_file = None; s.lmstudio_model = None; s.quantizations.clear();
                })
            }
            "set_lmstudio_model" => {
                let name = str_param(params, "name")?;
                let models = llamafile_builder_core::lmstudio::get_lmstudio_models(None);
                let model = models.into_iter().find(|m| m.dir_name == name || m.display_name == name)
                    .ok_or((-32000, format!("LM Studio model not found: {name}")))?;
                self.mutate_state(|s| {
                    s.lmstudio_model = Some(model); s.model_id = None;
                    s.custom_model_file = None; s.ollama_model = None; s.quantizations.clear();
                })
            }
            "set_quantization" => {
                let role = str_param(params, "role")?;
                let name = str_param(params, "name")?;
                self.mutate_state(|s| { s.quantizations.insert(role, name); })
            }
            "set_config" => {
                let key = str_param(params, "key")?;
                let value = params.get("value").cloned()
                    .ok_or((-32602, "missing 'value' param".to_string()))?;
                self.mutate_state(|s| { s.config_values.insert(key, value); })
            }
            "set_output" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| { s.output_path = Some(path); })
            }
            "set_args_file" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| { s.args_file = Some(path); })
            }
            "set_args_override" => {
                // Inline `.args` content. Mirrors what the TUI's .args
                // editor stores. `apply_saved_state` reads
                // `args_override` ahead of `args_file`, so this slot
                // wins if both are set.
                let content = str_param(params, "content")?;
                self.mutate_state(|s| { s.args_override = Some(content); })
            }
            "clear_args_override" => {
                self.mutate_state(|s| { s.args_override = None; })
            }
            "set_mmproj_disabled" => {
                let disabled = params.get("disabled").and_then(|v| v.as_bool())
                    .ok_or((-32602, "missing 'disabled' bool param".to_string()))?;
                self.mutate_state(|s| { s.mmproj_disabled = disabled; })
            }
            "add_custom_gpu" => {
                // Mirrors the HTTP server's POST /state/custom-gpu (action=add).
                let path = str_param(params, "path")?;
                let accel = str_param(params, "accel")?.to_lowercase();
                let os = str_param(params, "os")?.to_lowercase();
                llamafile_builder_core::parse_gpu_spec(&format!("{accel}:{os}"))
                    .map_err(|e| (-32000, e))?;
                self.mutate_state(|s| {
                    s.custom_gpu_libs
                        .retain(|l| !(l.accel == accel && l.os == os));
                    s.custom_gpu_libs.push(llamafile_builder_core::CustomGpuLib {
                        path,
                        accel,
                        os,
                    });
                })
            }
            "remove_custom_gpu" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| {
                    s.custom_gpu_libs.retain(|l| l.path != path);
                })
            }
            "add_companion_file" => {
                // Mirrors the HTTP server's POST /state/companion-file (action=add).
                let role = str_param(params, "role")?;
                let path = str_param(params, "path")?;
                self.mutate_state(|s| {
                    s.custom_companion_files.insert(role, path);
                })
            }
            "remove_companion_file" => {
                let role = str_param(params, "role")?;
                self.mutate_state(|s| {
                    s.custom_companion_files.remove(&role);
                })
            }
            "add_gpu" => {
                let spec = str_param(params, "spec")?;
                crate::commands::build::parse_gpu_spec(&spec).map_err(|e| (-32000, e))?;
                self.mutate_state(|s| {
                    if !s.gpu_libs.contains(&spec) { s.gpu_libs.push(spec); }
                })
            }
            "remove_gpu" => {
                let spec = str_param(params, "spec")?;
                self.mutate_state(|s| { s.gpu_libs.retain(|g| g != &spec); })
            }
            "set_runtime" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| {
                    s.custom_runtime = Some(path); s.version = None; s.version_group = None;
                })
            }
            "set_zipalign" => {
                let path = str_param(params, "path")?;
                self.mutate_state(|s| { s.custom_zipalign = Some(path); })
            }

            // ── State queries ──
            "get_state" => Ok(serde_json::to_value(&self.current_state()).unwrap()),
            "show_args" => {
                let builder = self.make_builder()?;
                Ok(Value::String(builder.resolved_args()))
            }
            "validate" => {
                let builder = self.make_builder()?;
                let msgs: Vec<Value> = builder.validate().iter().map(|m| match m {
                    llamafile_builder_core::ValidationMessage::Warning(s) => serde_json::json!({"level": "warning", "message": s}),
                    llamafile_builder_core::ValidationMessage::Error(s) => serde_json::json!({"level": "error", "message": s}),
                }).collect();
                Ok(Value::Array(msgs))
            }
            "resolved_components" => {
                let builder = self.make_builder()?;
                let json: Vec<Value> = builder.resolved_components().iter().map(|c| {
                    serde_json::json!({
                        "role": format!("{:?}", c.role),
                        "source": c.source_display(),
                        "zip_path": c.zip_path,
                    })
                }).collect();
                Ok(Value::Array(json))
            }
            "disk_requirements" => {
                let builder = self.make_builder()?;
                let json: Vec<Value> = builder.estimate_disk_requirements().iter().map(|r| {
                    serde_json::json!({
                        "destination": r.destination.to_string_lossy(),
                        "bytes": r.bytes,
                        "purpose": r.purpose.label(),
                    })
                }).collect();
                Ok(Value::Array(json))
            }
            "reset" => {
                self.mutate_state(|s| { *s = SavedState::default(); })
            }

            // build is handled separately (async)
            "build" => Err((-32000, "use build_async".to_string())),

            _ => Err((-32601, format!("method not found: {method}"))),
        }
    }

    fn make_builder(&self) -> Result<llamafile_builder_core::Builder, (i32, String)> {
        let cache = Cache::new(crate::get_cache_dir());
        state_file::build_from_state(&self.current_state(), self.config.clone(), cache)
            .map_err(|e| (-32000, e))
    }
}

fn str_param(params: &Value, key: &str) -> Result<String, (i32, String)> {
    params.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
        .ok_or((-32602, format!("missing '{key}' param")))
}

// ============================================================================
// Main loop
// ============================================================================

pub async fn run(config: BuilderConfig) -> Result<(), String> {
    let store = ConfigStore::new(crate::get_recipes_dir());
    let mut session = Session::new(config, store);
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    tracing::info!("JSON-RPC interactive mode started");

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| format!("stdin read error: {e}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::err(Value::Null, -32700, format!("parse error: {e}"));
                writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap()).ok();
                stdout.flush().ok();
                continue;
            }
        };

        let resp = if req.method == "build" {
            match session.make_builder() {
                Ok(builder) => {
                    if !builder.can_build() {
                        let errors: Vec<String> = builder.validate().iter().filter_map(|m| match m {
                            llamafile_builder_core::ValidationMessage::Error(s) => Some(s.clone()),
                            _ => None,
                        }).collect();
                        Response::err(req.id, -32000, errors.join("; "))
                    } else {
                        let notif = Notification {
                            jsonrpc: "2.0", method: "progress",
                            params: serde_json::json!({"stage": "starting"}),
                        };
                        writeln!(stdout, "{}", serde_json::to_string(&notif).unwrap()).ok();
                        stdout.flush().ok();

                        match builder.build().await {
                            Ok(path) => Response::ok(req.id, serde_json::json!({
                                "output_path": path.to_string_lossy(),
                                "size": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                            })),
                            Err(e) => Response::err(req.id, -32000, format!("{e}")),
                        }
                    }
                }
                Err((code, msg)) => Response::err(req.id, code, msg),
            }
        } else {
            match session.handle(&req.method, &req.params) {
                Ok(result) => Response::ok(req.id, result),
                Err((code, msg)) => Response::err(req.id, code, msg),
            }
        };

        writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap()).ok();
        stdout.flush().ok();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    //! Direct tests of the JSON-RPC dispatcher (`Session::handle`). The async
    //! `build` method is not exercised here — it routes through `run()`'s main
    //! loop, and the build pipeline itself is covered end-to-end in
    //! `llamafile-builder-core::builder::tests`. These tests focus on the
    //! protocol surface: method dispatch, parameter validation, state
    //! mutations, and error classification.
    use super::*;
    use llamafile_builder_core::test_utils::test_config;
    use serde_json::json;

    fn session() -> (Session, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        (Session::new(test_config(), store), tmp)
    }

    fn ok(s: &mut Session, method: &str, params: Value) -> Value {
        s.handle(method, &params).unwrap_or_else(|(c, m)| panic!("{method} failed: ({c}) {m}"))
    }

    fn err(s: &mut Session, method: &str, params: Value) -> (i32, String) {
        s.handle(method, &params).err().unwrap_or_else(|| panic!("{method} should have failed"))
    }

    // ── Method dispatch ──

    #[test]
    fn unknown_method_returns_method_not_found() {
        let (mut s, _t) = session();
        let (code, msg) = err(&mut s, "no_such_method", json!({}));
        assert_eq!(code, -32601);
        assert!(msg.contains("no_such_method"));
    }

    #[test]
    fn build_method_is_routed_specially() {
        // `build` is handled outside `Session::handle` (in `run()`'s loop)
        // because it's async. The dispatcher returns a sentinel error so a
        // direct caller doesn't silently no-op.
        let (mut s, _t) = session();
        let (code, msg) = err(&mut s, "build", json!({}));
        assert_eq!(code, -32000);
        assert!(msg.contains("build_async"));
    }

    // ── Data queries ──

    #[test]
    fn list_versions_returns_configured_versions() {
        let (mut s, _t) = session();
        let v = ok(&mut s, "list_versions", json!({}));
        assert!(v.is_array() && !v.as_array().unwrap().is_empty());
    }

    #[test]
    fn list_models_filters_by_search() {
        let (mut s, _t) = session();
        let all = ok(&mut s, "list_models", json!({})).as_array().unwrap().len();
        let filtered = ok(&mut s, "list_models", json!({"search": "qwen"}));
        let n = filtered.as_array().unwrap().len();
        assert!(n > 0 && n <= all, "qwen filter should match a non-empty subset (got {n}/{all})");
    }

    #[test]
    fn show_model_round_trips_known_id() {
        let (mut s, _t) = session();
        let v = ok(&mut s, "show_model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"}));
        assert_eq!(v["id"].as_str(), Some("Qwen/Qwen3-0.6B-GGUF"));
    }

    #[test]
    fn show_model_unknown_id_errors() {
        let (mut s, _t) = session();
        let (code, msg) = err(&mut s, "show_model", json!({"id": "nonexistent/model"}));
        assert_eq!(code, -32000);
        assert!(msg.contains("nonexistent/model"));
    }

    #[test]
    fn show_model_missing_param_errors() {
        let (mut s, _t) = session();
        let (code, _) = err(&mut s, "show_model", json!({}));
        assert_eq!(code, -32602, "missing required param should be -32602");
    }

    // ── Recipe lifecycle ──

    #[test]
    fn recipe_create_list_load_delete_cycle() {
        let (mut s, _t) = session();

        // Empty initially
        let listed = ok(&mut s, "list_recipes", json!({}));
        assert_eq!(listed.as_array().unwrap().len(), 0);

        // Create
        let created = ok(&mut s, "create_recipe", json!({}));
        let id = created["id"].as_str().unwrap().to_string();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(s.current_recipe_id.map(|u| u.to_string()), Some(id.clone()));

        // List shows it as `current`
        let listed = ok(&mut s, "list_recipes", json!({}));
        let arr = listed.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["current"].as_bool(), Some(true));

        // Load it back (de-selects then re-selects via the loader)
        s.current_recipe_id = None;
        let loaded = ok(&mut s, "load_recipe", json!({"id": id}));
        assert_eq!(loaded["id"].as_str(), Some(id.as_str()));
        assert_eq!(s.current_recipe_id.map(|u| u.to_string()), Some(id.clone()));

        // Delete it; current_recipe_id is cleared
        ok(&mut s, "delete_recipe", json!({"id": id}));
        assert_eq!(s.current_recipe_id, None);
        let listed = ok(&mut s, "list_recipes", json!({}));
        assert_eq!(listed.as_array().unwrap().len(), 0);
    }

    #[test]
    fn load_recipe_unknown_id_errors() {
        let (mut s, _t) = session();
        let (code, _) = err(&mut s, "load_recipe", json!({"id": "does-not-exist"}));
        assert_eq!(code, -32000);
    }

    #[test]
    fn save_recipe_without_active_errors() {
        let (mut s, _t) = session();
        let (code, msg) = err(&mut s, "save_recipe", json!({"name": "anything"}));
        assert_eq!(code, -32000);
        assert!(msg.contains("no active recipe"));
    }

    // ── State mutations on the current recipe ──

    #[test]
    fn set_model_persists_to_current_recipe() {
        let (mut s, _t) = session();
        ok(&mut s, "set_model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"}));
        let state = ok(&mut s, "get_state", json!({}));
        assert_eq!(state["model_id"].as_str(), Some("Qwen/Qwen3-0.6B-GGUF"));
    }

    #[test]
    fn set_model_unknown_id_errors_without_creating_recipe() {
        let (mut s, _t) = session();
        let (code, _) = err(&mut s, "set_model", json!({"id": "nonexistent/model"}));
        assert_eq!(code, -32000);
        // Validation rejects the model before mutate_state runs, so no
        // recipe should have been auto-created.
        assert_eq!(s.current_recipe_id, None);
    }

    #[test]
    fn set_args_override_then_clear() {
        let (mut s, _t) = session();
        ok(&mut s, "set_args_override", json!({"content": "-m\n/zip/foo.gguf\n..."}));
        let state = ok(&mut s, "get_state", json!({}));
        assert_eq!(state["args_override"].as_str(), Some("-m\n/zip/foo.gguf\n..."));

        ok(&mut s, "clear_args_override", json!({}));
        let state = ok(&mut s, "get_state", json!({}));
        assert!(state["args_override"].is_null());
    }

    #[test]
    fn set_config_requires_value() {
        let (mut s, _t) = session();
        let (code, msg) = err(&mut s, "set_config", json!({"key": "context_length"}));
        assert_eq!(code, -32602);
        assert!(msg.contains("value"));
    }

    #[test]
    fn set_config_persists_value() {
        let (mut s, _t) = session();
        ok(&mut s, "set_config", json!({"key": "context_length", "value": 8192}));
        let state = ok(&mut s, "get_state", json!({}));
        assert_eq!(state["config_values"]["context_length"].as_i64(), Some(8192));
    }

    #[test]
    fn add_then_remove_gpu_lib_round_trips() {
        let (mut s, _t) = session();
        ok(&mut s, "add_gpu", json!({"spec": "cuda:linux"}));
        let state = ok(&mut s, "get_state", json!({}));
        assert_eq!(state["gpu_libs"], json!(["cuda:linux"]));

        ok(&mut s, "remove_gpu", json!({"spec": "cuda:linux"}));
        let state = ok(&mut s, "get_state", json!({}));
        // SavedState skips serializing an empty `gpu_libs`, so the field is
        // either absent (Null) or an empty array. Both mean "no GPU libs".
        let empty = state["gpu_libs"].as_array().map(|a| a.is_empty()).unwrap_or(true);
        assert!(empty, "gpu_libs should be empty after removal, got {:?}", state["gpu_libs"]);
    }

    #[test]
    fn add_gpu_invalid_spec_errors() {
        let (mut s, _t) = session();
        let (code, _) = err(&mut s, "add_gpu", json!({"spec": "fictional-accelerator"}));
        assert_eq!(code, -32000);
    }

    #[test]
    fn add_companion_file_then_remove() {
        let (mut s, _t) = session();
        ok(&mut s, "add_companion_file", json!({"role": "mmproj", "path": "/tmp/mm.gguf"}));
        let state = ok(&mut s, "get_state", json!({}));
        assert_eq!(state["custom_companion_files"]["mmproj"].as_str(), Some("/tmp/mm.gguf"));

        ok(&mut s, "remove_companion_file", json!({"role": "mmproj"}));
        let state = ok(&mut s, "get_state", json!({}));
        // Empty HashMap is skipped at serialization; either way the role is gone.
        let empty = state["custom_companion_files"].as_object().map(|o| o.is_empty()).unwrap_or(true);
        assert!(empty, "custom_companion_files should be empty after removal, got {:?}", state["custom_companion_files"]);
    }

    #[test]
    fn reset_clears_recipe_state() {
        let (mut s, _t) = session();
        ok(&mut s, "set_model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"}));
        ok(&mut s, "set_output", json!({"path": "/tmp/out.llamafile"}));
        ok(&mut s, "reset", json!({}));
        let state = ok(&mut s, "get_state", json!({}));
        assert!(state["model_id"].is_null());
        assert!(state["output_path"].is_null());
    }

    // ── Derived queries ──

    #[test]
    fn validate_reports_errors_for_bogus_custom_runtime() {
        let (mut s, _t) = session();
        // `apply_saved_state` fills in default version + model for a sparse
        // state, so an "empty" recipe actually validates clean. To exercise
        // the error path, point the runtime at a path that definitely does
        // not exist.
        ok(&mut s, "set_runtime", json!({"path": "/nonexistent/definitely-not-here"}));
        let msgs = ok(&mut s, "validate", json!({}));
        let arr = msgs.as_array().unwrap();
        let errors: Vec<_> = arr.iter()
            .filter(|m| m["level"].as_str() == Some("error"))
            .collect();
        assert!(
            !errors.is_empty(),
            "non-existent custom runtime should produce a validation error, got: {arr:?}",
        );
    }

    #[test]
    fn show_args_returns_renderable_args_after_full_setup() {
        let (mut s, _t) = session();
        // Pick the first version + first release from the bundled config so
        // this test doesn't go stale every release.
        let versions = ok(&mut s, "list_versions", json!({}));
        let group = versions[0]["name"].as_str().unwrap().to_string();
        let release = versions[0]["releases"][0]["version"].as_str().unwrap().to_string();
        ok(&mut s, "set_version", json!({"group": group, "version": release}));
        ok(&mut s, "set_model", json!({"id": "Qwen/Qwen3-0.6B-GGUF"}));

        let args = ok(&mut s, "show_args", json!({}));
        let s = args.as_str().unwrap();
        assert!(s.contains("-m"), "args should contain '-m' flag, got: {s}");
        assert!(s.contains(".gguf"), "args should reference a .gguf file");
    }
}

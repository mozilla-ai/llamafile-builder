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

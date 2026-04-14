use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Write};

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;

use crate::state_file::{self, SavedState};

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
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError { code, message }),
        }
    }
}

// ============================================================================
// Session
// ============================================================================

struct Session {
    config: BuilderConfig,
    saved: SavedState,
}

impl Session {
    fn new(config: BuilderConfig) -> Self {
        Self {
            config,
            saved: SavedState::default(),
        }
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, (i32, String)> {
        match method {
            // ── Queries ──
            "list_versions" => Ok(serde_json::to_value(&self.config.versions).unwrap()),
            "list_models" => {
                let search = params.get("search").and_then(|v| v.as_str());
                let tag = params.get("tag").and_then(|v| v.as_str());
                let models =
                    llamafile_core::search_models(&self.config.models, search, tag);
                Ok(serde_json::to_value(&models).unwrap())
            }
            "show_model" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or((-32602, "missing 'id' param".to_string()))?;
                let model = self
                    .config
                    .models
                    .iter()
                    .find(|m| m.id == id)
                    .ok_or((-32000, format!("model not found: {id}")))?;
                Ok(serde_json::to_value(model).unwrap())
            }
            "system_info" => {
                let info = llamafile_core::system_info();
                Ok(serde_json::to_value(&info).unwrap())
            }

            // ── State mutations ──
            "set_version" => {
                let group = str_param(params, "group")?;
                let version = str_param(params, "version")?;
                self.saved.version_group = Some(group);
                self.saved.version = Some(version);
                self.saved.custom_runtime = None;
                Ok(Value::String("ok".to_string()))
            }
            "set_model" => {
                let id = str_param(params, "id")?;
                if !self.config.models.iter().any(|m| m.id == id) {
                    return Err((-32000, format!("model not found: {id}")));
                }
                self.saved.model_id = Some(id);
                self.saved.custom_model_file = None;
                self.saved.quantizations.clear();
                Ok(Value::String("ok".to_string()))
            }
            "set_model_file" => {
                let path = str_param(params, "path")?;
                self.saved.custom_model_file = Some(path);
                self.saved.model_id = None;
                self.saved.quantizations.clear();
                Ok(Value::String("ok".to_string()))
            }
            "set_quantization" => {
                let role = str_param(params, "role")?;
                let name = str_param(params, "name")?;
                self.saved.quantizations.insert(role, name);
                Ok(Value::String("ok".to_string()))
            }
            "set_config" => {
                let key = str_param(params, "key")?;
                let value = params
                    .get("value")
                    .cloned()
                    .ok_or((-32602, "missing 'value' param".to_string()))?;
                self.saved.config_values.insert(key, value);
                Ok(Value::String("ok".to_string()))
            }
            "set_output" => {
                let path = str_param(params, "path")?;
                self.saved.output_path = Some(path);
                Ok(Value::String("ok".to_string()))
            }
            "set_args_file" => {
                let path = str_param(params, "path")?;
                self.saved.args_file = Some(path);
                Ok(Value::String("ok".to_string()))
            }
            "add_gpu" => {
                let spec = str_param(params, "spec")?;
                crate::commands::build::parse_gpu_spec(&spec)
                    .map_err(|e| (-32000, e))?;
                if !self.saved.gpu_libs.contains(&spec) {
                    self.saved.gpu_libs.push(spec);
                }
                Ok(Value::String("ok".to_string()))
            }
            "remove_gpu" => {
                let spec = str_param(params, "spec")?;
                self.saved.gpu_libs.retain(|s| s != &spec);
                Ok(Value::String("ok".to_string()))
            }
            "set_runtime" => {
                let path = str_param(params, "path")?;
                self.saved.custom_runtime = Some(path);
                self.saved.version = None;
                self.saved.version_group = None;
                Ok(Value::String("ok".to_string()))
            }
            "set_zipalign" => {
                let path = str_param(params, "path")?;
                self.saved.custom_zipalign = Some(path);
                Ok(Value::String("ok".to_string()))
            }

            // ── State queries ──
            "get_state" => Ok(serde_json::to_value(&self.saved).unwrap()),
            "show_args" => {
                let builder = self.make_builder()?;
                Ok(Value::String(builder.resolved_args()))
            }
            "validate" => {
                let builder = self.make_builder()?;
                let msgs = builder.validate();
                let json_msgs: Vec<Value> = msgs
                    .iter()
                    .map(|m| match m {
                        llamafile_core::ValidationMessage::Warning(s) => {
                            serde_json::json!({"level": "warning", "message": s})
                        }
                        llamafile_core::ValidationMessage::Error(s) => {
                            serde_json::json!({"level": "error", "message": s})
                        }
                    })
                    .collect();
                Ok(Value::Array(json_msgs))
            }
            "resolved_components" => {
                let builder = self.make_builder()?;
                let components = builder.resolved_components();
                let json: Vec<Value> = components
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "role": format!("{:?}", c.role),
                            "source": c.source_display(),
                            "zip_path": c.zip_path,
                        })
                    })
                    .collect();
                Ok(Value::Array(json))
            }
            "config_schema" => {
                Ok(serde_json::to_value(&self.config.config_schema).unwrap())
            }
            "reset" => {
                self.saved = SavedState::default();
                Ok(Value::String("ok".to_string()))
            }

            // build is handled separately (async)
            "build" => Err((-32000, "use build_async".to_string())),

            _ => Err((-32601, format!("method not found: {method}"))),
        }
    }

    fn make_builder(&self) -> Result<llamafile_core::Builder, (i32, String)> {
        let cache = Cache::new(crate::get_cache_dir());
        state_file::build_from_state(&self.saved, self.config.clone(), cache)
            .map_err(|e| (-32000, e))
    }
}

fn str_param(params: &Value, key: &str) -> Result<String, (i32, String)> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or((-32602, format!("missing '{key}' param")))
}

// ============================================================================
// Main loop
// ============================================================================

pub async fn run(config: BuilderConfig) -> Result<(), String> {
    let mut session = Session::new(config);
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
            // Async build with progress notifications
            match session.make_builder() {
                Ok(builder) => {
                    if !builder.can_build() {
                        let msgs = builder.validate();
                        let errors: Vec<String> = msgs
                            .iter()
                            .filter_map(|m| match m {
                                llamafile_core::ValidationMessage::Error(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect();
                        Response::err(req.id, -32000, errors.join("; "))
                    } else {
                        // Send progress notifications via tracing -> stdout
                        let notif = Notification {
                            jsonrpc: "2.0",
                            method: "progress",
                            params: serde_json::json!({"stage": "starting"}),
                        };
                        writeln!(stdout, "{}", serde_json::to_string(&notif).unwrap()).ok();
                        stdout.flush().ok();

                        match builder.build().await {
                            Ok(path) => Response::ok(
                                req.id,
                                serde_json::json!({
                                    "output_path": path.to_string_lossy(),
                                    "size": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                                }),
                            ),
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

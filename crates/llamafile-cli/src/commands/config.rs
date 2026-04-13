use std::path::Path;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::Builder;

use crate::cli::ConfigCmd;
use crate::state_file::{self, SavedState};

pub fn run(
    cmd: &ConfigCmd,
    config: &BuilderConfig,
    state_path: &Path,
    json: bool,
) -> Result<(), String> {
    let mut saved = SavedState::load(state_path).unwrap_or_default();

    match cmd {
        ConfigCmd::SetVersion { group, version } => {
            saved.version_group = Some(group.clone());
            saved.version = Some(version.clone());
            saved.custom_runtime = None;
            saved.save(state_path)?;
            tracing::info!("version set to {} / {}", group, version);
        }
        ConfigCmd::SetModel { id } => {
            // Verify model exists
            if !config.models.iter().any(|m| m.id == *id) {
                return Err(format!("Model '{}' not found in registry", id));
            }
            saved.model_id = Some(id.clone());
            saved.custom_model_file = None;
            saved.quantizations.clear();
            saved.save(state_path)?;
            tracing::info!("model set to {}", id);
        }
        ConfigCmd::SetModelFile { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            saved.custom_model_file = Some(path.to_string_lossy().to_string());
            saved.model_id = None;
            saved.quantizations.clear();
            saved.save(state_path)?;
            tracing::info!("custom model file set to {}", path.display());
        }
        ConfigCmd::SetQuantization { role, name } => {
            saved.quantizations.insert(role.clone(), name.clone());
            saved.save(state_path)?;
            tracing::info!("quantization for '{}' set to '{}'", role, name);
        }
        ConfigCmd::SetOutput { path } => {
            saved.output_path = Some(path.to_string_lossy().to_string());
            saved.save(state_path)?;
            tracing::info!("output path set to {}", path.display());
        }
        ConfigCmd::Set { key, value } => {
            let parsed: serde_json::Value = serde_json::from_str(value)
                .unwrap_or_else(|_| serde_json::Value::String(value.clone()));
            saved.config_values.insert(key.clone(), parsed);
            saved.save(state_path)?;
            tracing::info!("config '{}' set", key);
        }
        ConfigCmd::SetArgsFile { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            saved.args_file = Some(path.to_string_lossy().to_string());
            saved.save(state_path)?;
            tracing::info!("args file set to {}", path.display());
        }
        ConfigCmd::AddGpu { spec } => {
            if !saved.gpu_libs.contains(spec) {
                saved.gpu_libs.push(spec.clone());
            }
            saved.save(state_path)?;
            tracing::info!("GPU library added: {}", spec);
        }
        ConfigCmd::RemoveGpu { spec } => {
            saved.gpu_libs.retain(|s| s != spec);
            saved.save(state_path)?;
            tracing::info!("GPU library removed: {}", spec);
        }
        ConfigCmd::SetRuntime { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            saved.custom_runtime = Some(path.to_string_lossy().to_string());
            saved.version_group = None;
            saved.version = None;
            saved.save(state_path)?;
            tracing::info!("custom runtime set to {}", path.display());
        }
        ConfigCmd::SetZipalign { path } => {
            if !path.exists() {
                return Err(format!("File not found: {}", path.display()));
            }
            saved.custom_zipalign = Some(path.to_string_lossy().to_string());
            saved.save(state_path)?;
            tracing::info!("custom zipalign set to {}", path.display());
        }
        ConfigCmd::Show => {
            if json {
                println!("{}", serde_json::to_string_pretty(&saved).unwrap());
                return Ok(());
            }

            let cache = Cache::new(crate::get_cache_dir());
            let builder = match state_file::build_from_state(&saved, config.clone(), cache) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Warning: {e}");
                    // Fall back to empty builder for display
                    Builder::new(config.clone(), Cache::new(crate::get_cache_dir()))
                }
            };

            // Version
            if let Some(ref runtime) = saved.custom_runtime {
                println!("Version:       custom ({})", runtime);
            } else if let (Some(ref g), Some(ref v)) = (&saved.version_group, &saved.version) {
                println!("Version:       {} / {}", g, v);
            } else if let Some(ref g) = builder.state().selected_version_group {
                let v = builder.state().selected_release_version.as_deref().unwrap_or("?");
                println!("Version:       {} / {} (default)", g, v);
            } else {
                println!("Version:       (none available)");
            }

            // Model
            if let Some(ref path) = saved.custom_model_file {
                println!("Model:         custom ({})", path);
            } else {
                let model_id = saved.model_id.as_deref()
                    .or(builder.state().selected_model_id.as_deref());
                let is_default = saved.model_id.is_none();
                if let Some(id) = model_id {
                    if let Some(model) = config.models.iter().find(|m| m.id == id) {
                        let quant_info: Vec<String> = model
                            .files
                            .iter()
                            .map(|f| {
                                let selected = saved
                                    .quantizations
                                    .get(&f.role)
                                    .map(|n| n.as_str())
                                    .or_else(|| f.quantizations.first().map(|q| q.name.as_str()))
                                    .unwrap_or("?");
                                format!("{}:{}", f.role, selected)
                            })
                            .collect();
                        let tag = if is_default { " (default)" } else { "" };
                        println!("Model:         {} ({}){}", model.name, quant_info.join(", "), tag);
                    } else {
                        println!("Model:         {} (not found in registry)", id);
                    }
                } else {
                    println!("Model:         (none available)");
                }
            }

            // Output
            if let Some(ref path) = saved.output_path {
                println!("Output:        {}", path);
            } else if !builder.state().output_path.is_empty() {
                println!("Output:        {} (auto)", builder.state().output_path);
            }

            // GPU
            if saved.gpu_libs.is_empty() {
                println!("GPU libs:      (none)");
            } else {
                println!("GPU libs:      {}", saved.gpu_libs.join(", "));
            }

            // Args override
            if let Some(ref path) = saved.args_file {
                println!("Args file:     {} (overrides config below)", path);
            }

            // Config values: show ALL effective values (defaults + overrides)
            let all_values = llamafile_core::args::default_config_values(&config.config_schema);
            println!();
            println!("Configuration (.args parameters):");
            let mut keys: Vec<_> = all_values.keys().collect();
            keys.sort();
            for key in keys {
                let default_val = &all_values[key];
                let is_overridden = saved.config_values.contains_key(key.as_str());
                let effective = saved.config_values.get(key.as_str()).unwrap_or(default_val);
                if is_overridden {
                    println!("  {} = {} (was: {})", key, effective, default_val);
                } else {
                    println!("  {} = {}", key, effective);
                }
            }

            // Custom binaries
            if let Some(ref path) = saved.custom_zipalign {
                println!("\nZipalign:      {}", path);
            }

            // Validation
            let msgs = builder.validate();
            let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
            let warnings: Vec<_> = msgs.iter().filter(|m| !m.is_error()).collect();
            if !errors.is_empty() || !warnings.is_empty() {
                println!();
            }
            for e in &errors {
                println!("ERROR: {:?}", e);
            }
            for w in &warnings {
                println!("WARNING: {:?}", w);
            }
            if errors.is_empty() {
                println!("\nReady to build.");
            }
        }
        ConfigCmd::ShowArgs => {
            let cache = Cache::new(crate::get_cache_dir());
            let builder = state_file::build_from_state(&saved, config.clone(), cache)?;
            println!("{}", builder.resolved_args());
        }
        ConfigCmd::Reset => {
            SavedState::delete(state_path);
            tracing::info!("build configuration reset");
        }
    }

    Ok(())
}

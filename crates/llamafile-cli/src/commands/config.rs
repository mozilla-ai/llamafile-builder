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
            if saved.is_empty() {
                println!("No build configuration saved. Use 'config set-*' commands to configure.");
                return Ok(());
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&saved).unwrap());
                return Ok(());
            }

            // Build a temporary Builder to show resolved state
            let cache = Cache::new(crate::get_cache_dir());
            let mut builder = Builder::new(config.clone(), cache);
            if let Err(e) = state_file::apply_to_builder(&saved, &mut builder) {
                eprintln!("Warning: {e}");
            }

            // Version
            if let Some(ref runtime) = saved.custom_runtime {
                println!("Version:       custom ({})", runtime);
            } else if let (Some(ref g), Some(ref v)) = (&saved.version_group, &saved.version) {
                println!("Version:       {} / {}", g, v);
            } else {
                println!("Version:       (not set)");
            }

            // Model
            if let Some(ref path) = saved.custom_model_file {
                println!("Model:         custom ({})", path);
            } else if let Some(ref id) = saved.model_id {
                if let Some(model) = config.models.iter().find(|m| m.id == *id) {
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
                    println!("Model:         {} ({})", model.name, quant_info.join(", "));
                } else {
                    println!("Model:         {} (not found in registry)", id);
                }
            } else {
                println!("Model:         (not set)");
            }

            // Output
            if let Some(ref path) = saved.output_path {
                println!("Output:        {}", path);
            } else if !builder.state().output_path.is_empty() {
                println!("Output:        {} (auto)", builder.state().output_path);
            } else {
                println!("Output:        (not set)");
            }

            // GPU
            if saved.gpu_libs.is_empty() {
                println!("GPU libs:      (none)");
            } else {
                println!("GPU libs:      {}", saved.gpu_libs.join(", "));
            }

            // Config values
            if !saved.config_values.is_empty() {
                let pairs: Vec<String> = saved
                    .config_values
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                println!("Config:        {}", pairs.join(", "));
            }

            // Args override
            if let Some(ref path) = saved.args_file {
                println!("Args file:     {}", path);
            }

            // Custom binaries
            if let Some(ref path) = saved.custom_zipalign {
                println!("Zipalign:      {}", path);
            }

            // Validation
            let msgs = builder.validate();
            let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
            let warnings: Vec<_> = msgs.iter().filter(|m| !m.is_error()).collect();
            if !errors.is_empty() {
                println!();
                for e in &errors {
                    println!("ERROR: {:?}", e);
                }
            }
            if !warnings.is_empty() {
                for w in &warnings {
                    println!("WARNING: {:?}", w);
                }
            }
            if errors.is_empty() {
                println!("\nReady to build.");
            }
        }
        ConfigCmd::ShowArgs => {
            let cache = Cache::new(crate::get_cache_dir());
            let mut builder = Builder::new(config.clone(), cache);
            if let Err(e) = state_file::apply_to_builder(&saved, &mut builder) {
                return Err(e);
            }
            println!("{}", builder.resolved_args());
        }
        ConfigCmd::Reset => {
            SavedState::delete(state_path);
            tracing::info!("build configuration reset");
        }
    }

    Ok(())
}

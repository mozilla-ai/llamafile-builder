use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::models::{GpuAccel, Os};

use crate::cli::BuildArgs;
use crate::state_file::{self, SavedState};

pub async fn run(args: &BuildArgs, config: BuilderConfig, cache: Cache) -> Result<(), String> {
    // Build saved state: either from CLI args (one-shot) or from current recipe (multi-step)
    let saved = if let Some(ref model) = args.model {
        saved_state_from_args(args, model)?
    } else {
        let store = llamafile_core::ConfigStore::new(crate::get_configs_dir());
        let config_dir = crate::get_config_dir();
        let recipe = state_file::get_or_create_current_recipe(&store, &config_dir)?;
        let mut s = recipe.state;
        merge_cli_overrides(&mut s, args)?;
        s
    };

    let builder = state_file::build_from_state(&saved, config, cache)?;

    // Validate
    let msgs = builder.validate();
    for msg in &msgs {
        match msg {
            llamafile_core::validation::ValidationMessage::Warning(s) => {
                tracing::warn!("{s}");
            }
            llamafile_core::validation::ValidationMessage::Error(s) => {
                tracing::error!("{s}");
            }
        }
    }
    if !builder.can_build() {
        return Err("Build validation failed. Fix the errors above.".to_string());
    }

    builder.build().await.map_err(|e| format!("{e}"))?;

    Ok(())
}

/// Convert one-shot CLI args into a SavedState (not persisted).
fn saved_state_from_args(args: &BuildArgs, model: &str) -> Result<SavedState, String> {
    let mut saved = SavedState::default();

    // Version
    if let Some(ref runtime) = args.runtime {
        saved.custom_runtime = Some(runtime.to_string_lossy().to_string());
    } else if let Some(ref v) = args.version {
        // We don't know the group name from just the version string.
        // Store it as version only; build_from_state will need to resolve it.
        // For now, search through config at apply time -- store a marker.
        saved.version = Some(v.clone());
    }
    // If neither runtime nor version specified, defaults will apply

    // Model
    let model_path = std::path::Path::new(model);
    if model_path.exists() && model_path.extension().is_some_and(|e| e == "gguf") {
        saved.custom_model_file = Some(model.to_string());
    } else {
        saved.model_id = Some(model.to_string());
    }

    // Quantization
    if let Some(ref q) = args.quantization {
        saved.quantizations.insert("weights".to_string(), q.clone());
    }
    if let Some(ref q) = args.mmproj_quantization {
        saved.quantizations.insert("mmproj".to_string(), q.clone());
    }

    // Output
    if let Some(ref output) = args.output {
        saved.output_path = Some(output.to_string_lossy().to_string());
    }

    // Context length
    if let Some(ctx) = args.context_length {
        saved
            .config_values
            .insert("context_length".to_string(), serde_json::json!(ctx));
    }

    // Args file
    if let Some(ref path) = args.args_file {
        saved.args_file = Some(path.to_string_lossy().to_string());
    }

    // GPU
    for gpu in &args.gpu {
        // Validate the spec early
        parse_gpu_spec(gpu)?;
        saved.gpu_libs.push(gpu.clone());
    }

    // Custom zipalign
    if let Some(ref path) = args.zipalign {
        saved.custom_zipalign = Some(path.to_string_lossy().to_string());
    }

    Ok(saved)
}

/// Merge CLI args as overrides into an existing SavedState (multi-step mode).
/// Only non-None/non-empty args are applied.
fn merge_cli_overrides(saved: &mut SavedState, args: &BuildArgs) -> Result<(), String> {
    if let Some(ref v) = args.version {
        saved.version = Some(v.clone());
        saved.version_group = None; // will be resolved by build_from_state
    }
    if let Some(ref runtime) = args.runtime {
        saved.custom_runtime = Some(runtime.to_string_lossy().to_string());
        saved.version = None;
        saved.version_group = None;
    }
    if let Some(ref output) = args.output {
        saved.output_path = Some(output.to_string_lossy().to_string());
    }
    if let Some(ctx) = args.context_length {
        saved.config_values.insert("context_length".to_string(), serde_json::json!(ctx));
    }
    if let Some(ref q) = args.quantization {
        saved.quantizations.insert("weights".to_string(), q.clone());
    }
    if let Some(ref q) = args.mmproj_quantization {
        saved.quantizations.insert("mmproj".to_string(), q.clone());
    }
    if let Some(ref path) = args.args_file {
        saved.args_file = Some(path.to_string_lossy().to_string());
    }
    for gpu in &args.gpu {
        parse_gpu_spec(gpu)?;
        if !saved.gpu_libs.contains(gpu) {
            saved.gpu_libs.push(gpu.clone());
        }
    }
    if let Some(ref path) = args.zipalign {
        saved.custom_zipalign = Some(path.to_string_lossy().to_string());
    }
    Ok(())
}

pub fn parse_gpu_spec(spec: &str) -> Result<(GpuAccel, Vec<Os>), String> {
    let parts: Vec<&str> = spec.split(':').collect();
    let accel = match parts[0].to_lowercase().as_str() {
        "cuda" => GpuAccel::Cuda,
        "rocm" => GpuAccel::Rocm,
        "vulkan" => GpuAccel::Vulkan,
        other => {
            return Err(format!(
                "Unknown GPU type: '{}'. Use cuda, rocm, or vulkan.",
                other
            ))
        }
    };

    let os_list = if parts.len() > 1 {
        match parts[1].to_lowercase().as_str() {
            "linux" => vec![Os::Linux],
            "windows" => vec![Os::Windows],
            "all" => vec![Os::Linux, Os::Windows],
            other => {
                return Err(format!(
                    "Unknown OS: '{}'. Use linux, windows, or all.",
                    other
                ))
            }
        }
    } else if cfg!(target_os = "linux") {
        vec![Os::Linux]
    } else if cfg!(target_os = "windows") {
        vec![Os::Windows]
    } else {
        vec![Os::Linux]
    };

    Ok((accel, os_list))
}

use std::path::Path;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::models::{GpuAccel, Os};
use llamafile_core::Builder;

use crate::cli::BuildArgs;
use crate::state_file::{self, SavedState};

pub async fn run(args: &BuildArgs, config: BuilderConfig, cache: Cache) -> Result<(), String> {
    let mut builder = Builder::new(config, cache);

    if let Some(ref model) = args.model {
        // One-shot mode: all options from command line
        configure_from_args(&mut builder, args, model)?;
    } else {
        // Multi-step mode: load saved state
        let state_path = crate::get_state_path();
        let saved = SavedState::load(&state_path).ok_or(
            "No --model specified and no saved configuration found.\n\
             Use 'config set-model <id>' first, or pass --model on the command line."
                .to_string(),
        )?;
        if saved.is_empty() {
            return Err(
                "Saved configuration is empty. Use 'config set-*' commands first.".to_string(),
            );
        }
        state_file::apply_to_builder(&saved, &mut builder)?;

        // CLI args still override saved state where provided
        if let Some(ref v) = args.version {
            let found = find_version(&builder, v)?;
            builder.select_version(&found.0, &found.1);
        }
        if let Some(ref output) = args.output {
            builder.set_output_path(output);
        }
        if let Some(ctx) = args.context_length {
            builder.set_config_value("context_length", serde_json::json!(ctx));
        }
        if let Some(ref args_file) = args.args_file {
            let content = std::fs::read_to_string(args_file)
                .map_err(|e| format!("Failed to read args file: {e}"))?;
            builder.set_args_override(&content);
        }
        for gpu in &args.gpu {
            let (accel, os_list) = parse_gpu_spec(gpu)?;
            for os in os_list {
                builder.add_gpu_lib(accel, os);
            }
        }
        if let Some(ref zipalign) = args.zipalign {
            builder.use_custom_zipalign(zipalign);
        }
    }

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

/// Configure builder entirely from CLI arguments (one-shot mode).
fn configure_from_args(
    builder: &mut Builder,
    args: &BuildArgs,
    model: &str,
) -> Result<(), String> {
    // Select version
    if let Some(ref runtime) = args.runtime {
        builder.use_custom_runtime(runtime);
    } else if let Some(ref v) = args.version {
        let found = find_version(builder, v)?;
        builder.select_version(&found.0, &found.1);
    } else if !builder.select_default_version() {
        return Err("No llamafile versions available in config.".to_string());
    }

    // Select model
    let model_path = Path::new(model);
    if model_path.exists() && model_path.extension().is_some_and(|e| e == "gguf") {
        builder.select_custom_model(model_path);
    } else {
        builder.select_registry_model(model);
        if builder.state().selected_model_id.is_none() {
            return Err(format!(
                "Model '{}' not found in registry. Use 'models' to list available models.",
                model
            ));
        }
    }

    // Select quantization
    if let Some(ref quant_name) = args.quantization {
        if let Some(model_id) = builder.state().selected_model_id.clone() {
            if let Some(m) = builder.config().models.iter().find(|m| m.id == model_id) {
                if let Some(weights_file) = m.files.iter().find(|f| f.role == "weights") {
                    if let Some(idx) = weights_file
                        .quantizations
                        .iter()
                        .position(|q| q.name.eq_ignore_ascii_case(quant_name))
                    {
                        builder.select_quantization("weights", idx);
                    } else {
                        let available: Vec<_> = weights_file
                            .quantizations
                            .iter()
                            .map(|q| q.name.as_str())
                            .collect();
                        return Err(format!(
                            "Quantization '{}' not found. Available: {}",
                            quant_name,
                            available.join(", ")
                        ));
                    }
                }
            }
        }
    }

    if let Some(ref output) = args.output {
        builder.set_output_path(output);
    }
    if let Some(ctx) = args.context_length {
        builder.set_config_value("context_length", serde_json::json!(ctx));
    }
    if let Some(ref args_file) = args.args_file {
        let content = std::fs::read_to_string(args_file)
            .map_err(|e| format!("Failed to read args file: {e}"))?;
        builder.set_args_override(&content);
    }
    for gpu in &args.gpu {
        let (accel, os_list) = parse_gpu_spec(gpu)?;
        for os in os_list {
            builder.add_gpu_lib(accel, os);
        }
    }
    if let Some(ref zipalign) = args.zipalign {
        builder.use_custom_zipalign(zipalign);
    }

    Ok(())
}

fn find_version(builder: &Builder, version_str: &str) -> Result<(String, String), String> {
    for group in &builder.config().versions {
        for release in &group.releases {
            if release.version.contains(version_str) {
                return Ok((group.name.clone(), release.version.clone()));
            }
        }
    }
    Err(format!(
        "Version '{}' not found. Use 'versions' to list available versions.",
        version_str
    ))
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

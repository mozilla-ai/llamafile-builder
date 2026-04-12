use std::path::Path;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::models::{GpuAccel, Os};
use llamafile_core::Builder;

use crate::cli::BuildArgs;

pub async fn run(args: &BuildArgs, config: BuilderConfig, cache: Cache) -> Result<(), String> {
    let mut builder = Builder::new(config, cache);

    // Select version
    if let Some(ref runtime) = args.runtime {
        builder.use_custom_runtime(runtime);
    } else if let Some(ref v) = args.version {
        let found = find_version(&builder, v)?;
        builder.select_version(&found.0, &found.1);
    } else {
        if !builder.select_default_version() {
            return Err("No llamafile versions available in config.".to_string());
        }
    }

    // Select model
    let model_path = Path::new(&args.model);
    if model_path.exists() && model_path.extension().is_some_and(|e| e == "gguf") {
        builder.select_custom_model(model_path);
    } else {
        builder.select_registry_model(&args.model);
        if builder.state().selected_model_id.is_none() {
            return Err(format!(
                "Model '{}' not found in registry. Use 'models' to list available models.",
                args.model
            ));
        }
    }

    // Select quantization
    if let Some(ref quant_name) = args.quantization {
        if let Some(model_id) = builder.state().selected_model_id.clone() {
            if let Some(model) = builder.config().models.iter().find(|m| m.id == model_id) {
                if let Some(weights_file) = model.files.iter().find(|f| f.role == "weights") {
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

    // Set output path
    if let Some(ref output) = args.output {
        builder.set_output_path(output);
    }

    // Override context length
    if let Some(ctx) = args.context_length {
        builder.set_config_value("context_length", serde_json::json!(ctx));
    }

    // Custom .args file
    if let Some(ref args_file) = args.args_file {
        let content = std::fs::read_to_string(args_file)
            .map_err(|e| format!("Failed to read args file: {e}"))?;
        builder.set_args_override(&content);
    }

    // GPU libraries
    for gpu in &args.gpu {
        let (accel, os_list) = parse_gpu_spec(gpu)?;
        for os in os_list {
            builder.add_gpu_lib(accel, os);
        }
    }

    // Custom zipalign
    if let Some(ref zipalign) = args.zipalign {
        builder.use_custom_zipalign(zipalign);
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

    // Run the build -- all progress is via tracing
    builder.build().await.map_err(|e| format!("{e}"))?;

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

fn parse_gpu_spec(spec: &str) -> Result<(GpuAccel, Vec<Os>), String> {
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

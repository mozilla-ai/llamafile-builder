use std::path::Path;

// Re-export SavedState from core — single source of truth
pub use llamafile_core::config_store::SavedState;
use llamafile_core::config_store::{BuildRecipe, ConfigStore};

/// Create a fully configured Builder from saved state + defaults.
/// This is the single place where defaults are applied at build time.
pub fn build_from_state(
    saved: &SavedState,
    config: llamafile_core::BuilderConfig,
    cache: llamafile_core::Cache,
) -> Result<llamafile_core::Builder, String> {
    let mut builder = llamafile_core::Builder::new(config, cache);

    // Apply defaults for anything not explicitly configured
    if saved.version_group.is_none() && saved.version.is_none() && saved.custom_runtime.is_none() {
        builder.select_default_version();
    }
    let has_model = saved.model_id.is_some()
        || saved.custom_model_file.is_some()
        || saved.ollama_model.is_some()
        || saved.lmstudio_model.is_some();
    if !has_model {
        builder.select_default_model();
    }

    // Apply saved overrides on top
    apply_to_builder(saved, &mut builder)?;

    Ok(builder)
}

/// Apply saved state to a Builder (without defaults — use build_from_state instead).
fn apply_to_builder(
    saved: &SavedState,
    builder: &mut llamafile_core::Builder,
) -> Result<(), String> {
    // Version
    if let Some(ref runtime) = saved.custom_runtime {
        builder.use_custom_runtime(std::path::Path::new(runtime));
    } else if let (Some(ref group), Some(ref version)) =
        (&saved.version_group, &saved.version)
    {
        builder.select_version(group, version);
    } else if let Some(ref version) = saved.version {
        let found = builder.config().versions.iter().find_map(|g| {
            g.releases
                .iter()
                .find(|r| r.version.contains(version.as_str()))
                .map(|r| (g.name.clone(), r.version.clone()))
        });
        if let Some((group, ver)) = found {
            builder.select_version(&group, &ver);
        } else {
            return Err(format!(
                "Version '{}' not found. Use 'versions' to list available versions.",
                version
            ));
        }
    }

    // Model
    if let Some(ref path) = saved.custom_model_file {
        builder.select_custom_model(std::path::Path::new(path));
    } else if let Some(ref model) = saved.ollama_model {
        builder.select_ollama_model(model.clone());
    } else if let Some(ref model) = saved.lmstudio_model {
        builder.select_lmstudio_model(model.clone());
    } else if let Some(ref id) = saved.model_id {
        builder.select_registry_model(id);
        if builder.state().selected_model_id.is_none() {
            return Err(format!("Model '{}' not found in registry", id));
        }
    }

    // Quantizations
    for (role, quant_name) in &saved.quantizations {
        if let Some(model_id) = builder.state().selected_model_id.clone() {
            if let Some(model) = builder.config().models.iter().find(|m| m.id == model_id) {
                if let Some(file) = model.files.iter().find(|f| f.role == *role) {
                    if let Some(idx) = file
                        .quantizations
                        .iter()
                        .position(|q| q.name.eq_ignore_ascii_case(quant_name))
                    {
                        builder.select_quantization(role, idx);
                    } else {
                        return Err(format!(
                            "Quantization '{}' not found for role '{}'",
                            quant_name, role
                        ));
                    }
                }
            }
        }
    }

    // Config values
    for (key, value) in &saved.config_values {
        builder.set_config_value(key, value.clone());
    }

    // Args override
    if let Some(ref args_file) = saved.args_file {
        let content = std::fs::read_to_string(args_file)
            .map_err(|e| format!("Failed to read args file '{}': {e}", args_file))?;
        builder.set_args_override(&content);
    }

    // Output path
    if let Some(ref path) = saved.output_path {
        builder.set_output_path(std::path::Path::new(path));
    }

    // GPU libs
    for spec in &saved.gpu_libs {
        let (accel, os_list) = crate::commands::build::parse_gpu_spec(spec)?;
        for os in os_list {
            builder.add_gpu_lib(accel, os);
        }
    }

    // Custom binaries
    if let Some(ref path) = saved.custom_zipalign {
        builder.use_custom_zipalign(std::path::Path::new(path));
    }

    Ok(())
}

/// Path to the file that tracks which recipe is "current" for the CLI.
pub fn current_recipe_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("current-recipe")
}

/// Load the current recipe ID from the pointer file.
pub fn load_current_recipe_id(config_dir: &Path) -> Option<uuid::Uuid> {
    let path = current_recipe_path(config_dir);
    let content = std::fs::read_to_string(path).ok()?;
    uuid::Uuid::parse_str(content.trim()).ok()
}

/// Save the current recipe ID to the pointer file.
pub fn save_current_recipe_id(config_dir: &Path, id: uuid::Uuid) -> Result<(), String> {
    let path = current_recipe_path(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }
    std::fs::write(&path, id.to_string())
        .map_err(|e| format!("Failed to write current recipe pointer: {e}"))?;
    Ok(())
}

/// Clear the current recipe pointer.
pub fn clear_current_recipe(config_dir: &Path) {
    let _ = std::fs::remove_file(current_recipe_path(config_dir));
}

/// Get or create the current working recipe.
/// If a current recipe ID exists and is valid, returns it.
/// Otherwise creates a new one and sets it as current.
pub fn get_or_create_current_recipe(
    store: &ConfigStore,
    config_dir: &Path,
) -> Result<BuildRecipe, String> {
    if let Some(id) = load_current_recipe_id(config_dir) {
        if let Ok(recipe) = store.get(id) {
            return Ok(recipe);
        }
    }
    // Create new
    let recipe = store.create().map_err(|e| format!("{e}"))?;
    save_current_recipe_id(config_dir, recipe.id)?;
    Ok(recipe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn test_config() -> llamafile_core::BuilderConfig {
        llamafile_core::BuilderConfig {
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

    fn test_cache() -> llamafile_core::Cache {
        llamafile_core::Cache::new(std::path::PathBuf::from("/tmp/test-cache"))
    }

    #[test]
    fn build_from_state_with_explicit_config() {
        let saved = SavedState {
            version_group: Some("llamafile 0.10".to_string()),
            version: Some("0.10.0".to_string()),
            model_id: Some("Qwen/Qwen3-0.6B-GGUF".to_string()),
            output_path: Some("/tmp/test.llamafile".to_string()),
            config_values: {
                let mut m = HashMap::new();
                m.insert("context_length".to_string(), serde_json::json!(16384));
                m
            },
            ..Default::default()
        };

        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        assert!(builder.state().selected_model_id.is_some());
        assert!(builder.state().resolved_llamafile_path.is_some());
        assert!(builder.can_build());
    }

    #[test]
    fn build_from_state_empty_uses_defaults() {
        let saved = SavedState::default();
        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        assert!(builder.state().selected_version_group.is_some());
        assert!(builder.state().selected_model_id.is_some());
        assert!(builder.can_build());
    }

    #[test]
    fn current_recipe_pointer_round_trips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let id = uuid::Uuid::new_v4();
        save_current_recipe_id(tmp.path(), id).unwrap();
        assert_eq!(load_current_recipe_id(tmp.path()), Some(id));

        clear_current_recipe(tmp.path());
        assert_eq!(load_current_recipe_id(tmp.path()), None);
    }
}

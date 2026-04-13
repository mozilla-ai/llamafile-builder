use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Persisted build configuration for multi-step CLI usage.
/// Saved to ~/.llamafile-builder/build-state.json between invocations.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SavedState {
    // Version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_runtime: Option<String>,

    // Model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_model_file: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub quantizations: HashMap<String, String>, // role -> quant name

    // Config
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_values: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_file: Option<String>,

    // Output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,

    // GPU
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_libs: Vec<String>, // "cuda", "cuda:linux", "rocm:windows", etc.

    // Custom binaries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_zipalign: Option<String>,
}

impl SavedState {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize state: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("Failed to write state file: {e}"))?;
        Ok(())
    }

    pub fn delete(path: &std::path::Path) {
        let _ = std::fs::remove_file(path);
    }

    pub fn is_empty(&self) -> bool {
        self.version_group.is_none()
            && self.version.is_none()
            && self.custom_runtime.is_none()
            && self.model_id.is_none()
            && self.custom_model_file.is_none()
            && self.quantizations.is_empty()
            && self.config_values.is_empty()
            && self.args_file.is_none()
            && self.output_path.is_none()
            && self.gpu_libs.is_empty()
            && self.custom_zipalign.is_none()
    }
}

/// Apply saved state to a Builder.
pub fn apply_to_builder(
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
    }

    // Model
    if let Some(ref path) = saved.custom_model_file {
        builder.select_custom_model(std::path::Path::new(path));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_state_round_trips_through_json() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path();

        let state = SavedState {
            version_group: Some("llamafile 0.10".to_string()),
            version: Some("0.10.0".to_string()),
            model_id: Some("Qwen/Qwen3-0.6B-GGUF".to_string()),
            output_path: Some("/tmp/out.llamafile".to_string()),
            gpu_libs: vec!["cuda:linux".to_string()],
            config_values: {
                let mut m = HashMap::new();
                m.insert("context_length".to_string(), serde_json::json!(8192));
                m.insert("server_enabled".to_string(), serde_json::json!(false));
                m
            },
            ..Default::default()
        };

        state.save(path).unwrap();
        let loaded = SavedState::load(path).expect("should load");

        assert_eq!(loaded.version_group.as_deref(), Some("llamafile 0.10"));
        assert_eq!(loaded.version.as_deref(), Some("0.10.0"));
        assert_eq!(loaded.model_id.as_deref(), Some("Qwen/Qwen3-0.6B-GGUF"));
        assert_eq!(loaded.output_path.as_deref(), Some("/tmp/out.llamafile"));
        assert_eq!(loaded.gpu_libs, vec!["cuda:linux"]);
        assert_eq!(loaded.config_values.get("context_length"), Some(&serde_json::json!(8192)));
        assert_eq!(loaded.config_values.get("server_enabled"), Some(&serde_json::json!(false)));
        assert!(loaded.custom_runtime.is_none());
        assert!(loaded.custom_model_file.is_none());
    }

    #[test]
    fn apply_to_builder_sets_version_and_model() {
        let config = llamafile_core::config_manager::parse_config_data::<Vec<llamafile_core::LlamafileVersionGroup>>(
            include_str!("../../../data/versions.json"),
        ).unwrap();
        let models = llamafile_core::config_manager::parse_config_data::<Vec<llamafile_core::Model>>(
            include_str!("../../../data/models.json"),
        ).unwrap();
        let schema = llamafile_core::config_manager::parse_config_data::<Vec<llamafile_core::ConfigSchemaItem>>(
            include_str!("../../../data/config.json"),
        ).unwrap();

        let builder_config = llamafile_core::BuilderConfig {
            versions: config,
            models,
            config_schema: schema,
            config_version: 1,
            min_builder_version: None,
        };
        let cache = llamafile_core::Cache::new(std::path::PathBuf::from("/tmp/test-cache"));
        let mut builder = llamafile_core::Builder::new(builder_config, cache);

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

        apply_to_builder(&saved, &mut builder).unwrap();

        assert_eq!(builder.state().selected_model_id.as_deref(), Some("Qwen/Qwen3-0.6B-GGUF"));
        assert!(builder.state().resolved_llamafile_path.is_some());
        assert_eq!(builder.state().output_path, "/tmp/test.llamafile");
        assert_eq!(
            builder.state().config_values.get("context_length"),
            Some(&serde_json::json!(16384))
        );
        assert!(builder.can_build());
    }
}

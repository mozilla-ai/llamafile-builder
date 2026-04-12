use crate::config_manager::BuilderConfig;
use crate::state::BuildState;
use crate::zipalign;

/// A validation message -- either a warning (build can proceed) or an error (cannot proceed).
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationMessage {
    Warning(String),
    Error(String),
}

impl ValidationMessage {
    pub fn is_error(&self) -> bool {
        matches!(self, ValidationMessage::Error(_))
    }
}

/// Validate a build state against the loaded configuration.
/// Returns a list of warnings and errors.
pub fn validate(state: &BuildState, config: &BuilderConfig) -> Vec<ValidationMessage> {
    let mut messages = Vec::new();

    // ── Required components ──

    // Model must be selected
    if state.resolved_weights_filename.is_none() && state.selected_model_id.is_none() {
        // Check if there's any model selection at all
        let has_model = state.selected_model_id.is_some()
            || !state.custom_model_files.is_empty()
            || state.selected_ollama_model.is_some()
            || state.selected_lmstudio_model.is_some();
        if !has_model {
            messages.push(ValidationMessage::Error("No model selected".to_string()));
        }
    }

    // Version must be selected (unless using custom binary)
    if state.resolved_llamafile_path.is_none() {
        messages.push(ValidationMessage::Error(
            "No llamafile version selected".to_string(),
        ));
    }

    // Output path must be set
    if state.output_path.is_empty() {
        messages.push(ValidationMessage::Error(
            "No output path specified".to_string(),
        ));
    }

    // ── Context size vs model training length ──

    if let Some(model_id) = &state.selected_model_id {
        if let Some(model) = config.models.iter().find(|m| &m.id == model_id) {
            if let Some(ctx_value) = state.config_values.get("context_length") {
                let ctx = ctx_value.as_u64().unwrap_or(0) as u32;
                if ctx > model.context_length && model.context_length > 0 {
                    messages.push(ValidationMessage::Warning(format!(
                        "Context length {} exceeds model's training context length {}. \
                         This may produce degraded output quality.",
                        ctx, model.context_length
                    )));
                }
            }
        }
    }

    // ── Large context on CPU ──

    if let Some(ctx_value) = state.config_values.get("context_length") {
        let ctx = ctx_value.as_u64().unwrap_or(0);
        if ctx >= 32768 {
            messages.push(ValidationMessage::Warning(
                "Large context length (32K+) may require significant memory \
                 and be slow on CPU-only builds."
                    .to_string(),
            ));
        }
    }

    // ── Custom binary paths exist and are executable ──

    if let Some(ref path) = state.custom_llamafile_path {
        let p = std::path::Path::new(path);
        if !p.exists() {
            messages.push(ValidationMessage::Error(format!(
                "Custom llamafile binary not found: {}",
                path
            )));
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(p) {
                    if meta.permissions().mode() & 0o111 == 0 {
                        messages.push(ValidationMessage::Warning(format!(
                            "Custom llamafile binary is not executable: {}",
                            path
                        )));
                    }
                }
            }
        }
    }

    // ── Zipalign available ──

    let zipalign_path = zipalign::find_zipalign();
    if zipalign_path.to_str() == Some("zipalign") {
        // find_zipalign fell through to PATH -- check if it's actually in PATH
        if which_in_path("zipalign").is_none() {
            messages.push(ValidationMessage::Error(
                "zipalign binary not found. It must be in PATH or next to the \
                 llamafile-builder binary. zipalign is included in llamafile releases."
                    .to_string(),
            ));
        }
    }

    messages
}

fn which_in_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Model;
    use crate::state::LlamafileSource;

    fn empty_config() -> BuilderConfig {
        BuilderConfig {
            versions: vec![],
            models: vec![],
            config_schema: vec![],
            config_version: 1,
            min_builder_version: None,
        }
    }

    fn config_with_model(id: &str, context_length: u32) -> BuilderConfig {
        BuilderConfig {
            versions: vec![],
            models: vec![Model {
                id: id.to_string(),
                name: id.to_string(),
                organization: "test".to_string(),
                parameters: "1B".to_string(),
                param_count: 1_000_000_000,
                description: String::new(),
                license: "MIT".to_string(),
                huggingface_url: None,
                tags: vec![],
                context_length,
                default: false,
                files: vec![],
            }],
            config_schema: vec![],
            config_version: 1,
            min_builder_version: None,
        }
    }

    #[test]
    fn empty_state_produces_errors() {
        let state = BuildState::default();
        let config = empty_config();
        let msgs = validate(&state, &config);

        let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
        assert!(errors.len() >= 2); // no model, no version
    }

    #[test]
    fn valid_state_produces_no_errors() {
        let mut state = BuildState::default();
        state.selected_model_id = Some("test-model".to_string());
        state.resolved_weights_filename = Some("model.gguf".to_string());
        state.resolved_llamafile_path = Some("/path/to/llamafile".to_string());
        state.output_path = "/tmp/output.llamafile".to_string();

        let config = empty_config();
        let msgs = validate(&state, &config);

        let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
        assert!(errors.is_empty());
    }

    #[test]
    fn context_length_warning() {
        let mut state = BuildState::default();
        state.selected_model_id = Some("m".to_string());
        state.resolved_weights_filename = Some("m.gguf".to_string());
        state.resolved_llamafile_path = Some("/path".to_string());
        state.output_path = "/tmp/out.llamafile".to_string();
        state.config_values.insert(
            "context_length".to_string(),
            serde_json::json!(16384),
        );

        let config = config_with_model("m", 4096);
        let msgs = validate(&state, &config);

        let warnings: Vec<_> = msgs
            .iter()
            .filter(|m| matches!(m, ValidationMessage::Warning(s) if s.contains("exceeds")))
            .collect();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn missing_custom_binary_is_error() {
        let mut state = BuildState::default();
        state.selected_model_id = Some("m".to_string());
        state.resolved_weights_filename = Some("m.gguf".to_string());
        state.llamafile_source = LlamafileSource::Custom;
        state.custom_llamafile_path = Some("/nonexistent/llamafile".to_string());
        state.resolved_llamafile_path = Some("/nonexistent/llamafile".to_string());
        state.output_path = "/tmp/out.llamafile".to_string();

        let config = empty_config();
        let msgs = validate(&state, &config);

        let errors: Vec<_> = msgs
            .iter()
            .filter(|m| matches!(m, ValidationMessage::Error(s) if s.contains("not found")))
            .collect();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn large_context_warning() {
        let mut state = BuildState::default();
        state.selected_model_id = Some("m".to_string());
        state.resolved_weights_filename = Some("m.gguf".to_string());
        state.resolved_llamafile_path = Some("/path".to_string());
        state.output_path = "/tmp/out.llamafile".to_string();
        state.config_values.insert(
            "context_length".to_string(),
            serde_json::json!(65536),
        );

        let config = empty_config();
        let msgs = validate(&state, &config);

        let warnings: Vec<_> = msgs
            .iter()
            .filter(|m| matches!(m, ValidationMessage::Warning(s) if s.contains("32K")))
            .collect();
        assert_eq!(warnings.len(), 1);
    }
}

use std::collections::HashMap;
use std::path::PathBuf;

use llamafile_core::config_manager::BuilderConfig;
use llamafile_core::models::*;
use llamafile_core::cache::Cache;
use llamafile_core::Builder;

/// Which section of the dashboard is focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Version,
    Model,
    Weights,
    Mmproj,
    Output,
    Config,
    Gpu,
    Build,
    ShowArgs,
    Quit,
}

impl Focus {
    /// Get the description for the status bar.
    pub fn description(self) -> &'static str {
        match self {
            Focus::Version => "Select the llamafile runtime version to use",
            Focus::Model => "Choose a model from the registry or a local file",
            Focus::Weights => "Select quantization level for model weights",
            Focus::Mmproj => "Select quantization for the vision adapter (mmproj)",
            Focus::Output => "Set the output path for the built llamafile",
            Focus::Config => "Edit runtime configuration (.args parameters)",
            Focus::Gpu => "Add or remove GPU acceleration libraries",
            Focus::Build => "Download components and assemble the llamafile",
            Focus::ShowArgs => "Preview the .args file that will be bundled",
            Focus::Quit => "Exit the application",
        }
    }

    /// Navigate to next focusable section, skipping disabled ones.
    pub fn next(self, has_mmproj: bool) -> Focus {
        let all = Self::ordered();
        let idx = all.iter().position(|&f| f == self).unwrap_or(0);
        for offset in 1..all.len() {
            let candidate = all[(idx + offset) % all.len()];
            if candidate == Focus::Mmproj && !has_mmproj {
                continue;
            }
            return candidate;
        }
        self
    }

    /// Navigate to previous focusable section, skipping disabled ones.
    pub fn prev(self, has_mmproj: bool) -> Focus {
        let all = Self::ordered();
        let idx = all.iter().position(|&f| f == self).unwrap_or(0);
        for offset in 1..all.len() {
            let candidate = all[(idx + all.len() - offset) % all.len()];
            if candidate == Focus::Mmproj && !has_mmproj {
                continue;
            }
            return candidate;
        }
        self
    }

    fn ordered() -> &'static [Focus] {
        &[
            Focus::Version,
            Focus::Model,
            Focus::Weights,
            Focus::Mmproj,
            Focus::Output,
            Focus::Config,
            Focus::Gpu,
            Focus::Build,
            Focus::ShowArgs,
            Focus::Quit,
        ]
    }
}

/// What popup overlay is showing, if any.
#[derive(Debug, Clone)]
pub enum Popup {
    None,
    VersionSelect { selected: usize },
    ModelSelect { search: String, selected: usize },
    QuantSelect { role: String, options: Vec<String>, selected: usize },
    OutputEdit { input: String },
    ConfigEdit {
        entries: Vec<crate::config_ui::ConfigEntry>,
        selected: usize,
        editing: bool,
        edit_buf: String,
        dropdown_open: bool,
        dropdown_selected: usize,
    },
    GpuEdit { specs: Vec<String>, selected: usize, adding: bool, add_buf: String },
    ArgsPreview { content: String },
    Confirm { message: String, action: ConfirmAction },
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    Quit,
}

/// Build status.
#[derive(Debug, Clone)]
pub enum BuildStatus {
    Idle,
    Building { phase: String, progress: f32 },
    Complete { path: String, size: u64 },
    Failed { error: String },
}

/// Log level filter for the log pane view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn raise(self) -> Self {
        match self {
            LogLevel::Debug => LogLevel::Info,
            LogLevel::Info => LogLevel::Warn,
            LogLevel::Warn => LogLevel::Error,
            LogLevel::Error => LogLevel::Error,
        }
    }

    pub fn lower(self) -> Self {
        match self {
            LogLevel::Debug => LogLevel::Debug,
            LogLevel::Info => LogLevel::Debug,
            LogLevel::Warn => LogLevel::Info,
            LogLevel::Error => LogLevel::Warn,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// A log entry with its level, for filtering.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
}

/// The full TUI application state.
pub struct App {
    pub config: BuilderConfig,
    pub cache_dir: PathBuf,

    // Current selections (mirrors SavedState)
    pub version_group: Option<String>,
    pub version: Option<String>,
    pub custom_runtime: Option<String>,
    pub model_id: Option<String>,
    pub custom_model_file: Option<String>,
    pub quantizations: HashMap<String, String>,
    pub config_values: HashMap<String, serde_json::Value>,
    pub output_path: Option<String>,
    pub gpu_libs: Vec<String>,
    pub custom_zipalign: Option<String>,
    pub args_file: Option<String>,

    // UI state
    pub focus: Focus,
    pub popup: Popup,
    pub build_status: BuildStatus,
    pub logs: Vec<LogEntry>,
    pub log_expanded: bool,
    pub log_level: LogLevel,
    pub should_quit: bool,
}

impl App {
    pub fn new(config: BuilderConfig, cache_dir: PathBuf) -> Self {
        let mut app = Self {
            config,
            cache_dir,
            version_group: None,
            version: None,
            custom_runtime: None,
            model_id: None,
            custom_model_file: None,
            quantizations: HashMap::new(),
            config_values: HashMap::new(),
            output_path: None,
            gpu_libs: Vec::new(),
            custom_zipalign: None,
            args_file: None,
            focus: Focus::Version,
            popup: Popup::None,
            build_status: BuildStatus::Idle,
            logs: Vec::new(),
            log_expanded: false,
            log_level: LogLevel::Info,
            should_quit: false,
        };
        // Apply defaults
        app.apply_defaults();
        app
    }

    fn apply_defaults(&mut self) {
        // Default version
        if let Some(group) = self.config.versions.iter().find(|g| g.default) {
            if let Some(release) = group.releases.first() {
                self.version_group = Some(group.name.clone());
                self.version = Some(release.version.clone());
            }
        } else if let Some(group) = self.config.versions.first() {
            if let Some(release) = group.releases.first() {
                self.version_group = Some(group.name.clone());
                self.version = Some(release.version.clone());
            }
        }
        // Default model
        if let Some(model) = self.config.models.iter().find(|m| m.default) {
            self.model_id = Some(model.id.clone());
        } else if let Some(model) = self.config.models.first() {
            self.model_id = Some(model.id.clone());
        }
    }

    /// Get the currently selected model, if any.
    pub fn selected_model(&self) -> Option<&Model> {
        self.model_id.as_ref().and_then(|id| {
            self.config.models.iter().find(|m| &m.id == id)
        })
    }

    /// Whether the selected model has an mmproj file.
    pub fn has_mmproj(&self) -> bool {
        self.selected_model()
            .map(|m| m.files.iter().any(|f| f.role == "mmproj"))
            .unwrap_or(false)
    }

    /// Get the display name for the selected version.
    pub fn version_display(&self) -> String {
        if let Some(ref runtime) = self.custom_runtime {
            format!("custom ({})", runtime)
        } else if let (Some(ref g), Some(ref v)) = (&self.version_group, &self.version) {
            format!("{} / {}", g, v)
        } else {
            "(none)".to_string()
        }
    }

    /// Get quant display for a role.
    pub fn quant_for_role(&self, role: &str) -> String {
        if let Some(name) = self.quantizations.get(role) {
            return name.clone();
        }
        if let Some(model) = self.selected_model() {
            if let Some(file) = model.files.iter().find(|f| f.role == role) {
                if let Some(q) = file.quantizations.first() {
                    return format!("{} (default)", q.name);
                }
            }
        }
        "(none)".to_string()
    }

    /// Get the effective output path.
    pub fn effective_output(&self) -> String {
        if let Some(ref p) = self.output_path {
            return p.clone();
        }
        // Compute auto path from model
        if let Some(model) = self.selected_model() {
            if let Some(file) = model.files.iter().find(|f| f.role == "weights") {
                let quant_name = self.quantizations.get("weights")
                    .map(|s| s.as_str())
                    .or_else(|| file.quantizations.first().map(|q| q.name.as_str()))
                    .unwrap_or("model");
                if let Some(q) = file.quantizations.iter().find(|q| q.name == quant_name)
                    .or_else(|| file.quantizations.first())
                {
                    let stem = q.filename.trim_end_matches(".gguf").trim_end_matches(".GGUF");
                    let dir = llamafile_core::models::get_default_output_dir();
                    return format!("{}/{}.llamafile", dir, stem);
                }
            }
        }
        "(not set)".to_string()
    }

    /// Get config summary string.
    pub fn config_summary(&self) -> String {
        let defaults = llamafile_core::args::default_config_values(&self.config.config_schema);
        let mut parts = Vec::new();

        let show_keys = ["server_enabled", "context_length", "temperature"];
        for key in show_keys {
            let val = self.config_values.get(key).or_else(|| defaults.get(key));
            if let Some(v) = val {
                let overridden = self.config_values.contains_key(key);
                let display = match v {
                    serde_json::Value::Bool(b) => {
                        if key == "server_enabled" {
                            if *b { "server=on" } else { "server=off" }.to_string()
                        } else {
                            format!("{}={}", key, b)
                        }
                    }
                    serde_json::Value::Number(n) => format!("{}={}", key, n),
                    serde_json::Value::String(s) => format!("{}={}", key, s),
                    _ => format!("{}={}", key, v),
                };
                if overridden {
                    parts.push(format!("{}*", display));
                } else {
                    parts.push(display);
                }
            }
        }
        parts.join("  ")
    }

    /// Build a Builder from the current state.
    pub fn make_builder(&self) -> Result<Builder, String> {
        let cache = Cache::new(self.cache_dir.clone());
        let mut builder = Builder::new(self.config.clone(), cache);

        // Version
        if let Some(ref runtime) = self.custom_runtime {
            builder.use_custom_runtime(std::path::Path::new(runtime));
        } else if let (Some(ref g), Some(ref v)) = (&self.version_group, &self.version) {
            builder.select_version(g, v);
        } else {
            builder.select_default_version();
        }

        // Model
        if let Some(ref path) = self.custom_model_file {
            builder.select_custom_model(std::path::Path::new(path));
        } else if let Some(ref id) = self.model_id {
            builder.select_registry_model(id);
        } else {
            builder.select_default_model();
        }

        // Quantizations
        for (role, name) in &self.quantizations {
            if let Some(model_id) = builder.state().selected_model_id.clone() {
                if let Some(model) = builder.config().models.iter().find(|m| m.id == model_id) {
                    if let Some(file) = model.files.iter().find(|f| f.role == *role) {
                        if let Some(idx) = file.quantizations.iter().position(|q| q.name.eq_ignore_ascii_case(name)) {
                            builder.select_quantization(role, idx);
                        }
                    }
                }
            }
        }

        // Config values
        for (key, value) in &self.config_values {
            builder.set_config_value(key, value.clone());
        }

        // Output
        if let Some(ref path) = self.output_path {
            builder.set_output_path(std::path::Path::new(path));
        }

        // GPU
        for spec in &self.gpu_libs {
            if let Ok((accel, os_list)) = crate::parse_gpu_spec(spec) {
                for os in os_list {
                    builder.add_gpu_lib(accel, os);
                }
            }
        }

        // Custom zipalign
        if let Some(ref path) = self.custom_zipalign {
            builder.use_custom_zipalign(std::path::Path::new(path));
        }

        // Args file
        if let Some(ref path) = self.args_file {
            if let Ok(content) = std::fs::read_to_string(path) {
                builder.set_args_override(&content);
            }
        }

        Ok(builder)
    }

    pub fn add_log(&mut self, level: LogLevel, msg: String) {
        self.logs.push(LogEntry { level, message: msg });
        // Keep last 1000 entries
        if self.logs.len() > 1000 {
            self.logs.drain(0..self.logs.len() - 1000);
        }
    }

    /// Get log entries filtered by the current log level.
    pub fn filtered_logs(&self) -> Vec<&LogEntry> {
        self.logs.iter().filter(|e| e.level >= self.log_level).collect()
    }

    pub fn validation_errors(&self) -> Vec<String> {
        match self.make_builder() {
            Ok(builder) => builder
                .validate()
                .iter()
                .filter_map(|m| match m {
                    llamafile_core::ValidationMessage::Error(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            Err(e) => vec![e],
        }
    }

    pub fn can_build(&self) -> bool {
        self.validation_errors().is_empty()
    }
}

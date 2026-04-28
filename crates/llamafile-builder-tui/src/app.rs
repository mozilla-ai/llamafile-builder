use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use llamafile_builder_core::config_manager::BuilderConfig;
use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore, SavedState};
use llamafile_builder_core::models::*;
use llamafile_builder_core::cache::Cache;
use llamafile_builder_core::Builder;

/// An available GPU library from the selected release.
#[derive(Debug, Clone)]
pub struct GpuLibOption {
    pub accel: GpuAccel,
    pub os: Os,
    pub size: u64,
}

impl GpuLibOption {
    pub fn label(&self) -> String {
        let accel_desc = match self.accel {
            GpuAccel::Cuda => "CUDA — NVIDIA GPUs",
            GpuAccel::Rocm => "ROCm — AMD GPUs",
            GpuAccel::Vulkan => "Vulkan",
        };
        let os_desc = match self.os {
            Os::Linux => "Linux (.so)",
            Os::Windows => "Windows (.dll)",
        };
        format!(
            "{} — {} — {}",
            accel_desc, os_desc, format_size(self.size),
        )
    }
}

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
    pub fn next(self, has_mmproj: bool, args_overridden: bool, local_model: bool) -> Focus {
        let all = Self::ordered();
        let idx = all.iter().position(|&f| f == self).unwrap_or(0);
        for offset in 1..all.len() {
            let candidate = all[(idx + offset) % all.len()];
            if (candidate == Focus::Weights || candidate == Focus::Mmproj) && local_model {
                continue;
            }
            if candidate == Focus::Mmproj && !has_mmproj {
                continue;
            }
            if candidate == Focus::Config && args_overridden {
                continue;
            }
            return candidate;
        }
        self
    }

    /// Navigate to previous focusable section, skipping disabled ones.
    pub fn prev(self, has_mmproj: bool, args_overridden: bool, local_model: bool) -> Focus {
        let all = Self::ordered();
        let idx = all.iter().position(|&f| f == self).unwrap_or(0);
        for offset in 1..all.len() {
            let candidate = all[(idx + all.len() - offset) % all.len()];
            if (candidate == Focus::Weights || candidate == Focus::Mmproj) && local_model {
                continue;
            }
            if candidate == Focus::Mmproj && !has_mmproj {
                continue;
            }
            if candidate == Focus::Config && args_overridden {
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

/// Which model source tab is active in the model popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTab {
    Registry,
    Ollama,
    LmStudio,
}

impl ModelTab {
    pub fn label(self) -> &'static str {
        match self {
            ModelTab::Registry => "Registry",
            ModelTab::Ollama => "Ollama",
            ModelTab::LmStudio => "LM Studio",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ModelTab::Registry => ModelTab::Ollama,
            ModelTab::Ollama => ModelTab::LmStudio,
            ModelTab::LmStudio => ModelTab::Registry,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ModelTab::Registry => ModelTab::LmStudio,
            ModelTab::Ollama => ModelTab::Registry,
            ModelTab::LmStudio => ModelTab::Ollama,
        }
    }
}

/// What popup overlay is showing, if any.
#[derive(Debug, Clone)]
pub enum Popup {
    None,
    VersionSelect { selected: usize },
    ModelSelect {
        tab: ModelTab,
        search: String,
        selected: usize,
        ollama_models: Vec<OllamaModel>,
        lmstudio_models: Vec<LMStudioModel>,
    },
    QuantSelect { role: String, options: Vec<String>, selected: usize },
    OutputEdit { textarea: tui_textarea::TextArea<'static>, original: String },
    ConfigEdit {
        entries: Vec<crate::config_ui::ConfigEntry>,
        selected: usize,
        editing: bool,
        edit_buf: String,
        dropdown_open: bool,
        dropdown_selected: usize,
    },
    GpuSelect { available: Vec<GpuLibOption>, selected: Vec<bool>, cursor: usize },
    ArgsEdit { textarea: tui_textarea::TextArea<'static>, original: String },
    SaveRecipe { input: String },
    LoadRecipe { recipes: Vec<BuildRecipe>, selected: usize },
    Confirm { message: String, action: ConfirmAction },
    /// Modal shown when a build fails. Displays the error text with a single
    /// dismiss key. The full error is also written to the log pane.
    BuildError { message: String },
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    /// Cancel the running build, then quit. Sets the cancel flag so download
    /// tasks tear down cleanly. (Quitting while idle goes through
    /// `should_quit` directly without a confirmation.)
    CancelBuildAndQuit,
}

/// Build status.
#[derive(Debug, Clone)]
pub enum BuildStatus {
    Idle,
    Building { phase: String, progress: f32 },
    Complete { path: String, size: u64 },
    Failed { error: String },
}

/// Current download progress, parsed from tracing events.
#[derive(Debug, Clone)]
pub struct DownloadInfo {
    pub filename: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub percentage: f32,
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
    pub store: ConfigStore,
    pub recipe_id: Option<uuid::Uuid>,

    // Current selections (mirrors SavedState)
    pub version_group: Option<String>,
    pub version: Option<String>,
    pub custom_runtime: Option<String>,
    pub model_source: ModelTab,
    pub model_id: Option<String>,
    pub custom_model_file: Option<String>,
    pub ollama_model: Option<OllamaModel>,
    pub lmstudio_model: Option<LMStudioModel>,
    pub quantizations: HashMap<String, String>,
    pub config_values: HashMap<String, serde_json::Value>,
    pub output_path: Option<String>,
    pub gpu_libs: Vec<(GpuAccel, Os)>,
    pub custom_zipalign: Option<String>,
    pub args_file: Option<String>,
    pub args_override: Option<String>,
    pub mmproj_disabled: bool,

    // UI state
    pub focus: Focus,
    pub popup: Popup,
    pub build_status: BuildStatus,
    pub build_started_at: Option<Instant>,
    pub download_info: Option<DownloadInfo>,
    pub logs: Vec<LogEntry>,
    pub log_expanded: bool,
    pub log_level: LogLevel,
    pub log_scroll_offset: u16, // scroll offset from the bottom (0 = tail)
    pub should_quit: bool,
}

impl App {
    pub fn new(config: BuilderConfig, cache_dir: PathBuf, store: ConfigStore) -> Self {
        let mut app = Self {
            config,
            cache_dir,
            store,
            recipe_id: None,
            version_group: None,
            version: None,
            custom_runtime: None,
            model_source: ModelTab::Registry,
            model_id: None,
            custom_model_file: None,
            ollama_model: None,
            lmstudio_model: None,
            quantizations: HashMap::new(),
            config_values: HashMap::new(),
            output_path: None,
            gpu_libs: Vec::new(),
            custom_zipalign: None,
            args_file: None,
            args_override: None,
            mmproj_disabled: false,
            focus: Focus::Version,
            popup: Popup::None,
            build_status: BuildStatus::Idle,
            build_started_at: None,
            download_info: None,
            logs: Vec::new(),
            log_expanded: false,
            log_level: LogLevel::Info,
            log_scroll_offset: 0,
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

    /// Whether the model comes from a local source (Ollama/LM Studio) with no quantization choices.
    pub fn is_local_model(&self) -> bool {
        self.model_source == ModelTab::Ollama || self.model_source == ModelTab::LmStudio
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
        // Compute auto path from model source
        let dir = llamafile_builder_core::models::get_default_output_dir();
        if let Some(ref ollama) = self.ollama_model {
            let stem = format!("{}_{}", ollama.model_name, ollama.tag);
            return format!("{}/{}.llamafile", dir, stem);
        }
        if let Some(ref lms) = self.lmstudio_model {
            let stem = lms.dir_name.replace('/', "_");
            return format!("{}/{}.llamafile", dir, stem);
        }
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
                    return format!("{}/{}.llamafile", dir, stem);
                }
            }
        }
        "(not set)".to_string()
    }

    /// Get config summary string.
    pub fn config_summary(&self) -> String {
        if self.args_override.is_some() {
            return "manually overridden, check .args".to_string();
        }

        let defaults = llamafile_builder_core::args::default_config_values(&self.config.config_schema);
        let mut parts = Vec::new();

        // Mode: server+CLI or CLI-only
        let server_on = self.config_values.get("server_enabled")
            .or_else(|| defaults.get("server_enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let mode = if server_on { "mode=Server" } else { "mode=Combined" };
        parts.push(mode.to_string());

        // Other key params
        for key in ["context_length", "temperature"] {
            let val = self.config_values.get(key).or_else(|| defaults.get(key));
            if let Some(v) = val {
                let display = match v {
                    serde_json::Value::Number(n) => format!("{}={}", key, n),
                    serde_json::Value::String(s) => format!("{}={}", key, s),
                    _ => format!("{}={}", key, v),
                };
                parts.push(display);
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
        } else if let Some(ref model) = self.ollama_model {
            builder.select_ollama_model(model.clone());
        } else if let Some(ref model) = self.lmstudio_model {
            builder.select_lmstudio_model(model.clone());
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
        for &(accel, os) in &self.gpu_libs {
            builder.add_gpu_lib(accel, os);
        }

        // Custom zipalign
        if let Some(ref path) = self.custom_zipalign {
            builder.use_custom_zipalign(std::path::Path::new(path));
        }

        // Disable mmproj if user toggled it off
        if self.mmproj_disabled {
            builder.disable_file_role("mmproj");
        }

        // Args override (from manual editing or file)
        if let Some(ref content) = self.args_override {
            builder.set_args_override(content);
        } else if let Some(ref path) = self.args_file {
            if let Ok(content) = std::fs::read_to_string(path) {
                builder.set_args_override(&content);
            }
        }

        Ok(builder)
    }

    /// Get the currently selected release.
    pub fn selected_release(&self) -> Option<&LlamafileRelease> {
        let group_name = self.version_group.as_ref()?;
        let version = self.version.as_ref()?;
        let group = self.config.versions.iter().find(|g| &g.name == group_name)?;
        group.releases.iter().find(|r| &r.version == version)
    }

    /// Enumerate GPU libraries available in the selected release.
    pub fn available_gpu_libs(&self) -> Vec<GpuLibOption> {
        let mut out = Vec::new();
        let release = match self.selected_release() {
            Some(r) => r,
            None => return out,
        };
        let gpu_libs = match release.gpu_libs.as_ref() {
            Some(g) => g,
            None => return out,
        };
        for (accel, by_platform) in [
            (GpuAccel::Cuda, gpu_libs.cuda.as_ref()),
            (GpuAccel::Rocm, gpu_libs.rocm.as_ref()),
            (GpuAccel::Vulkan, gpu_libs.vulkan.as_ref()),
        ] {
            if let Some(bp) = by_platform {
                if let Some(lib) = &bp.linux {
                    out.push(GpuLibOption { accel, os: Os::Linux, size: lib.size_bytes });
                }
                if let Some(lib) = &bp.windows {
                    out.push(GpuLibOption { accel, os: Os::Windows, size: lib.size_bytes });
                }
            }
        }
        out
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
                    llamafile_builder_core::ValidationMessage::Error(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            Err(e) => vec![e],
        }
    }

    pub fn can_build(&self) -> bool {
        self.validation_errors().is_empty()
    }

    /// Convert current App fields to a SavedState for persistence.
    pub fn to_saved_state(&self) -> SavedState {
        // Convert typed gpu_libs back to string specs
        let gpu_libs: Vec<String> = self.gpu_libs.iter().map(|(accel, os)| {
            let a = match accel {
                GpuAccel::Cuda => "cuda",
                GpuAccel::Rocm => "rocm",
                GpuAccel::Vulkan => "vulkan",
            };
            let o = match os {
                Os::Linux => "linux",
                Os::Windows => "windows",
            };
            format!("{}:{}", a, o)
        }).collect();

        SavedState {
            version_group: self.version_group.clone(),
            version: self.version.clone(),
            custom_runtime: self.custom_runtime.clone(),
            model_id: self.model_id.clone(),
            custom_model_file: self.custom_model_file.clone(),
            ollama_model: self.ollama_model.clone(),
            lmstudio_model: self.lmstudio_model.clone(),
            quantizations: self.quantizations.clone(),
            config_values: self.config_values.clone(),
            args_file: self.args_file.clone(),
            output_path: self.output_path.clone(),
            gpu_libs,
            custom_gpu_libs: vec![],
            custom_companion_files: Default::default(),
            custom_zipalign: self.custom_zipalign.clone(),
        }
    }

    /// Load App fields from a BuildRecipe.
    pub fn load_from_recipe(&mut self, recipe: &BuildRecipe) {
        let s = &recipe.state;
        self.version_group = s.version_group.clone();
        self.version = s.version.clone();
        self.custom_runtime = s.custom_runtime.clone();
        self.model_id = s.model_id.clone();
        self.custom_model_file = s.custom_model_file.clone();
        self.ollama_model = s.ollama_model.clone();
        self.lmstudio_model = s.lmstudio_model.clone();
        self.quantizations = s.quantizations.clone();
        self.config_values = s.config_values.clone();
        self.args_file = s.args_file.clone();
        self.output_path = s.output_path.clone();
        self.custom_zipalign = s.custom_zipalign.clone();
        self.args_override = None;
        self.mmproj_disabled = false;

        // Convert string gpu specs to typed tuples
        self.gpu_libs = s.gpu_libs.iter().filter_map(|spec| {
            let parts: Vec<&str> = spec.split(':').collect();
            let accel = match parts[0] {
                "cuda" => Some(GpuAccel::Cuda),
                "rocm" => Some(GpuAccel::Rocm),
                "vulkan" => Some(GpuAccel::Vulkan),
                _ => None,
            }?;
            let os = if parts.len() > 1 {
                match parts[1] {
                    "linux" => Os::Linux,
                    "windows" => Os::Windows,
                    _ => return None,
                }
            } else {
                Os::Linux
            };
            Some((accel, os))
        }).collect();

        // Set model source tab
        if self.ollama_model.is_some() {
            self.model_source = ModelTab::Ollama;
        } else if self.lmstudio_model.is_some() {
            self.model_source = ModelTab::LmStudio;
        } else {
            self.model_source = ModelTab::Registry;
        }

        self.recipe_id = Some(recipe.id);
    }

    /// Save current state to a named recipe.
    pub fn save_recipe(&mut self, name: &str) -> Result<(), String> {
        let id = match self.recipe_id {
            Some(id) => id,
            None => {
                let recipe = self.store.create().map_err(|e| format!("{e}"))?;
                self.recipe_id = Some(recipe.id);
                recipe.id
            }
        };
        let mut recipe = self.store.get(id).map_err(|e| format!("{e}"))?;
        recipe.state = self.to_saved_state();
        recipe.state.resolve_defaults(&self.config);
        recipe.name = Some(name.to_string());
        recipe.updated_at = chrono::Utc::now();
        self.store.save(&recipe).map_err(|e| format!("{e}"))?;
        Ok(())
    }

    /// List all saved recipes.
    pub fn list_recipes(&self) -> Vec<BuildRecipe> {
        self.store.list().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> BuilderConfig {
        BuilderConfig {
            versions: llamafile_builder_core::config_manager::parse_config_data(
                include_str!("../../../data/versions.json"),
            ).unwrap(),
            models: llamafile_builder_core::config_manager::parse_config_data(
                include_str!("../../../data/models.json"),
            ).unwrap(),
            config_schema: llamafile_builder_core::config_manager::parse_config_data(
                include_str!("../../../data/config.json"),
            ).unwrap(),
            config_version: 1,
            min_builder_version: None,
        }
    }

    fn test_app() -> App {
        let tmp = tempfile::TempDir::new().unwrap();
        App::new(test_config(), PathBuf::from("/tmp/test-cache"), ConfigStore::new(tmp.into_path()))
    }

    // ── Defaults ──

    #[test]
    fn new_app_selects_default_version_and_model() {
        let app = test_app();
        assert!(app.version_group.is_some(), "should have a default version group");
        assert!(app.version.is_some(), "should have a default version");
        assert!(app.model_id.is_some(), "should have a default model");
        assert_eq!(app.model_source, ModelTab::Registry);
    }

    #[test]
    fn new_app_can_build() {
        let app = test_app();
        assert!(app.can_build(), "default state should be buildable");
    }

    #[test]
    fn effective_output_has_llamafile_extension() {
        let app = test_app();
        assert!(app.effective_output().ends_with(".llamafile"));
    }

    // ── Focus navigation ──

    #[test]
    fn focus_next_wraps_around() {
        let focus = Focus::Quit.next(false, false, false);
        assert_eq!(focus, Focus::Version);
    }

    #[test]
    fn focus_prev_wraps_around() {
        let focus = Focus::Version.prev(false, false, false);
        assert_eq!(focus, Focus::Quit);
    }

    #[test]
    fn focus_skips_mmproj_when_no_mmproj() {
        let focus = Focus::Weights.next(false, false, false);
        assert_eq!(focus, Focus::Output, "should skip Mmproj");
    }

    #[test]
    fn focus_includes_mmproj_when_present() {
        let focus = Focus::Weights.next(true, false, false);
        assert_eq!(focus, Focus::Mmproj);
    }

    #[test]
    fn focus_skips_config_when_args_overridden() {
        let focus = Focus::Output.next(false, true, false);
        assert_eq!(focus, Focus::Gpu, "should skip Config");
    }

    #[test]
    fn focus_skips_weights_and_mmproj_for_local_model() {
        let focus = Focus::Model.next(true, false, true);
        assert_eq!(focus, Focus::Output, "should skip Weights and Mmproj");
    }

    #[test]
    fn focus_prev_skips_weights_for_local_model() {
        let focus = Focus::Output.prev(true, false, true);
        assert_eq!(focus, Focus::Model, "should skip Mmproj and Weights");
    }

    // ── Model source switching ──

    #[test]
    fn selecting_ollama_clears_registry_model() {
        let mut app = test_app();
        assert!(app.model_id.is_some());

        app.model_source = ModelTab::Ollama;
        app.ollama_model = Some(OllamaModel {
            model_name: "llama3".to_string(),
            tag: "8b".to_string(),
            display_name: "llama3:8b".to_string(),
            weights_path: "/path/to/blob".to_string(),
            size_bytes: 4_000_000_000,
        });
        app.model_id = None;

        assert!(app.is_local_model());
        assert!(app.selected_model().is_none(), "registry model should be None");
        assert!(!app.has_mmproj(), "Ollama model has no mmproj");
    }

    #[test]
    fn ollama_model_sets_output_filename() {
        let mut app = test_app();
        app.model_source = ModelTab::Ollama;
        app.model_id = None;
        app.ollama_model = Some(OllamaModel {
            model_name: "llama3".to_string(),
            tag: "8b".to_string(),
            display_name: "llama3:8b".to_string(),
            weights_path: "/path".to_string(),
            size_bytes: 1,
        });
        app.output_path = None;
        assert!(app.effective_output().ends_with("llama3_8b.llamafile"));
    }

    #[test]
    fn lmstudio_model_sets_output_filename() {
        let mut app = test_app();
        app.model_source = ModelTab::LmStudio;
        app.model_id = None;
        app.lmstudio_model = Some(LMStudioModel {
            dir_name: "org/model-name".to_string(),
            display_name: "Model Name".to_string(),
            dir_path: "/path".to_string(),
            files: vec![],
            total_size_bytes: 1,
        });
        app.output_path = None;
        assert!(app.effective_output().ends_with("org_model-name.llamafile"));
    }

    #[test]
    fn explicit_output_path_overrides_auto() {
        let mut app = test_app();
        app.output_path = Some("/custom/output.llamafile".to_string());
        assert_eq!(app.effective_output(), "/custom/output.llamafile");
    }

    // ── GPU libs ──

    #[test]
    fn available_gpu_libs_from_selected_release() {
        let app = test_app();
        let libs = app.available_gpu_libs();
        // The default release (0.10.1_alpha) ships cuda, rocm, and vulkan for
        // Linux + Windows. Anything less means serde dropped a section (see the
        // original vulkan regression) or the iteration skipped an accelerator.
        assert_eq!(libs.len(), 6, "expected 3 accels × 2 OSes, got {:?}", libs);

        let accels: std::collections::HashSet<_> = libs.iter().map(|l| l.accel).collect();
        assert!(accels.contains(&GpuAccel::Cuda));
        assert!(accels.contains(&GpuAccel::Rocm));
        assert!(accels.contains(&GpuAccel::Vulkan));

        for lib in &libs {
            assert!(lib.size > 0, "zero-size lib: {:?}", lib);
        }
    }

    // ── Model tab navigation ──

    #[test]
    fn model_tab_cycles() {
        assert_eq!(ModelTab::Registry.next(), ModelTab::Ollama);
        assert_eq!(ModelTab::Ollama.next(), ModelTab::LmStudio);
        assert_eq!(ModelTab::LmStudio.next(), ModelTab::Registry);
        assert_eq!(ModelTab::Registry.prev(), ModelTab::LmStudio);
    }

    // ── Log filtering ──

    #[test]
    fn log_level_filtering() {
        let mut app = test_app();
        app.add_log(LogLevel::Debug, "debug msg".to_string());
        app.add_log(LogLevel::Info, "info msg".to_string());
        app.add_log(LogLevel::Error, "error msg".to_string());

        app.log_level = LogLevel::Info;
        assert_eq!(app.filtered_logs().len(), 2);

        app.log_level = LogLevel::Error;
        assert_eq!(app.filtered_logs().len(), 1);

        app.log_level = LogLevel::Debug;
        assert_eq!(app.filtered_logs().len(), 3);
    }

    #[test]
    fn log_capped_at_1000() {
        let mut app = test_app();
        for i in 0..1100 {
            app.add_log(LogLevel::Info, format!("msg {}", i));
        }
        assert_eq!(app.logs.len(), 1000);
        assert!(app.logs.last().unwrap().message.contains("1099"));
    }
}

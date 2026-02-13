use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Source of model selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    Registry,
    Custom,
    Ollama,
    LMStudio,
}

/// Source of a specific file (within a registry model)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileSource {
    Registry,
    Custom,
}

/// Source of llamafile selection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlamafileSource {
    Registry,
    Custom,
}

/// Selection state for a model file (weights, mmproj, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSelection {
    pub source: FileSource,
    pub quantization_idx: usize,
    pub custom_path: Option<String>,
}

/// A custom model file (for fully custom mode)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomFile {
    pub role: String,
    pub path: String,
}

/// Complete build configuration state managed by Rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildState {
    // Model selection
    pub model_source: ModelSource,
    pub selected_model_id: Option<String>,
    /// File selections keyed by role (e.g., "weights", "mmproj")
    pub model_file_selections: HashMap<String, FileSelection>,
    /// Custom files for fully custom mode
    pub custom_model_files: Vec<CustomFile>,
    /// Selected Ollama model (when model_source is Ollama)
    pub selected_ollama_model: Option<crate::models::OllamaModel>,
    /// Selected LM Studio model (when model_source is LMStudio)
    pub selected_lmstudio_model: Option<crate::models::LMStudioModel>,
    /// Selected file indices for LM Studio model (keyed by role: "weights", "mmproj")
    pub lmstudio_file_selections: HashMap<String, usize>,

    // Llamafile selection
    pub llamafile_source: LlamafileSource,
    pub selected_version_group: Option<String>,
    pub selected_release_version: Option<String>,
    pub custom_llamafile_path: Option<String>,

    // Configuration values (from config.json schema)
    pub config_values: HashMap<String, serde_json::Value>,

    // Output
    pub output_path: String,

    // Derived/resolved paths (computed from selections)
    pub resolved_llamafile_path: Option<String>,

    // Resolved model file paths (computed from current selection)
    pub resolved_weights_path: Option<String>,
    pub resolved_weights_filename: Option<String>,
    pub resolved_mmproj_path: Option<String>,
    pub resolved_mmproj_filename: Option<String>,
}

impl Default for BuildState {
    fn default() -> Self {
        Self {
            model_source: ModelSource::Registry,
            selected_model_id: None,
            model_file_selections: HashMap::new(),
            custom_model_files: Vec::new(),
            selected_ollama_model: None,
            selected_lmstudio_model: None,
            lmstudio_file_selections: HashMap::new(),
            llamafile_source: LlamafileSource::Registry,
            selected_version_group: None,
            selected_release_version: None,
            custom_llamafile_path: None,
            config_values: HashMap::new(),
            output_path: String::new(),
            resolved_llamafile_path: None,
            resolved_weights_path: None,
            resolved_weights_filename: None,
            resolved_mmproj_path: None,
            resolved_mmproj_filename: None,
        }
    }
}

impl BuildState {
    /// Get the GGUF filename for the current selection
    pub fn get_gguf_filename(&self, models: &[crate::models::Model]) -> Option<String> {
        match self.model_source {
            ModelSource::Custom => {
                // In custom mode, get filename from custom files
                self.custom_model_files
                    .iter()
                    .find(|f| f.role == "weights")
                    .map(|f| get_filename_from_path(&f.path))
            }
            ModelSource::Ollama => {
                // In Ollama mode, return None (we use model_name_tag for output)
                None
            }
            ModelSource::LMStudio => {
                // In LM Studio mode, return the selected weights filename
                if let Some(ref model) = self.selected_lmstudio_model {
                    let weights_idx = self.lmstudio_file_selections.get("weights").copied().unwrap_or(0);
                    model.files.iter()
                        .filter(|f| f.role == "weights")
                        .nth(weights_idx)
                        .map(|f| f.filename.clone())
                } else {
                    None
                }
            }
            ModelSource::Registry => {
                // In registry mode, check for custom path first, then registry
                if let Some(sel) = self.model_file_selections.get("weights") {
                    if sel.source == FileSource::Custom {
                        if let Some(ref path) = sel.custom_path {
                            return Some(get_filename_from_path(path));
                        }
                    }
                }
                // Get from registry model
                let model_id = self.selected_model_id.as_ref()?;
                let model = models.iter().find(|m| &m.id == model_id)?;
                let weights_file = model.files.iter().find(|f| f.role == "weights")?;
                let sel = self.model_file_selections.get("weights")?;
                let quant = weights_file.quantizations.get(sel.quantization_idx)?;
                Some(get_filename_from_url(&quant.download_url))
            }
        }
    }

    /// Get the model name for the current selection
    pub fn get_model_name(&self, models: &[crate::models::Model]) -> Option<String> {
        match self.model_source {
            ModelSource::Custom => None,
            ModelSource::Ollama => {
                // For Ollama, return the display name (model_name:tag)
                self.selected_ollama_model.as_ref().map(|m| m.display_name.clone())
            }
            ModelSource::LMStudio => {
                // For LM Studio, return the display name
                self.selected_lmstudio_model.as_ref().map(|m| m.display_name.clone())
            }
            ModelSource::Registry => {
                let model_id = self.selected_model_id.as_ref()?;
                let model = models.iter().find(|m| &m.id == model_id)?;
                Some(model.name.clone())
            }
        }
    }

    /// Update the output path based on current selection
    pub fn update_output_path(&mut self, models: &[crate::models::Model]) {
        // Determine base name based on model source
        let base_name = match self.model_source {
            ModelSource::Ollama => {
                // For Ollama: use model_name_tag format
                if let Some(ref ollama) = self.selected_ollama_model {
                    format!("{}_{}", ollama.model_name, ollama.tag)
                } else {
                    "output".to_string()
                }
            }
            _ => {
                // For Registry/Custom: use GGUF filename or model name
                let gguf_filename = self.get_gguf_filename(models);
                let model_name = self.get_model_name(models);

                if let Some(gguf) = gguf_filename {
                    gguf.trim_end_matches(".gguf")
                        .trim_end_matches(".GGUF")
                        .to_string()
                } else if let Some(name) = model_name {
                    name.to_lowercase().replace(" ", "-").replace(".", "-")
                } else {
                    "output".to_string()
                }
            }
        };

        // Get output directory
        let output_dir = get_default_output_dir();

        self.output_path = format!("{}/{}.llamafile", output_dir, base_name);
    }

    /// Initialize file selections for a model
    pub fn init_file_selections(&mut self, model: &crate::models::Model) {
        self.model_file_selections.clear();
        for file in &model.files {
            self.model_file_selections.insert(
                file.role.clone(),
                FileSelection {
                    source: FileSource::Registry,
                    quantization_idx: 0,
                    custom_path: None,
                },
            );
        }
        // Context length defaults come from config.json schema, not overridden here
    }

    /// Update resolved model file paths based on current selection
    /// This should be called after any model selection change
    pub fn update_resolved_model_paths(&mut self, models: &[crate::models::Model]) {
        // Clear existing
        self.resolved_weights_path = None;
        self.resolved_weights_filename = None;
        self.resolved_mmproj_path = None;
        self.resolved_mmproj_filename = None;

        match self.model_source {
            ModelSource::Custom => {
                // Custom mode: get from custom_model_files
                for file in &self.custom_model_files {
                    let filename = get_filename_from_path(&file.path);
                    if file.role == "weights" {
                        self.resolved_weights_path = Some(file.path.clone());
                        self.resolved_weights_filename = Some(filename);
                    } else if file.role == "mmproj" {
                        self.resolved_mmproj_path = Some(file.path.clone());
                        self.resolved_mmproj_filename = Some(filename);
                    }
                }
            }
            ModelSource::Ollama => {
                // Ollama: only has weights (the blob path)
                if let Some(ref ollama) = self.selected_ollama_model {
                    self.resolved_weights_path = Some(ollama.weights_path.clone());
                    self.resolved_weights_filename = Some(get_filename_from_path(&ollama.weights_path));
                }
            }
            ModelSource::LMStudio => {
                // LM Studio: get from selected files
                if let Some(ref model) = self.selected_lmstudio_model {
                    // Get weights file
                    let weights_idx = self.lmstudio_file_selections.get("weights").copied().unwrap_or(0);
                    if let Some(file) = model.files.iter().filter(|f| f.role == "weights").nth(weights_idx) {
                        self.resolved_weights_path = Some(file.path.clone());
                        self.resolved_weights_filename = Some(file.filename.clone());
                    }

                    // Get mmproj file if present
                    let mmproj_idx = self.lmstudio_file_selections.get("mmproj").copied().unwrap_or(0);
                    if let Some(file) = model.files.iter().filter(|f| f.role == "mmproj").nth(mmproj_idx) {
                        self.resolved_mmproj_path = Some(file.path.clone());
                        self.resolved_mmproj_filename = Some(file.filename.clone());
                    }
                }
            }
            ModelSource::Registry => {
                // Registry: check for custom paths first, then construct from registry data
                let model_id = match self.selected_model_id.as_ref() {
                    Some(id) => id,
                    None => return,
                };
                let model = match models.iter().find(|m| &m.id == model_id) {
                    Some(m) => m,
                    None => return,
                };

                for file in &model.files {
                    let sel = match self.model_file_selections.get(&file.role) {
                        Some(s) => s,
                        None => continue,
                    };

                    let (path, filename) = if sel.source == FileSource::Custom {
                        if let Some(ref custom_path) = sel.custom_path {
                            (custom_path.clone(), get_filename_from_path(custom_path))
                        } else {
                            continue;
                        }
                    } else {
                        // Registry file - path will be computed during build
                        // Just set the filename from the download URL
                        if let Some(quant) = file.quantizations.get(sel.quantization_idx) {
                            let filename = get_filename_from_url(&quant.download_url);
                            // Path is not known until download - leave as None
                            (String::new(), filename)
                        } else {
                            continue;
                        }
                    };

                    if file.role == "weights" {
                        self.resolved_weights_path = if path.is_empty() { None } else { Some(path) };
                        self.resolved_weights_filename = Some(filename);
                    } else if file.role == "mmproj" {
                        self.resolved_mmproj_path = if path.is_empty() { None } else { Some(path) };
                        self.resolved_mmproj_filename = Some(filename);
                    }
                }
            }
        }
    }
}

/// Thread-safe wrapper for BuildState
pub struct BuildStateManager {
    state: Mutex<BuildState>,
    models: Mutex<Vec<crate::models::Model>>,
    versions: Mutex<Vec<crate::models::LlamafileVersionGroup>>,
}

impl BuildStateManager {
    pub fn new() -> Self {
        // Load models and versions
        let models: Vec<crate::models::Model> =
            serde_json::from_str(include_str!("../models.json")).unwrap_or_default();
        let versions: Vec<crate::models::LlamafileVersionGroup> =
            serde_json::from_str(include_str!("../versions.json")).unwrap_or_default();

        Self {
            state: Mutex::new(BuildState::default()),
            models: Mutex::new(models),
            versions: Mutex::new(versions),
        }
    }

    pub fn get_state(&self) -> BuildState {
        self.state.lock().unwrap().clone()
    }

    pub fn get_models(&self) -> Vec<crate::models::Model> {
        self.models.lock().unwrap().clone()
    }

    pub fn get_versions(&self) -> Vec<crate::models::LlamafileVersionGroup> {
        self.versions.lock().unwrap().clone()
    }

    pub fn select_registry_model(&self, model_id: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if let Some(model) = models.iter().find(|m| m.id == model_id) {
            state.model_source = ModelSource::Registry;
            state.selected_model_id = Some(model_id);
            state.custom_model_files.clear();
            state.selected_ollama_model = None;
            state.selected_lmstudio_model = None;
            state.lmstudio_file_selections.clear();
            state.init_file_selections(model);
            state.update_output_path(&models);
            state.update_resolved_model_paths(&models);
        }

        state.clone()
    }

    pub fn select_custom_model(&self, path: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        state.model_source = ModelSource::Custom;
        state.selected_model_id = None;
        state.model_file_selections.clear();
        state.selected_ollama_model = None;
        state.selected_lmstudio_model = None;
        state.lmstudio_file_selections.clear();
        state.custom_model_files = vec![CustomFile {
            role: "weights".to_string(),
            path,
        }];
        state.update_output_path(&models);
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn select_ollama_model(&self, ollama_model: crate::models::OllamaModel) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        state.model_source = ModelSource::Ollama;
        state.selected_model_id = None;
        state.model_file_selections.clear();
        state.custom_model_files.clear();
        state.selected_ollama_model = Some(ollama_model);
        state.selected_lmstudio_model = None;
        state.lmstudio_file_selections.clear();
        state.update_output_path(&models);
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn select_lmstudio_model(&self, lmstudio_model: crate::models::LMStudioModel) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        // Initialize file selections - select first file of each role
        // Note: indices are within the filtered array for each role, not the full files array
        state.lmstudio_file_selections.clear();
        let has_weights = lmstudio_model.files.iter().any(|f| f.role == "weights");
        let has_mmproj = lmstudio_model.files.iter().any(|f| f.role == "mmproj");
        if has_weights {
            state.lmstudio_file_selections.insert("weights".to_string(), 0);
        }
        if has_mmproj {
            state.lmstudio_file_selections.insert("mmproj".to_string(), 0);
        }

        state.model_source = ModelSource::LMStudio;
        state.selected_model_id = None;
        state.model_file_selections.clear();
        state.custom_model_files.clear();
        state.selected_ollama_model = None;
        state.selected_lmstudio_model = Some(lmstudio_model);
        state.update_output_path(&models);
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn select_lmstudio_file(&self, role: String, idx: usize) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        state.lmstudio_file_selections.insert(role.clone(), idx);

        // Update output path if weights changed
        if role == "weights" {
            state.update_output_path(&models);
        }
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn add_custom_companion_file(&self, role: String, path: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        // Remove existing file with same role
        state.custom_model_files.retain(|f| f.role != role);
        state.custom_model_files.push(CustomFile { role, path });
        state.update_output_path(&models);
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn remove_custom_companion_file(&self, role: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        state.custom_model_files.retain(|f| f.role != role);
        state.update_output_path(&models);
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn select_file_quantization(&self, role: String, idx: usize) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if let Some(sel) = state.model_file_selections.get_mut(&role) {
            sel.quantization_idx = idx;
            sel.source = FileSource::Registry;
            sel.custom_path = None;
        }

        if role == "weights" {
            state.update_output_path(&models);
        }
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn use_custom_file_for_role(&self, role: String, path: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if let Some(sel) = state.model_file_selections.get_mut(&role) {
            sel.source = FileSource::Custom;
            sel.custom_path = Some(path);
        }

        if role == "weights" {
            state.update_output_path(&models);
        }
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn clear_custom_file_for_role(&self, role: String) -> BuildState {
        let models = self.models.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        if let Some(sel) = state.model_file_selections.get_mut(&role) {
            sel.source = FileSource::Registry;
            sel.custom_path = None;
        }

        if role == "weights" {
            state.update_output_path(&models);
        }
        state.update_resolved_model_paths(&models);

        state.clone()
    }

    pub fn select_llamafile_version(&self, group_name: String, version: String) -> BuildState {
        let versions = self.versions.lock().unwrap();
        let mut state = self.state.lock().unwrap();

        state.llamafile_source = LlamafileSource::Registry;
        state.selected_version_group = Some(group_name.clone());
        state.selected_release_version = Some(version.clone());
        state.custom_llamafile_path = None;

        // Resolve the path from cache
        if let Some(group) = versions.iter().find(|g| g.name == group_name) {
            if let Some(release) = group.releases.iter().find(|r| r.version == version) {
                let cache_dir = get_llamafile_cache_dir();
                let filename = get_filename_from_url(&release.download_url);
                state.resolved_llamafile_path = Some(format!("{}/{}", cache_dir, filename));
            }
        }

        state.clone()
    }

    pub fn use_custom_llamafile(&self, path: String) -> BuildState {
        let mut state = self.state.lock().unwrap();

        state.llamafile_source = LlamafileSource::Custom;
        state.selected_version_group = None;
        state.selected_release_version = None;
        state.custom_llamafile_path = Some(path.clone());
        state.resolved_llamafile_path = Some(path);

        state.clone()
    }

    pub fn set_config_value(&self, key: String, value: serde_json::Value) -> BuildState {
        let mut state = self.state.lock().unwrap();
        state.config_values.insert(key, value);
        state.clone()
    }

    pub fn set_output_path(&self, path: String) -> BuildState {
        let mut state = self.state.lock().unwrap();
        state.output_path = path;
        state.clone()
    }
}

impl Default for BuildStateManager {
    fn default() -> Self {
        Self::new()
    }
}

// Helper functions

fn get_filename_from_path(path: &str) -> String {
    path.split('/')
        .last()
        .unwrap_or(path)
        .split('\\')
        .last()
        .unwrap_or(path)
        .to_string()
}

fn get_filename_from_url(url: &str) -> String {
    url.split('/')
        .last()
        .unwrap_or("file")
        .split('?')
        .next()
        .unwrap_or("file")
        .to_string()
}

fn get_default_output_dir() -> String {
    directories::UserDirs::new()
        .and_then(|dirs| {
            dirs.desktop_dir()
                .map(|p| p.to_string_lossy().to_string())
                .or_else(|| dirs.download_dir().map(|p| p.to_string_lossy().to_string()))
                .or_else(|| dirs.document_dir().map(|p| p.to_string_lossy().to_string()))
        })
        .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| ".".to_string())
}

fn get_llamafile_cache_dir() -> String {
    let base = directories::BaseDirs::new();
    if let Some(base) = base {
        if cfg!(target_os = "windows") {
            base.data_local_dir()
                .join("llamafile")
                .join("bin")
                .to_string_lossy()
                .to_string()
        } else {
            base.home_dir()
                .join(".llamafile")
                .join("bin")
                .to_string_lossy()
                .to_string()
        }
    } else {
        ".".to_string()
    }
}

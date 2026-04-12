use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::models::{
    get_default_output_dir, get_filename_from_path, get_filename_from_url, get_llamafile_cache_dir,
    LMStudioModel, LlamafileVersionGroup, Model, OllamaModel,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    Registry,
    Custom,
    Ollama,
    LMStudio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FileSource {
    Registry,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LlamafileSource {
    Registry,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSelection {
    pub source: FileSource,
    pub quantization_idx: usize,
    pub custom_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomFile {
    pub role: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildState {
    pub model_source: ModelSource,
    pub selected_model_id: Option<String>,
    pub model_file_selections: HashMap<String, FileSelection>,
    pub custom_model_files: Vec<CustomFile>,
    pub selected_ollama_model: Option<OllamaModel>,
    pub selected_lmstudio_model: Option<LMStudioModel>,
    pub lmstudio_file_selections: HashMap<String, usize>,

    pub llamafile_source: LlamafileSource,
    pub selected_version_group: Option<String>,
    pub selected_release_version: Option<String>,
    pub custom_llamafile_path: Option<String>,

    pub config_values: HashMap<String, serde_json::Value>,

    pub output_path: String,

    pub resolved_llamafile_path: Option<String>,
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
    pub fn get_gguf_filename(&self, models: &[Model]) -> Option<String> {
        match self.model_source {
            ModelSource::Custom => self
                .custom_model_files
                .iter()
                .find(|f| f.role == "weights")
                .map(|f| get_filename_from_path(&f.path)),
            ModelSource::Ollama => None,
            ModelSource::LMStudio => {
                if let Some(ref model) = self.selected_lmstudio_model {
                    let weights_idx = self
                        .lmstudio_file_selections
                        .get("weights")
                        .copied()
                        .unwrap_or(0);
                    model
                        .files
                        .iter()
                        .filter(|f| f.role == "weights")
                        .nth(weights_idx)
                        .map(|f| f.filename.clone())
                } else {
                    None
                }
            }
            ModelSource::Registry => {
                if let Some(sel) = self.model_file_selections.get("weights") {
                    if sel.source == FileSource::Custom {
                        if let Some(ref path) = sel.custom_path {
                            return Some(get_filename_from_path(path));
                        }
                    }
                }
                let model_id = self.selected_model_id.as_ref()?;
                let model = models.iter().find(|m| &m.id == model_id)?;
                let weights_file = model.files.iter().find(|f| f.role == "weights")?;
                let sel = self.model_file_selections.get("weights")?;
                let quant = weights_file.quantizations.get(sel.quantization_idx)?;
                Some(quant.filename.clone())
            }
        }
    }

    pub fn get_model_name(&self, models: &[Model]) -> Option<String> {
        match self.model_source {
            ModelSource::Custom => None,
            ModelSource::Ollama => self
                .selected_ollama_model
                .as_ref()
                .map(|m| m.display_name.clone()),
            ModelSource::LMStudio => self
                .selected_lmstudio_model
                .as_ref()
                .map(|m| m.display_name.clone()),
            ModelSource::Registry => {
                let model_id = self.selected_model_id.as_ref()?;
                let model = models.iter().find(|m| &m.id == model_id)?;
                Some(model.name.clone())
            }
        }
    }

    pub fn update_output_path(&mut self, models: &[Model]) {
        let base_name = match self.model_source {
            ModelSource::Ollama => {
                if let Some(ref ollama) = self.selected_ollama_model {
                    format!("{}_{}", ollama.model_name, ollama.tag)
                } else {
                    "output".to_string()
                }
            }
            _ => {
                let gguf_filename = self.get_gguf_filename(models);
                let model_name = self.get_model_name(models);
                if let Some(gguf) = gguf_filename {
                    gguf.trim_end_matches(".gguf")
                        .trim_end_matches(".GGUF")
                        .to_string()
                } else if let Some(name) = model_name {
                    name.to_lowercase().replace(' ', "-").replace('.', "-")
                } else {
                    "output".to_string()
                }
            }
        };
        let output_dir = get_default_output_dir();
        let mut path = std::path::PathBuf::from(output_dir);
        path.push(format!("{}.llamafile", base_name));
        self.output_path = path.to_string_lossy().to_string();
    }

    pub fn init_file_selections(&mut self, model: &Model) {
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
    }

    pub fn update_resolved_model_paths(&mut self, models: &[Model]) {
        self.resolved_weights_path = None;
        self.resolved_weights_filename = None;
        self.resolved_mmproj_path = None;
        self.resolved_mmproj_filename = None;

        match self.model_source {
            ModelSource::Custom => {
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
                if let Some(ref ollama) = self.selected_ollama_model {
                    self.resolved_weights_path = Some(ollama.weights_path.clone());
                    self.resolved_weights_filename =
                        Some(get_filename_from_path(&ollama.weights_path));
                }
            }
            ModelSource::LMStudio => {
                if let Some(ref model) = self.selected_lmstudio_model {
                    let weights_idx = self
                        .lmstudio_file_selections
                        .get("weights")
                        .copied()
                        .unwrap_or(0);
                    if let Some(file) = model
                        .files
                        .iter()
                        .filter(|f| f.role == "weights")
                        .nth(weights_idx)
                    {
                        self.resolved_weights_path = Some(file.path.clone());
                        self.resolved_weights_filename = Some(file.filename.clone());
                    }
                    let mmproj_idx = self
                        .lmstudio_file_selections
                        .get("mmproj")
                        .copied()
                        .unwrap_or(0);
                    if let Some(file) = model
                        .files
                        .iter()
                        .filter(|f| f.role == "mmproj")
                        .nth(mmproj_idx)
                    {
                        self.resolved_mmproj_path = Some(file.path.clone());
                        self.resolved_mmproj_filename = Some(file.filename.clone());
                    }
                }
            }
            ModelSource::Registry => {
                let model_id = match self.selected_model_id.as_ref() {
                    Some(id) => id.clone(),
                    None => return,
                };
                let model = match models.iter().find(|m| m.id == model_id) {
                    Some(m) => m.clone(),
                    None => return,
                };
                for file in &model.files {
                    let sel = match self.model_file_selections.get(&file.role) {
                        Some(s) => s.clone(),
                        None => continue,
                    };
                    let (path, filename) = if sel.source == FileSource::Custom {
                        if let Some(ref custom_path) = sel.custom_path {
                            (custom_path.clone(), get_filename_from_path(custom_path))
                        } else {
                            continue;
                        }
                    } else if let Some(quant) = file.quantizations.get(sel.quantization_idx) {
                        let filename = quant.filename.clone();
                        (String::new(), filename)
                    } else {
                        continue;
                    };
                    if file.role == "weights" {
                        self.resolved_weights_path =
                            if path.is_empty() { None } else { Some(path) };
                        self.resolved_weights_filename = Some(filename);
                    } else if file.role == "mmproj" {
                        self.resolved_mmproj_path =
                            if path.is_empty() { None } else { Some(path) };
                        self.resolved_mmproj_filename = Some(filename);
                    }
                }
            }
        }
    }

    // State mutation methods

    pub fn select_registry_model(&mut self, model_id: String, models: &[Model]) {
        if let Some(model) = models.iter().find(|m| m.id == model_id) {
            self.model_source = ModelSource::Registry;
            self.selected_model_id = Some(model_id);
            self.custom_model_files.clear();
            self.selected_ollama_model = None;
            self.selected_lmstudio_model = None;
            self.lmstudio_file_selections.clear();
            self.init_file_selections(model);
            self.update_output_path(models);
            self.update_resolved_model_paths(models);
        }
    }

    pub fn select_custom_model(&mut self, path: String, models: &[Model]) {
        self.model_source = ModelSource::Custom;
        self.selected_model_id = None;
        self.model_file_selections.clear();
        self.selected_ollama_model = None;
        self.selected_lmstudio_model = None;
        self.lmstudio_file_selections.clear();
        self.custom_model_files = vec![CustomFile {
            role: "weights".to_string(),
            path,
        }];
        self.update_output_path(models);
        self.update_resolved_model_paths(models);
    }

    pub fn select_ollama_model(&mut self, ollama_model: OllamaModel, models: &[Model]) {
        self.model_source = ModelSource::Ollama;
        self.selected_model_id = None;
        self.model_file_selections.clear();
        self.custom_model_files.clear();
        self.selected_ollama_model = Some(ollama_model);
        self.selected_lmstudio_model = None;
        self.lmstudio_file_selections.clear();
        self.update_output_path(models);
        self.update_resolved_model_paths(models);
    }

    pub fn select_lmstudio_model(&mut self, lmstudio_model: LMStudioModel, models: &[Model]) {
        self.lmstudio_file_selections.clear();
        let has_weights = lmstudio_model.files.iter().any(|f| f.role == "weights");
        let has_mmproj = lmstudio_model.files.iter().any(|f| f.role == "mmproj");
        if has_weights {
            self.lmstudio_file_selections
                .insert("weights".to_string(), 0);
        }
        if has_mmproj {
            self.lmstudio_file_selections
                .insert("mmproj".to_string(), 0);
        }
        self.model_source = ModelSource::LMStudio;
        self.selected_model_id = None;
        self.model_file_selections.clear();
        self.custom_model_files.clear();
        self.selected_ollama_model = None;
        self.selected_lmstudio_model = Some(lmstudio_model);
        self.update_output_path(models);
        self.update_resolved_model_paths(models);
    }

    pub fn select_lmstudio_file(&mut self, role: String, idx: usize, models: &[Model]) {
        self.lmstudio_file_selections.insert(role.clone(), idx);
        if role == "weights" {
            self.update_output_path(models);
        }
        self.update_resolved_model_paths(models);
    }

    pub fn add_custom_companion_file(&mut self, role: String, path: String, models: &[Model]) {
        self.custom_model_files.retain(|f| f.role != role);
        self.custom_model_files.push(CustomFile { role, path });
        self.update_output_path(models);
        self.update_resolved_model_paths(models);
    }

    pub fn remove_custom_companion_file(&mut self, role: &str, models: &[Model]) {
        self.custom_model_files.retain(|f| f.role != role);
        self.update_output_path(models);
        self.update_resolved_model_paths(models);
    }

    pub fn select_file_quantization(&mut self, role: String, idx: usize, models: &[Model]) {
        if let Some(sel) = self.model_file_selections.get_mut(&role) {
            sel.quantization_idx = idx;
            sel.source = FileSource::Registry;
            sel.custom_path = None;
        }
        if role == "weights" {
            self.update_output_path(models);
        }
        self.update_resolved_model_paths(models);
    }

    pub fn use_custom_file_for_role(&mut self, role: String, path: String, models: &[Model]) {
        if let Some(sel) = self.model_file_selections.get_mut(&role) {
            sel.source = FileSource::Custom;
            sel.custom_path = Some(path);
        }
        if role == "weights" {
            self.update_output_path(models);
        }
        self.update_resolved_model_paths(models);
    }

    pub fn select_llamafile_version(
        &mut self,
        group_name: String,
        version: String,
        versions: &[LlamafileVersionGroup],
    ) {
        self.llamafile_source = LlamafileSource::Registry;
        self.selected_version_group = Some(group_name.clone());
        self.selected_release_version = Some(version.clone());
        self.custom_llamafile_path = None;

        if let Some(group) = versions.iter().find(|g| g.name == group_name) {
            if let Some(release) = group.releases.iter().find(|r| r.version == version) {
                let cache_dir = get_llamafile_cache_dir();
                let filename = get_filename_from_url(&release.download_url);
                let path = std::path::PathBuf::from(&cache_dir).join(&filename);
                self.resolved_llamafile_path = Some(path.to_string_lossy().to_string());
            }
        }
    }

    pub fn use_custom_llamafile(&mut self, path: String) {
        self.llamafile_source = LlamafileSource::Custom;
        self.selected_version_group = None;
        self.selected_release_version = None;
        self.custom_llamafile_path = Some(path.clone());
        self.resolved_llamafile_path = Some(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        LMStudioFile, LlamafileRelease, Model, ModelFile, ModelFileQuantization,
    };
    use pretty_assertions::assert_eq;

    fn load_models() -> Vec<Model> {
        crate::config_manager::parse_config_data(include_str!("../../../data/models.json"))
            .expect("models.json must be valid")
    }

    fn load_versions() -> Vec<LlamafileVersionGroup> {
        crate::config_manager::parse_config_data(include_str!("../../../data/versions.json"))
            .expect("versions.json must be valid")
    }

    // ---------- select_registry_model ----------

    #[test]
    fn select_registry_model_sets_source_and_seeds_selections() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);

        assert_eq!(bs.model_source, ModelSource::Registry);
        assert_eq!(
            bs.selected_model_id.as_deref(),
            Some("mistralai/Ministral-3-3B-Instruct-2512-GGUF")
        );
        assert!(bs.model_file_selections.contains_key("weights"));
        assert!(bs.model_file_selections.contains_key("mmproj"));
        let weights_sel = &bs.model_file_selections["weights"];
        assert_eq!(weights_sel.source, FileSource::Registry);
        assert_eq!(weights_sel.quantization_idx, 0);
        assert!(weights_sel.custom_path.is_none());

        assert!(bs.output_path.ends_with(".llamafile"));
        assert!(bs.output_path.contains("Ministral-3-3B-Instruct-2512"));
        assert_eq!(
            bs.resolved_weights_filename.as_deref(),
            Some("Ministral-3-3B-Instruct-2512-Q4_K_M.gguf")
        );
    }

    #[test]
    fn select_registry_model_clears_other_sources() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_custom_model("/tmp/foo.gguf".to_string(), &models);
        assert_eq!(bs.model_source, ModelSource::Custom);

        bs.select_registry_model("Qwen/Qwen3-0.6B-GGUF".to_string(), &models);
        assert_eq!(bs.model_source, ModelSource::Registry);
        assert!(bs.custom_model_files.is_empty());
        assert!(bs.selected_ollama_model.is_none());
        assert!(bs.selected_lmstudio_model.is_none());
    }

    #[test]
    fn select_registry_model_nonexistent_id_is_noop() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("does-not-exist".to_string(), &models);
        assert_eq!(bs.model_source, ModelSource::Registry);
        assert!(bs.selected_model_id.is_none());
        assert!(bs.model_file_selections.is_empty());
    }

    // ---------- select_custom_model ----------

    #[test]
    fn select_custom_model_records_file_and_output_path() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_custom_model("/tmp/my-model.gguf".to_string(), &models);

        assert_eq!(bs.model_source, ModelSource::Custom);
        assert!(bs.selected_model_id.is_none());
        assert_eq!(bs.custom_model_files.len(), 1);
        assert_eq!(bs.custom_model_files[0].role, "weights");
        assert_eq!(bs.custom_model_files[0].path, "/tmp/my-model.gguf");

        assert_eq!(
            bs.resolved_weights_path.as_deref(),
            Some("/tmp/my-model.gguf")
        );
        assert_eq!(
            bs.resolved_weights_filename.as_deref(),
            Some("my-model.gguf")
        );

        assert!(bs.output_path.ends_with("my-model.llamafile"));
    }

    // ---------- select_ollama_model ----------

    #[test]
    fn select_ollama_model_sets_paths_and_output() {
        let models = load_models();
        let mut bs = BuildState::default();
        let ollama = OllamaModel {
            model_name: "llama3".to_string(),
            tag: "8b".to_string(),
            display_name: "llama3:8b".to_string(),
            weights_path: "/home/u/.ollama/models/blobs/sha256-abc".to_string(),
            size_bytes: 4_000_000_000,
        };
        bs.select_ollama_model(ollama.clone(), &models);

        assert_eq!(bs.model_source, ModelSource::Ollama);
        assert!(bs.selected_ollama_model.is_some());
        assert_eq!(
            bs.resolved_weights_path.as_deref(),
            Some("/home/u/.ollama/models/blobs/sha256-abc")
        );
        assert_eq!(
            bs.resolved_weights_filename.as_deref(),
            Some("sha256-abc")
        );
        assert!(bs.output_path.ends_with("llama3_8b.llamafile"));
    }

    // ---------- select_lmstudio_model ----------

    #[test]
    fn select_lmstudio_model_seeds_file_selections_and_resolves_paths() {
        let models = load_models();
        let mut bs = BuildState::default();
        let lmstudio = LMStudioModel {
            dir_name: "MyModel".to_string(),
            display_name: "MyModel".to_string(),
            dir_path: "/lm/MyModel".to_string(),
            total_size_bytes: 0,
            files: vec![
                LMStudioFile {
                    filename: "model.gguf".to_string(),
                    path: "/lm/MyModel/model.gguf".to_string(),
                    size_bytes: 100,
                    role: "weights".to_string(),
                },
                LMStudioFile {
                    filename: "model-mmproj.gguf".to_string(),
                    path: "/lm/MyModel/model-mmproj.gguf".to_string(),
                    size_bytes: 50,
                    role: "mmproj".to_string(),
                },
            ],
        };
        bs.select_lmstudio_model(lmstudio, &models);

        assert_eq!(bs.model_source, ModelSource::LMStudio);
        assert_eq!(
            bs.lmstudio_file_selections.get("weights").copied(),
            Some(0)
        );
        assert_eq!(
            bs.lmstudio_file_selections.get("mmproj").copied(),
            Some(0)
        );
        assert_eq!(
            bs.resolved_weights_path.as_deref(),
            Some("/lm/MyModel/model.gguf")
        );
        assert_eq!(
            bs.resolved_mmproj_path.as_deref(),
            Some("/lm/MyModel/model-mmproj.gguf")
        );
        assert!(bs.output_path.ends_with("model.llamafile"));
    }

    // ---------- select_file_quantization ----------

    #[test]
    fn select_file_quantization_updates_index_and_output() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);
        let q0_output = bs.output_path.clone();
        bs.select_file_quantization("weights".to_string(), 1, &models);

        assert_eq!(bs.model_file_selections["weights"].quantization_idx, 1);
        assert_eq!(
            bs.model_file_selections["weights"].source,
            FileSource::Registry
        );
        assert_ne!(bs.output_path, q0_output);
        assert!(bs.output_path.contains("BF16"));
        assert_eq!(
            bs.resolved_weights_filename.as_deref(),
            Some("Ministral-3-3B-Instruct-2512-BF16.gguf")
        );
    }

    #[test]
    fn select_file_quantization_for_non_weights_does_not_change_output_path() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);
        let before = bs.output_path.clone();
        bs.select_file_quantization("mmproj".to_string(), 0, &models);
        assert_eq!(bs.output_path, before);
    }

    // ---------- use_custom_file_for_role ----------

    #[test]
    fn use_custom_file_for_role_flips_source_and_path() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);
        bs.use_custom_file_for_role(
            "weights".to_string(),
            "/tmp/custom.gguf".to_string(),
            &models,
        );
        let sel = &bs.model_file_selections["weights"];
        assert_eq!(sel.source, FileSource::Custom);
        assert_eq!(sel.custom_path.as_deref(), Some("/tmp/custom.gguf"));
        assert!(bs.output_path.ends_with("custom.llamafile"));
        assert_eq!(
            bs.resolved_weights_filename.as_deref(),
            Some("custom.gguf")
        );
    }

    // ---------- init_file_selections ----------

    #[test]
    fn init_file_selections_seeds_default_for_every_role() {
        let model = Model {
            id: "m".to_string(),
            source: "hf".to_string(),
            name: "m".to_string(),
            organization: "o".to_string(),
            parameters: "1B".to_string(),
            param_count: 1,
            description: String::new(),
            license: String::new(),
            tags: vec![],
            context_length: 4096,
            default: false,
            files: vec![
                ModelFile {
                    role: "weights".to_string(),
                    label: None,
                    quantizations: vec![ModelFileQuantization {
                        name: "Q4".to_string(),
                        size_bytes: 1,
                        filename: "a.gguf".to_string(),
                        download_url: None,
                        sha256: None,
                    }],
                },
                ModelFile {
                    role: "mmproj".to_string(),
                    label: None,
                    quantizations: vec![ModelFileQuantization {
                        name: "BF16".to_string(),
                        size_bytes: 1,
                        filename: "b.gguf".to_string(),
                        download_url: None,
                        sha256: None,
                    }],
                },
            ],
        };
        let mut bs = BuildState::default();
        bs.init_file_selections(&model);
        assert_eq!(bs.model_file_selections.len(), 2);
        for (_, sel) in &bs.model_file_selections {
            assert_eq!(sel.source, FileSource::Registry);
            assert_eq!(sel.quantization_idx, 0);
            assert!(sel.custom_path.is_none());
        }
    }

    // ---------- update_output_path ----------

    #[test]
    fn update_output_path_uses_default_for_empty_state() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.update_output_path(&models);
        assert!(bs.output_path.ends_with("output.llamafile"));
    }

    // ---------- update_resolved_model_paths ----------

    #[test]
    fn update_resolved_model_paths_custom_source_records_weights_and_mmproj() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.model_source = ModelSource::Custom;
        bs.custom_model_files = vec![
            CustomFile {
                role: "weights".to_string(),
                path: "/x/w.gguf".to_string(),
            },
            CustomFile {
                role: "mmproj".to_string(),
                path: "/x/m.gguf".to_string(),
            },
        ];
        bs.update_resolved_model_paths(&models);
        assert_eq!(bs.resolved_weights_path.as_deref(), Some("/x/w.gguf"));
        assert_eq!(bs.resolved_weights_filename.as_deref(), Some("w.gguf"));
        assert_eq!(bs.resolved_mmproj_path.as_deref(), Some("/x/m.gguf"));
        assert_eq!(bs.resolved_mmproj_filename.as_deref(), Some("m.gguf"));
    }

    // ---------- get_gguf_filename / get_model_name ----------

    #[test]
    fn get_gguf_filename_registry_from_url() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);
        assert_eq!(
            bs.get_gguf_filename(&models).as_deref(),
            Some("Ministral-3-3B-Instruct-2512-Q4_K_M.gguf")
        );
    }

    #[test]
    fn get_gguf_filename_registry_with_custom_override() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string(), &models);
        bs.use_custom_file_for_role(
            "weights".to_string(),
            "/foo/bar/baz.gguf".to_string(),
            &models,
        );
        assert_eq!(bs.get_gguf_filename(&models).as_deref(), Some("baz.gguf"));
    }

    #[test]
    fn get_gguf_filename_custom() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_custom_model("/tmp/a.gguf".to_string(), &models);
        assert_eq!(bs.get_gguf_filename(&models).as_deref(), Some("a.gguf"));
    }

    #[test]
    fn get_gguf_filename_ollama_is_none() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_ollama_model(
            OllamaModel {
                model_name: "m".to_string(),
                tag: "t".to_string(),
                display_name: "m:t".to_string(),
                weights_path: "/blob".to_string(),
                size_bytes: 1,
            },
            &models,
        );
        assert_eq!(bs.get_gguf_filename(&models), None);
    }

    #[test]
    fn get_model_name_registry_custom_and_ollama() {
        let models = load_models();
        let mut bs = BuildState::default();

        bs.select_registry_model("Qwen/Qwen3-0.6B-GGUF".to_string(), &models);
        let name = bs.get_model_name(&models).unwrap();
        assert!(!name.is_empty());

        bs.select_custom_model("/tmp/c.gguf".to_string(), &models);
        assert_eq!(bs.get_model_name(&models), None);

        bs.select_ollama_model(
            OllamaModel {
                model_name: "x".to_string(),
                tag: "1".to_string(),
                display_name: "x:1".to_string(),
                weights_path: "/p".to_string(),
                size_bytes: 1,
            },
            &models,
        );
        assert_eq!(bs.get_model_name(&models).as_deref(), Some("x:1"));
    }

    // ---------- select_llamafile_version ----------

    #[test]
    fn select_llamafile_version_sets_resolved_path_from_registry() {
        let versions = load_versions();
        let mut bs = BuildState::default();
        let group = &versions[0];
        let release: &LlamafileRelease = &group.releases[0];
        bs.select_llamafile_version(group.name.clone(), release.version.clone(), &versions);
        assert_eq!(bs.llamafile_source, LlamafileSource::Registry);
        assert_eq!(
            bs.selected_version_group.as_deref(),
            Some(group.name.as_str())
        );
        assert_eq!(
            bs.selected_release_version.as_deref(),
            Some(release.version.as_str())
        );
        let path = bs.resolved_llamafile_path.as_deref().unwrap();
        assert!(path.contains("llamafile"));
    }

    #[test]
    fn use_custom_llamafile_flips_source_and_sets_resolved_path() {
        let mut bs = BuildState::default();
        bs.select_llamafile_version(
            "llamafile 0.10".to_string(),
            "0.10.0".to_string(),
            &load_versions(),
        );
        assert_eq!(bs.llamafile_source, LlamafileSource::Registry);

        bs.use_custom_llamafile("/custom/llamafile".to_string());
        assert_eq!(bs.llamafile_source, LlamafileSource::Custom);
        assert_eq!(
            bs.resolved_llamafile_path.as_deref(),
            Some("/custom/llamafile")
        );
        assert_eq!(
            bs.custom_llamafile_path.as_deref(),
            Some("/custom/llamafile")
        );
        assert!(bs.selected_version_group.is_none());
        assert!(bs.selected_release_version.is_none());
    }

    // ---------- add/remove custom companion files ----------

    #[test]
    fn add_and_remove_custom_companion_file() {
        let models = load_models();
        let mut bs = BuildState::default();
        bs.select_custom_model("/tmp/a.gguf".to_string(), &models);
        assert_eq!(bs.custom_model_files.len(), 1);

        bs.add_custom_companion_file(
            "mmproj".to_string(),
            "/tmp/mmproj.gguf".to_string(),
            &models,
        );
        assert_eq!(bs.custom_model_files.len(), 2);
        assert_eq!(
            bs.resolved_mmproj_path.as_deref(),
            Some("/tmp/mmproj.gguf")
        );

        bs.add_custom_companion_file(
            "mmproj".to_string(),
            "/tmp/mmproj2.gguf".to_string(),
            &models,
        );
        assert_eq!(bs.custom_model_files.len(), 2);
        assert_eq!(
            bs.resolved_mmproj_path.as_deref(),
            Some("/tmp/mmproj2.gguf")
        );

        bs.remove_custom_companion_file("mmproj", &models);
        assert_eq!(bs.custom_model_files.len(), 1);
        assert!(bs.resolved_mmproj_path.is_none());
    }
}

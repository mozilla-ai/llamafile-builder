use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config_manager::BuilderConfig;
use crate::error::Error;

/// User build selections. Sparse during interactive use (only explicit
/// overrides). Resolved to a full snapshot when saved as a `BuildRecipe`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_runtime: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_model_file: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub quantizations: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_values: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_file: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_libs: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_zipalign: Option<String>,
}

impl SavedState {
    /// Resolve all defaults from the config, turning this sparse state into
    /// a full snapshot. Call this once when saving a recipe — not during
    /// interactive use.
    pub fn resolve_defaults(&mut self, config: &BuilderConfig) {
        // Version
        if self.version_group.is_none() && self.version.is_none() && self.custom_runtime.is_none() {
            let group = config.versions.iter().find(|g| g.default)
                .or_else(|| config.versions.first());
            if let Some(group) = group {
                if let Some(release) = group.releases.first() {
                    self.version_group = Some(group.name.clone());
                    self.version = Some(release.version.clone());
                }
            }
        }

        // Model
        if self.model_id.is_none() && self.custom_model_file.is_none() {
            let model = config.models.iter().find(|m| m.default)
                .or_else(|| config.models.first());
            if let Some(model) = model {
                self.model_id = Some(model.id.clone());
            }
        }

        // Quantizations: fill in default (index 0) for each role of the selected model
        if let Some(ref model_id) = self.model_id {
            if let Some(model) = config.models.iter().find(|m| &m.id == model_id) {
                for file in &model.files {
                    if !self.quantizations.contains_key(&file.role) {
                        if let Some(q) = file.quantizations.first() {
                            self.quantizations.insert(file.role.clone(), q.name.clone());
                        }
                    }
                }
            }
        }

        // Config values: fill in all schema defaults
        let defaults = crate::args::default_config_values(&config.config_schema);
        for (key, value) in defaults {
            self.config_values.entry(key).or_insert(value);
        }

        // Output path
        if self.output_path.is_none() {
            if let Some(ref model_id) = self.model_id {
                if let Some(model) = config.models.iter().find(|m| &m.id == model_id) {
                    if let Some(file) = model.files.iter().find(|f| f.role == "weights") {
                        let qname = self.quantizations.get("weights")
                            .and_then(|n| file.quantizations.iter().find(|q| &q.name == n))
                            .or_else(|| file.quantizations.first());
                        if let Some(q) = qname {
                            let stem = q.filename.trim_end_matches(".gguf").trim_end_matches(".GGUF");
                            let dir = crate::models::get_default_output_dir();
                            self.output_path = Some(format!("{}/{}.llamafile", dir, stem));
                        }
                    }
                }
            }
        }
    }
}

/// A named build recipe with identity and metadata.
/// The `state` field is a full snapshot (all defaults resolved) when
/// persisted, so the recipe is self-contained and reproducible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildRecipe {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: SavedState,
}

impl BuildRecipe {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: None,
            created_at: now,
            updated_at: now,
            state: SavedState::default(),
        }
    }

    pub fn is_named(&self) -> bool {
        self.name.is_some()
    }
}

/// Filesystem-backed store for build recipes.
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Create a new empty recipe, persist it, and return it.
    pub fn create(&self) -> Result<BuildRecipe, Error> {
        let recipe = BuildRecipe::new();
        self.save(&recipe)?;
        Ok(recipe)
    }

    /// Get a recipe by UUID.
    pub fn get(&self, id: Uuid) -> Result<BuildRecipe, Error> {
        let path = self.path_for(id);
        let content = std::fs::read_to_string(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::NotFound(format!("config not found: {id}"))
            } else {
                Error::Io(e)
            }
        })?;
        serde_json::from_str(&content).map_err(Error::Json)
    }

    /// Persist a recipe (create or update).
    pub fn save(&self, recipe: &BuildRecipe) -> Result<(), Error> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path_for(recipe.id);
        let json = serde_json::to_string_pretty(recipe)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Delete a recipe by UUID.
    pub fn delete(&self, id: Uuid) -> Result<(), Error> {
        let path = self.path_for(id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// List all recipes, most recently updated first.
    pub fn list(&self) -> Result<Vec<BuildRecipe>, Error> {
        let mut recipes = Vec::new();
        if !self.dir.exists() {
            return Ok(recipes);
        }
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(recipe) = serde_json::from_str::<BuildRecipe>(&content) {
                        recipes.push(recipe);
                    }
                }
            }
        }
        recipes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(recipes)
    }

    /// Find by name (case-insensitive).
    pub fn find_by_name(&self, name: &str) -> Result<Option<BuildRecipe>, Error> {
        Ok(self.list()?.into_iter().find(|r| {
            r.name.as_ref().map(|n| n.eq_ignore_ascii_case(name)).unwrap_or(false)
        }))
    }

    /// Find by name or UUID string.
    pub fn find_by_name_or_id(&self, query: &str) -> Result<Option<BuildRecipe>, Error> {
        if let Ok(id) = Uuid::parse_str(query) {
            return match self.get(id) {
                Ok(r) => Ok(Some(r)),
                Err(Error::NotFound(_)) => Ok(None),
                Err(e) => Err(e),
            };
        }
        self.find_by_name(query)
    }

    /// Delete all unnamed recipes (working drafts).
    pub fn cleanup_unnamed(&self) -> Result<usize, Error> {
        let mut count = 0;
        for recipe in self.list()? {
            if !recipe.is_named() {
                self.delete(recipe.id)?;
                count += 1;
            }
        }
        Ok(count)
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.json", id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BuilderConfig {
        BuilderConfig {
            versions: crate::config_manager::parse_config_data(
                include_str!("../../../data/versions.json"),
            ).unwrap(),
            models: crate::config_manager::parse_config_data(
                include_str!("../../../data/models.json"),
            ).unwrap(),
            config_schema: crate::config_manager::parse_config_data(
                include_str!("../../../data/config.json"),
            ).unwrap(),
            config_version: 1,
            min_builder_version: None,
        }
    }

    // ── ConfigStore CRUD ──

    #[test]
    fn create_and_get() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let recipe = store.create().unwrap();
        assert!(!recipe.id.is_nil());
        assert!(recipe.name.is_none());
        assert_eq!(store.get(recipe.id).unwrap().id, recipe.id);
    }

    #[test]
    fn save_preserves_state() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let mut recipe = store.create().unwrap();
        recipe.state.model_id = Some("test/model".to_string());
        recipe.name = Some("my-recipe".to_string());
        store.save(&recipe).unwrap();

        let loaded = store.get(recipe.id).unwrap();
        assert_eq!(loaded.state.model_id.as_deref(), Some("test/model"));
        assert_eq!(loaded.name.as_deref(), Some("my-recipe"));
    }

    #[test]
    fn delete_removes_recipe() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let recipe = store.create().unwrap();
        store.delete(recipe.id).unwrap();
        assert!(matches!(store.get(recipe.id), Err(Error::NotFound(_))));
    }

    #[test]
    fn list_sorted_most_recent_first() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let recent = store.create().unwrap();
        let mut old = store.create().unwrap();
        old.updated_at = Utc::now() - chrono::Duration::hours(1);
        store.save(&old).unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, recent.id);
    }

    #[test]
    fn find_by_name_case_insensitive() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let mut recipe = store.create().unwrap();
        recipe.name = Some("My Setup".to_string());
        store.save(&recipe).unwrap();

        assert!(store.find_by_name("my setup").unwrap().is_some());
        assert!(store.find_by_name("nonexistent").unwrap().is_none());
    }

    #[test]
    fn find_by_name_or_uuid() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        let mut recipe = store.create().unwrap();
        recipe.name = Some("test".to_string());
        store.save(&recipe).unwrap();

        assert!(store.find_by_name_or_id("test").unwrap().is_some());
        assert!(store.find_by_name_or_id(&recipe.id.to_string()).unwrap().is_some());
        assert!(store.find_by_name_or_id("nope").unwrap().is_none());
    }

    #[test]
    fn cleanup_unnamed_keeps_named() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        store.create().unwrap();
        store.create().unwrap();
        let mut named = store.create().unwrap();
        named.name = Some("keeper".to_string());
        store.save(&named).unwrap();

        assert_eq!(store.cleanup_unnamed().unwrap(), 2);
        let remaining = store.list().unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].is_named());
    }

    #[test]
    fn get_nonexistent_returns_not_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        assert!(matches!(store.get(Uuid::new_v4()), Err(Error::NotFound(_))));
    }

    #[test]
    fn empty_store_lists_empty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let store = ConfigStore::new(tmp.path().to_path_buf());
        assert!(store.list().unwrap().is_empty());
    }

    // ── resolve_defaults ──

    #[test]
    fn resolve_fills_all_required_fields() {
        let config = test_config();
        let mut state = SavedState::default();
        state.resolve_defaults(&config);

        assert!(state.version_group.is_some());
        assert!(state.version.is_some());
        assert!(state.model_id.is_some());
        assert!(state.output_path.is_some());
        assert!(state.output_path.as_ref().unwrap().ends_with(".llamafile"));
    }

    #[test]
    fn resolve_fills_quantizations_for_all_model_roles() {
        let config = test_config();
        let mut state = SavedState::default();
        state.resolve_defaults(&config);

        // The selected model should have at least a "weights" role
        assert!(!state.quantizations.is_empty());
        assert!(state.quantizations.contains_key("weights"));

        // Every role in the model's files should have a quantization
        let model = config.models.iter().find(|m| Some(&m.id) == state.model_id.as_ref()).unwrap();
        for file in &model.files {
            if !file.quantizations.is_empty() {
                assert!(state.quantizations.contains_key(&file.role),
                    "missing quantization for role '{}'", file.role);
            }
        }
    }

    #[test]
    fn resolve_fills_config_values_matching_schema() {
        let config = test_config();
        let mut state = SavedState::default();
        state.resolve_defaults(&config);

        // Should have at least as many config values as the schema has keys with defaults
        let schema_defaults = crate::args::default_config_values(&config.config_schema);
        for key in schema_defaults.keys() {
            assert!(state.config_values.contains_key(key),
                "missing config default for key '{}'", key);
        }
    }

    #[test]
    fn resolve_preserves_explicit_overrides() {
        let config = test_config();
        let mut state = SavedState::default();
        state.version_group = Some("custom-group".to_string());
        state.version = Some("99.99".to_string());
        state.config_values.insert("context_length".to_string(), serde_json::json!(99999));

        state.resolve_defaults(&config);

        // Explicit values preserved
        assert_eq!(state.version_group.as_deref(), Some("custom-group"));
        assert_eq!(state.version.as_deref(), Some("99.99"));
        assert_eq!(state.config_values["context_length"], 99999);

        // Other defaults still filled in
        assert!(state.model_id.is_some());
    }

    #[test]
    fn resolved_state_round_trips_through_json() {
        let config = test_config();
        let mut state = SavedState::default();
        state.resolve_defaults(&config);

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: SavedState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.version_group, state.version_group);
        assert_eq!(loaded.model_id, state.model_id);
        assert_eq!(loaded.quantizations, state.quantizations);
        assert_eq!(loaded.config_values.len(), state.config_values.len());
        assert_eq!(loaded.output_path, state.output_path);
    }
}

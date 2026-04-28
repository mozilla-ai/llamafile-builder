use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config_manager::BuilderConfig;
use crate::error::Error;

fn is_false(b: &bool) -> bool {
    !*b
}

/// A GPU library specified by a local file path (for custom/development builds).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CustomGpuLib {
    pub path: String,
    /// Acceleration type: "cuda" | "rocm" | "vulkan"
    pub accel: String,
    /// Target OS: "linux" | "windows"
    pub os: String,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama_model: Option<crate::models::OllamaModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lmstudio_model: Option<crate::models::LMStudioModel>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub quantizations: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config_values: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_file: Option<String>,
    /// Raw `.args` content overriding both schema-driven generation and any
    /// `args_file`. Set when the user manually edits the args via the TUI's
    /// .args editor or by other interfaces wanting to bypass the schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args_override: Option<String>,
    /// When true, drop the mmproj file role from the build even if the
    /// selected model offers one. Lets the user opt out of the vision adapter.
    #[serde(default, skip_serializing_if = "is_false")]
    pub mmproj_disabled: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,

    /// Extra files to bundle alongside a custom model (role → file path).
    /// Used when the user loads a local weights file and wants to also
    /// include an mmproj (vision adapter) or other companion file.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub custom_companion_files: HashMap<String, String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gpu_libs: Vec<String>,

    /// GPU libraries specified by local file path (not from version's gpu_libs catalog).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_gpu_libs: Vec<CustomGpuLib>,

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

        // Model (only default if no model source at all)
        if self.model_id.is_none() && self.custom_model_file.is_none()
            && self.ollama_model.is_none() && self.lmstudio_model.is_none() {
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

        // Output path — derive from registry model or custom file
        if self.output_path.is_none() {
            if let Some(ref model_id) = self.model_id {
                // Registry model: use quantization filename stem
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
            } else if let Some(ref custom_path) = self.custom_model_file {
                // Local file: use filename stem
                let stem = std::path::Path::new(custom_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("model");
                let dir = crate::models::get_default_output_dir();
                self.output_path = Some(format!("{}/{}.llamafile", dir, stem));
            }
        }
    }
}

/// Construct a fully configured `Builder` from a `SavedState`.
///
/// This is the **single canonical** SavedState → Builder converter.
/// Every interface (TUI, CLI, JSON-RPC, HTTP) goes through here so
/// they cannot drift on which fields they honour, how version-only
/// recipes resolve, how unknown quants/models surface, or how GPU
/// specs parse. CLAUDE.md's "rebuilds always construct a fresh
/// Builder from SavedState" depends on this being the only path.
///
/// Returns the populated builder ready for `set_cancel_flag` + `build`.
pub fn build_from_state(
    state: &SavedState,
    config: crate::config_manager::BuilderConfig,
    cache: crate::cache::Cache,
) -> Result<crate::builder::Builder, String> {
    let mut builder = crate::builder::Builder::new(config, cache);
    apply_saved_state(&mut builder, state)?;
    Ok(builder)
}

/// Apply a `SavedState` to an already-constructed `Builder`. Errors are
/// surfaced for: unknown registry version, unknown registry model, and
/// unknown quantization name. Defaults are applied when the state
/// leaves a slot unset (no version → first default version, no model →
/// default model, etc.).
pub fn apply_saved_state(
    builder: &mut crate::builder::Builder,
    state: &SavedState,
) -> Result<(), String> {
    use std::path::Path;

    // Version
    if let Some(ref runtime) = state.custom_runtime {
        builder.use_custom_runtime(Path::new(runtime));
    } else if let (Some(ref group), Some(ref version)) =
        (&state.version_group, &state.version)
    {
        builder.select_version(group, version);
    } else if let Some(ref version) = state.version {
        // Version-only (no group): substring-search every group's
        // releases. Lets `recipe set-version 0.10.0` work without the
        // user knowing which group it lives in.
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
    } else {
        builder.select_default_version();
    }

    // Model
    if let Some(ref path) = state.custom_model_file {
        builder.select_custom_model(Path::new(path));
        // Companion files (e.g. mmproj for multimodal local models)
        for (role, companion_path) in &state.custom_companion_files {
            builder.add_custom_companion_file(role, Path::new(companion_path));
        }
    } else if let Some(ref model) = state.ollama_model {
        builder.select_ollama_model(model.clone());
    } else if let Some(ref model) = state.lmstudio_model {
        builder.select_lmstudio_model(model.clone());
    } else if let Some(ref id) = state.model_id {
        builder.select_registry_model(id);
        if builder.state().selected_model_id.is_none() {
            return Err(format!("Model '{}' not found in registry", id));
        }
    } else {
        builder.select_default_model();
    }

    // Quantizations — per-role. Errors when the model has the role but
    // no matching quant; silently no-ops when the model has no such role
    // (recipe carries a stale quant for a now-removed file role).
    for (role, quant_name) in &state.quantizations {
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
    for (key, value) in &state.config_values {
        builder.set_config_value(key, value.clone());
    }

    // Args override: an inline `args_override` (e.g. set via the TUI's
    // .args editor and saved into a recipe) takes precedence over a
    // file path. Either way the builder ends up with the raw content.
    if let Some(ref content) = state.args_override {
        builder.set_args_override(content);
    } else if let Some(ref args_file) = state.args_file {
        let content = std::fs::read_to_string(args_file)
            .map_err(|e| format!("Failed to read args file '{}': {e}", args_file))?;
        builder.set_args_override(&content);
    }

    // Output path
    if let Some(ref path) = state.output_path {
        builder.set_output_path(Path::new(path));
    }

    // mmproj_disabled: drop the vision adapter even if the selected
    // model offers one.
    if state.mmproj_disabled {
        builder.disable_file_role("mmproj");
    }

    // GPU libs (registry-resolved, "<accel>[:<os>]" specs)
    for spec in &state.gpu_libs {
        let (accel, os_list) = crate::models::parse_gpu_spec(spec)?;
        for os in os_list {
            builder.add_gpu_lib(accel, os);
        }
    }

    // Custom GPU libs (local file paths)
    for lib in &state.custom_gpu_libs {
        let path = Path::new(&lib.path);
        let (accel, os_list) =
            crate::models::parse_gpu_spec(&format!("{}:{}", lib.accel, lib.os))?;
        for os in os_list {
            builder.add_custom_gpu_lib(path, accel, os);
        }
    }

    // Custom binaries
    if let Some(ref path) = state.custom_zipalign {
        builder.use_custom_zipalign(Path::new(path));
    }

    Ok(())
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

    /// Find by name or UUID string (full or 8-char prefix as shown in `list` output).
    pub fn find_by_name_or_id(&self, query: &str) -> Result<Option<BuildRecipe>, Error> {
        if let Ok(id) = Uuid::parse_str(query) {
            return match self.get(id) {
                Ok(r) => Ok(Some(r)),
                Err(Error::NotFound(_)) => Ok(None),
                Err(e) => Err(e),
            };
        }
        // Try 8-char hex prefix match (matches what `list` displays)
        if query.len() >= 8 && query.chars().all(|c| c.is_ascii_hexdigit()) {
            let recipes = self.list()?;
            let matches: Vec<_> = recipes.into_iter()
                .filter(|r| r.id.to_string().starts_with(query))
                .collect();
            if matches.len() == 1 {
                return Ok(Some(matches.into_iter().next().unwrap()));
            }
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

    use crate::test_utils::test_config;

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

    // ── build_from_state / apply_saved_state ──
    //
    // These cover the canonical SavedState → Builder converter shared by
    // every interface (TUI, CLI, JSON-RPC, HTTP). Coverage lives here so
    // it stays close to the implementation; interface-specific tests
    // (CLI's recipe pointer, TUI's flow tests) don't duplicate the wiring.

    fn test_cache() -> crate::cache::Cache {
        crate::cache::Cache::new(std::path::PathBuf::from("/tmp/test-cache"))
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

    /// Regression: a recipe saved with `args_override` (e.g. from the
    /// TUI's .args editor) must build with that override applied.
    /// Earlier CLI code only honoured `args_file`, so the override
    /// was silently dropped.
    #[test]
    fn build_from_state_applies_args_override() {
        let saved = SavedState {
            args_override: Some("# custom\n-m\n/zip/x.gguf\n".to_string()),
            ..Default::default()
        };
        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        assert_eq!(builder.resolved_args(), "# custom\n-m\n/zip/x.gguf\n");
    }

    /// Regression: `mmproj_disabled` must drop the vision adapter even
    /// when the selected model offers one.
    #[test]
    fn build_from_state_honours_mmproj_disabled() {
        let saved = SavedState {
            model_id: Some("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string()),
            mmproj_disabled: true,
            ..Default::default()
        };
        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        let has_mmproj = builder
            .resolved_components()
            .iter()
            .any(|c| matches!(c.role, crate::ComponentRole::MmprojWeights));
        assert!(
            !has_mmproj,
            "mmproj_disabled should drop the vision-adapter component, got {:?}",
            builder.resolved_components(),
        );
    }

    /// Regression for the TUI/CLI drift the previous review surfaced:
    /// the TUI used to silently ignore `custom_companion_files`. Now
    /// that the converter is shared, both paths bundle the companion.
    #[test]
    fn build_from_state_applies_custom_companion_files() {
        let mut companions = HashMap::new();
        companions.insert("mmproj".to_string(), "/tmp/companion-mmproj.gguf".to_string());
        let saved = SavedState {
            custom_model_file: Some("/tmp/local-weights.gguf".to_string()),
            custom_companion_files: companions,
            ..Default::default()
        };
        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        assert_eq!(
            builder.state().resolved_mmproj_path.as_deref(),
            Some("/tmp/companion-mmproj.gguf"),
            "companion file must be applied to the build state",
        );
    }

    /// Regression: version-only recipes (no group) must locate the
    /// release across all groups, not fall back to the default.
    #[test]
    fn build_from_state_resolves_version_only_recipe() {
        let saved = SavedState {
            version: Some("0.10.0".to_string()),
            ..Default::default()
        };
        let builder = build_from_state(&saved, test_config(), test_cache()).unwrap();
        assert_eq!(
            builder.state().selected_release_version.as_deref(),
            Some("0.10.0"),
            "version-only recipe must pick the matching release across groups",
        );
    }

    /// Regression: an unknown version-only recipe must error, not
    /// silently fall through to the default.
    #[test]
    fn build_from_state_unknown_version_errors() {
        let saved = SavedState {
            version: Some("99.99.99".to_string()),
            ..Default::default()
        };
        let err = build_from_state(&saved, test_config(), test_cache())
            .err()
            .expect("expected error");
        assert!(err.contains("Version '99.99.99' not found"), "got: {err}");
    }

    /// Regression: an unknown model id must error.
    #[test]
    fn build_from_state_unknown_model_errors() {
        let saved = SavedState {
            model_id: Some("not/a-real-model".to_string()),
            ..Default::default()
        };
        let err = build_from_state(&saved, test_config(), test_cache())
            .err()
            .expect("expected error");
        assert!(err.contains("not/a-real-model"), "got: {err}");
    }

    /// Regression: an unknown quantization name on a known role must
    /// error rather than silently falling back to the model default.
    #[test]
    fn build_from_state_unknown_quantization_errors() {
        let mut quants = HashMap::new();
        quants.insert("weights".to_string(), "Q99_XYZ".to_string());
        let saved = SavedState {
            model_id: Some("Qwen/Qwen3-0.6B-GGUF".to_string()),
            quantizations: quants,
            ..Default::default()
        };
        let err = build_from_state(&saved, test_config(), test_cache())
            .err()
            .expect("expected error");
        assert!(err.contains("Q99_XYZ"), "got: {err}");
    }
}

use serde::Deserialize;
use std::path::PathBuf;

use crate::error::Error;
use crate::models::{ConfigSchemaItem, LlamafileVersionGroup, Model};

const BUNDLED_VERSIONS: &str = include_str!("../../../data/versions.json");
const BUNDLED_MODELS: &str = include_str!("../../../data/models.json");
const BUNDLED_CONFIG: &str = include_str!("../../../data/config.json");

/// The builder's own version, from Cargo.toml at compile time.
const BUILDER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default remote URL for fetching updated config files (GitHub raw content).
const DEFAULT_REMOTE_URL: &str =
    "https://raw.githubusercontent.com/mozilla-ai/llamafile-builder/main/data";

/// Versioned envelope around a JSON config file.
#[derive(Debug, Deserialize)]
struct ConfigEnvelope<T> {
    config_version: u32,
    #[serde(default)]
    min_builder_version: Option<String>,
    data: T,
}

/// How the builder checks for config updates at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateMode {
    /// Always fetch updates silently.
    Always,
    /// Never check for updates.
    Never,
    /// Ask the user before checking (default).
    Ask,
}

/// Result of checking for config updates.
#[derive(Debug)]
pub enum UpdateStatus {
    /// Local configs are up to date.
    UpToDate,
    /// Newer configs are available.
    ConfigUpdateAvailable(ConfigUpdate),
    /// The remote config requires a newer builder version.
    BuilderUpdateRequired { min_version: String },
}

/// A fetched config update ready to be applied.
#[derive(Debug)]
pub struct ConfigUpdate {
    pub versions_json: String,
    pub models_json: String,
    pub config_json: String,
    pub config_version: u32,
}

/// All loaded configuration data needed by the Builder.
#[derive(Debug, Clone)]
pub struct BuilderConfig {
    pub versions: Vec<LlamafileVersionGroup>,
    pub models: Vec<Model>,
    pub config_schema: Vec<ConfigSchemaItem>,
    pub config_version: u32,
    pub min_builder_version: Option<String>,
}

/// Manages loading, caching, and updating configuration files.
///
/// On first run, bundled fallback configs are written to `config_dir`.
/// On subsequent runs, configs are loaded from `config_dir`.
/// Optionally fetches updates from a remote URL (default: GitHub).
pub struct ConfigManager {
    config_dir: PathBuf,
    remote_url: String,
    update_mode: UpdateMode,
}

impl ConfigManager {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            remote_url: DEFAULT_REMOTE_URL.to_string(),
            update_mode: UpdateMode::Ask,
        }
    }

    pub fn with_remote_url(mut self, url: &str) -> Self {
        self.remote_url = url.to_string();
        self
    }

    pub fn with_update_mode(mut self, mode: UpdateMode) -> Self {
        self.update_mode = mode;
        self
    }

    pub fn update_mode(&self) -> UpdateMode {
        self.update_mode
    }

    /// Load configs from disk, falling back to bundled defaults if files
    /// don't exist yet.
    pub fn load(&self) -> Result<BuilderConfig, Error> {
        std::fs::create_dir_all(&self.config_dir).map_err(|e| {
            Error::Config(format!(
                "Failed to create config directory {}: {}",
                self.config_dir.display(),
                e
            ))
        })?;

        let versions_json = self.load_or_init("versions.json", BUNDLED_VERSIONS)?;
        let models_json = self.load_or_init("models.json", BUNDLED_MODELS)?;
        let config_json = self.load_or_init("config.json", BUNDLED_CONFIG)?;

        let versions_env: ConfigEnvelope<Vec<LlamafileVersionGroup>> =
            serde_json::from_str(&versions_json)
                .map_err(|e| Error::Config(format!("Invalid versions.json: {e}")))?;
        let models_env: ConfigEnvelope<Vec<Model>> = serde_json::from_str(&models_json)
            .map_err(|e| Error::Config(format!("Invalid models.json: {e}")))?;
        let config_env: ConfigEnvelope<Vec<ConfigSchemaItem>> =
            serde_json::from_str(&config_json)
                .map_err(|e| Error::Config(format!("Invalid config.json: {e}")))?;

        // Use the highest config_version among the three files
        let config_version = versions_env
            .config_version
            .max(models_env.config_version)
            .max(config_env.config_version);

        // Use the most restrictive min_builder_version
        let min_builder_version = [
            &versions_env.min_builder_version,
            &models_env.min_builder_version,
            &config_env.min_builder_version,
        ]
        .iter()
        .filter_map(|v| v.as_ref())
        .max()
        .cloned();

        // Check if this builder version can handle the config
        if let Some(ref min_ver) = min_builder_version {
            if BUILDER_VERSION < min_ver.as_str() {
                tracing::warn!(
                    builder_version = BUILDER_VERSION,
                    min_required = %min_ver,
                    "config requires a newer builder version"
                );
            }
        }

        let versions = versions_env.data;
        let models = models_env.data;
        let config_schema = config_env.data;

        // Validate: at most one default per list
        let default_version_count = versions.iter().filter(|v| v.default).count();
        if default_version_count > 1 {
            tracing::warn!(
                count = default_version_count,
                "multiple version groups marked as default, using the first one"
            );
        }
        let default_model_count = models.iter().filter(|m| m.default).count();
        if default_model_count > 1 {
            tracing::warn!(
                count = default_model_count,
                "multiple models marked as default, using the first one"
            );
        }

        Ok(BuilderConfig {
            versions,
            models,
            config_schema,
            config_version,
            min_builder_version,
        })
    }

    /// Check the remote URL for newer configs.
    pub async fn check_for_updates(&self) -> Result<UpdateStatus, Error> {
        let local = self.load()?;

        let client = reqwest::Client::builder()
            .user_agent(format!("llamafile-builder/{}", BUILDER_VERSION))
            .build()?;

        // Fetch all three files
        let versions_url = format!("{}/versions.json", self.remote_url);
        let models_url = format!("{}/models.json", self.remote_url);
        let config_url = format!("{}/config.json", self.remote_url);

        let (versions_resp, models_resp, config_resp) = tokio::try_join!(
            client.get(&versions_url).send(),
            client.get(&models_url).send(),
            client.get(&config_url).send(),
        )?;

        if !versions_resp.status().is_success()
            || !models_resp.status().is_success()
            || !config_resp.status().is_success()
        {
            return Ok(UpdateStatus::UpToDate);
        }

        let versions_json = versions_resp.text().await?;
        let models_json = models_resp.text().await?;
        let config_json = config_resp.text().await?;

        // Parse envelopes to get version info
        let remote_versions: ConfigEnvelope<serde_json::Value> =
            serde_json::from_str(&versions_json)
                .map_err(|e| Error::Config(format!("Remote versions.json invalid: {e}")))?;
        let remote_models: ConfigEnvelope<serde_json::Value> =
            serde_json::from_str(&models_json)
                .map_err(|e| Error::Config(format!("Remote models.json invalid: {e}")))?;
        let remote_config: ConfigEnvelope<serde_json::Value> =
            serde_json::from_str(&config_json)
                .map_err(|e| Error::Config(format!("Remote config.json invalid: {e}")))?;

        let remote_version = remote_versions
            .config_version
            .max(remote_models.config_version)
            .max(remote_config.config_version);

        // Check if the remote config requires a newer builder
        let remote_min_builder = [
            &remote_versions.min_builder_version,
            &remote_models.min_builder_version,
            &remote_config.min_builder_version,
        ]
        .iter()
        .filter_map(|v| v.as_ref())
        .max()
        .cloned();

        if let Some(ref min_ver) = remote_min_builder {
            if BUILDER_VERSION < min_ver.as_str() {
                return Ok(UpdateStatus::BuilderUpdateRequired {
                    min_version: min_ver.clone(),
                });
            }
        }

        // Compare versions
        if remote_version <= local.config_version {
            return Ok(UpdateStatus::UpToDate);
        }

        Ok(UpdateStatus::ConfigUpdateAvailable(ConfigUpdate {
            versions_json,
            models_json,
            config_json,
            config_version: remote_version,
        }))
    }

    /// Write fetched configs to the local config directory.
    pub fn apply_update(&self, update: &ConfigUpdate) -> Result<(), Error> {
        std::fs::write(
            self.config_dir.join("versions.json"),
            &update.versions_json,
        )?;
        std::fs::write(self.config_dir.join("models.json"), &update.models_json)?;
        std::fs::write(self.config_dir.join("config.json"), &update.config_json)?;
        tracing::info!(config_version = update.config_version, "config update applied");
        Ok(())
    }

    /// Load a config file from disk.
    ///
    /// If the file doesn't exist, or if it matches the bundled default exactly
    /// (meaning no remote update has replaced it), write the current bundled
    /// default. This ensures that recompiling with updated `data/*.json` files
    /// always takes effect without requiring users to clear their cache.
    fn load_or_init(&self, filename: &str, bundled: &str) -> Result<String, Error> {
        let path = self.config_dir.join(filename);

        if path.exists() {
            let on_disk = std::fs::read_to_string(&path).map_err(|e| {
                Error::Config(format!("Failed to read {}: {}", path.display(), e))
            })?;

            if on_disk != bundled {
                tracing::debug!(filename, "using cached config (differs from bundled)");
                return Ok(on_disk);
            }
        }

        tracing::debug!(filename, "writing bundled default config");
        std::fs::write(&path, bundled).map_err(|e| {
            Error::Config(format!("Failed to write {}: {}", path.display(), e))
        })?;
        Ok(bundled.to_string())
    }
}

/// Parse the `data` field from a versioned config JSON string.
/// Useful in tests and other contexts that need the inner data without
/// going through ConfigManager.
pub fn parse_config_data<T: serde::de::DeserializeOwned>(json: &str) -> Result<T, Error> {
    let env: ConfigEnvelope<T> =
        serde_json::from_str(json).map_err(|e| Error::Config(format!("Invalid config: {e}")))?;
    Ok(env.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_creates_dir_and_writes_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        assert!(!config_dir.exists());

        let cm = ConfigManager::new(config_dir.clone());
        let config = cm.load().unwrap();

        assert!(config_dir.exists());
        assert!(config_dir.join("versions.json").exists());
        assert!(config_dir.join("models.json").exists());
        assert!(config_dir.join("config.json").exists());

        assert!(!config.versions.is_empty());
        assert!(!config.models.is_empty());
        assert!(!config.config_schema.is_empty());
        assert_eq!(config.config_version, 1);
        assert_eq!(config.min_builder_version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn load_reads_existing_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        let empty_envelope = r#"{"config_version": 5, "min_builder_version": "0.2.0", "data": []}"#;
        std::fs::write(config_dir.join("versions.json"), empty_envelope).unwrap();
        std::fs::write(config_dir.join("models.json"), empty_envelope).unwrap();
        std::fs::write(config_dir.join("config.json"), empty_envelope).unwrap();

        let cm = ConfigManager::new(config_dir);
        let config = cm.load().unwrap();

        assert!(config.versions.is_empty());
        assert!(config.models.is_empty());
        assert!(config.config_schema.is_empty());
        assert_eq!(config.config_version, 5);
        assert_eq!(config.min_builder_version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn load_fails_on_invalid_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(config_dir.join("versions.json"), "not json").unwrap();
        let envelope = r#"{"config_version": 1, "data": []}"#;
        std::fs::write(config_dir.join("models.json"), envelope).unwrap();
        std::fs::write(config_dir.join("config.json"), envelope).unwrap();

        let cm = ConfigManager::new(config_dir);
        let result = cm.load();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("versions.json"));
    }

    #[test]
    fn load_fails_on_bare_array() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Bare array (old format) should fail -- we only support the envelope
        std::fs::write(config_dir.join("versions.json"), "[]").unwrap();
        let envelope = r#"{"config_version": 1, "data": []}"#;
        std::fs::write(config_dir.join("models.json"), envelope).unwrap();
        std::fs::write(config_dir.join("config.json"), envelope).unwrap();

        let cm = ConfigManager::new(config_dir);
        let result = cm.load();
        assert!(result.is_err());
    }

    #[test]
    fn apply_update_writes_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        let cm = ConfigManager::new(config_dir.clone());
        let update = ConfigUpdate {
            versions_json: r#"{"config_version": 2, "data": [{"name":"test","tag":"latest","releases":[]}]}"#.to_string(),
            models_json: r#"{"config_version": 2, "data": []}"#.to_string(),
            config_json: r#"{"config_version": 2, "data": []}"#.to_string(),
            config_version: 2,
        };
        cm.apply_update(&update).unwrap();

        let content = std::fs::read_to_string(config_dir.join("versions.json")).unwrap();
        assert!(content.contains("test"));
    }

    #[test]
    fn bundled_configs_parse_correctly() {
        let v: ConfigEnvelope<Vec<LlamafileVersionGroup>> =
            serde_json::from_str(BUNDLED_VERSIONS).expect("bundled versions.json must be valid");
        assert!(!v.data.is_empty());
        assert_eq!(v.config_version, 1);

        let m: ConfigEnvelope<Vec<Model>> =
            serde_json::from_str(BUNDLED_MODELS).expect("bundled models.json must be valid");
        assert!(!m.data.is_empty());

        let c: ConfigEnvelope<Vec<ConfigSchemaItem>> =
            serde_json::from_str(BUNDLED_CONFIG).expect("bundled config.json must be valid");
        assert!(!c.data.is_empty());
    }

    #[test]
    fn config_version_uses_highest_across_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(
            config_dir.join("versions.json"),
            r#"{"config_version": 3, "data": []}"#,
        ).unwrap();
        std::fs::write(
            config_dir.join("models.json"),
            r#"{"config_version": 1, "data": []}"#,
        ).unwrap();
        std::fs::write(
            config_dir.join("config.json"),
            r#"{"config_version": 2, "data": []}"#,
        ).unwrap();

        let cm = ConfigManager::new(config_dir);
        let config = cm.load().unwrap();
        assert_eq!(config.config_version, 3);
    }
}

use std::path::PathBuf;

use crate::error::Error;
use crate::models::{ConfigSchemaItem, LlamafileVersionGroup, Model};

const BUNDLED_VERSIONS: &str = include_str!("../../../data/versions.json");
const BUNDLED_MODELS: &str = include_str!("../../../data/models.json");
const BUNDLED_CONFIG: &str = include_str!("../../../data/config.json");

/// Default remote URL for fetching updated config files (GitHub raw content).
const DEFAULT_REMOTE_URL: &str =
    "https://raw.githubusercontent.com/Mozilla-Ocho/llamafile-builder/main/data";

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

        let versions: Vec<LlamafileVersionGroup> = serde_json::from_str(&versions_json)
            .map_err(|e| Error::Config(format!("Invalid versions.json: {e}")))?;
        let models: Vec<Model> = serde_json::from_str(&models_json)
            .map_err(|e| Error::Config(format!("Invalid models.json: {e}")))?;
        let config_schema: Vec<ConfigSchemaItem> = serde_json::from_str(&config_json)
            .map_err(|e| Error::Config(format!("Invalid config.json: {e}")))?;

        Ok(BuilderConfig {
            versions,
            models,
            config_schema,
            config_version: 1, // Current format is bare arrays, version 1
            min_builder_version: None,
        })
    }

    /// Check the remote URL for newer configs.
    pub async fn check_for_updates(&self) -> Result<UpdateStatus, Error> {
        let client = reqwest::Client::builder()
            .user_agent("llamafile-builder/0.1.0")
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
            return Ok(UpdateStatus::UpToDate); // Can't reach remote, assume up to date
        }

        let versions_json = versions_resp.text().await?;
        let models_json = models_resp.text().await?;
        let config_json = config_resp.text().await?;

        // Validate fetched JSON before accepting
        let _: Vec<LlamafileVersionGroup> = serde_json::from_str(&versions_json)
            .map_err(|e| Error::Config(format!("Remote versions.json invalid: {e}")))?;
        let _: Vec<Model> = serde_json::from_str(&models_json)
            .map_err(|e| Error::Config(format!("Remote models.json invalid: {e}")))?;
        let _: Vec<ConfigSchemaItem> = serde_json::from_str(&config_json)
            .map_err(|e| Error::Config(format!("Remote config.json invalid: {e}")))?;

        // For now, always report as available if fetch succeeded.
        // Future: compare version fields in a wrapper object.
        Ok(UpdateStatus::ConfigUpdateAvailable(ConfigUpdate {
            versions_json,
            models_json,
            config_json,
            config_version: 1,
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
        Ok(())
    }

    /// Load a config file from disk, or write the bundled default and return it.
    fn load_or_init(&self, filename: &str, bundled: &str) -> Result<String, Error> {
        let path = self.config_dir.join(filename);
        if path.exists() {
            std::fs::read_to_string(&path).map_err(|e| {
                Error::Config(format!("Failed to read {}: {}", path.display(), e))
            })
        } else {
            std::fs::write(&path, bundled).map_err(|e| {
                Error::Config(format!("Failed to write default {}: {}", path.display(), e))
            })?;
            Ok(bundled.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_creates_dir_and_writes_defaults() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        // Dir doesn't exist yet
        assert!(!config_dir.exists());

        let cm = ConfigManager::new(config_dir.clone());
        let config = cm.load().unwrap();

        // Dir was created
        assert!(config_dir.exists());
        // Files were written
        assert!(config_dir.join("versions.json").exists());
        assert!(config_dir.join("models.json").exists());
        assert!(config_dir.join("config.json").exists());

        // Data was parsed
        assert!(!config.versions.is_empty());
        assert!(!config.models.is_empty());
        assert!(!config.config_schema.is_empty());
    }

    #[test]
    fn load_reads_existing_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Write minimal valid JSON
        std::fs::write(config_dir.join("versions.json"), "[]").unwrap();
        std::fs::write(config_dir.join("models.json"), "[]").unwrap();
        std::fs::write(config_dir.join("config.json"), "[]").unwrap();

        let cm = ConfigManager::new(config_dir);
        let config = cm.load().unwrap();

        assert!(config.versions.is_empty());
        assert!(config.models.is_empty());
        assert!(config.config_schema.is_empty());
    }

    #[test]
    fn load_fails_on_invalid_json() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        std::fs::write(config_dir.join("versions.json"), "not json").unwrap();
        std::fs::write(config_dir.join("models.json"), "[]").unwrap();
        std::fs::write(config_dir.join("config.json"), "[]").unwrap();

        let cm = ConfigManager::new(config_dir);
        let result = cm.load();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("versions.json"));
    }

    #[test]
    fn apply_update_writes_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config_dir = tmp.path().join("config");
        std::fs::create_dir_all(&config_dir).unwrap();

        let cm = ConfigManager::new(config_dir.clone());
        let update = ConfigUpdate {
            versions_json: "[{\"name\":\"test\",\"tag\":\"latest\",\"releases\":[]}]".to_string(),
            models_json: "[]".to_string(),
            config_json: "[]".to_string(),
            config_version: 2,
        };
        cm.apply_update(&update).unwrap();

        let content = std::fs::read_to_string(config_dir.join("versions.json")).unwrap();
        assert!(content.contains("test"));
    }

    #[test]
    fn with_remote_url_and_update_mode() {
        let cm = ConfigManager::new(PathBuf::from("/tmp"))
            .with_remote_url("https://custom.example.com/data")
            .with_update_mode(UpdateMode::Always);

        assert_eq!(cm.remote_url, "https://custom.example.com/data");
        assert_eq!(cm.update_mode(), UpdateMode::Always);
    }

    #[test]
    fn bundled_configs_parse_correctly() {
        // Verify the bundled JSON is valid
        let _: Vec<LlamafileVersionGroup> =
            serde_json::from_str(BUNDLED_VERSIONS).expect("bundled versions.json must be valid");
        let _: Vec<Model> =
            serde_json::from_str(BUNDLED_MODELS).expect("bundled models.json must be valid");
        let _: Vec<ConfigSchemaItem> =
            serde_json::from_str(BUNDLED_CONFIG).expect("bundled config.json must be valid");
    }
}

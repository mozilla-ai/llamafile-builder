//! Stable test fixtures. The JSON files are private copies of `data/*.json`
//! at the time the test suite was written — keeping them separate
//! immunises the test suite against unrelated changes to the bundled
//! defaults (a new model added in production must not flap snapshots).

use llamafile_builder_core::config_manager::{parse_config_data, BuilderConfig};

pub const VERSIONS_JSON: &str = include_str!("fixtures/versions.json");
pub const MODELS_JSON: &str = include_str!("fixtures/models.json");
pub const CONFIG_JSON: &str = include_str!("fixtures/config.json");

/// Build a fully populated `BuilderConfig` from the test fixtures.
pub fn builder_config() -> BuilderConfig {
    BuilderConfig {
        versions: parse_config_data(VERSIONS_JSON).expect("fixture versions.json must parse"),
        models: parse_config_data(MODELS_JSON).expect("fixture models.json must parse"),
        config_schema: parse_config_data(CONFIG_JSON).expect("fixture config.json must parse"),
        config_version: 1,
        min_builder_version: None,
    }
}

//! Test fixtures shared across the workspace's tests.
//!
//! Behind the `test-utils` feature so dependent crates pick it up via
//! their `dev-dependencies`. Within this crate the module is also
//! visible under `#[cfg(test)]`, so internal tests don't need to opt in.
//!
//! The JSON files under `tests/support/fixtures/` are private copies of
//! `data/*.json` at the time the test suite was written. Keeping them
//! separate immunises the test suite against unrelated changes to the
//! bundled defaults — a new model added in production must not break
//! tests that asserted against a specific quantization index.

use crate::config_manager::{parse_config_data, BuilderConfig};

pub const VERSIONS_JSON: &str = include_str!("../tests/support/fixtures/versions.json");
pub const MODELS_JSON: &str = include_str!("../tests/support/fixtures/models.json");
pub const CONFIG_JSON: &str = include_str!("../tests/support/fixtures/config.json");

pub fn test_config() -> BuilderConfig {
    BuilderConfig {
        versions: parse_config_data(VERSIONS_JSON).expect("fixture versions.json must parse"),
        models: parse_config_data(MODELS_JSON).expect("fixture models.json must parse"),
        config_schema: parse_config_data(CONFIG_JSON).expect("fixture config.json must parse"),
        config_version: 1,
        min_builder_version: None,
    }
}

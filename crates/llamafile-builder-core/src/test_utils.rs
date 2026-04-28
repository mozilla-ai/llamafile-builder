//! Test fixtures shared across the workspace's tests.
//!
//! Behind the `test-utils` feature so dependent crates pick it up via
//! their `dev-dependencies`. Within this crate the module is also
//! visible under `#[cfg(test)]`, so internal tests don't need to opt in.

use crate::config_manager::{parse_config_data, BuilderConfig};

/// `BuilderConfig` parsed from the bundled `data/*.json` files. The
/// six previously-duplicated copies of this helper across the
/// workspace tests now route through here.
pub fn test_config() -> BuilderConfig {
    BuilderConfig {
        versions: parse_config_data(include_str!("../../../data/versions.json"))
            .expect("bundled versions.json must parse"),
        models: parse_config_data(include_str!("../../../data/models.json"))
            .expect("bundled models.json must parse"),
        config_schema: parse_config_data(include_str!("../../../data/config.json"))
            .expect("bundled config.json must parse"),
        config_version: 1,
        min_builder_version: None,
    }
}

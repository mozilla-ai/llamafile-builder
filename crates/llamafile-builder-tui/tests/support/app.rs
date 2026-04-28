//! `App` factory wired to a temp `ConfigStore` and the test fixtures.

use std::path::PathBuf;

use llamafile_builder_core::config_store::ConfigStore;
use llamafile_builder_tui::app::App;
use tempfile::TempDir;

use super::fixtures;

/// An `App` plus the temp dir backing its recipe store. The temp dir is
/// returned alongside so the caller can keep it alive (dropping it would
/// delete the store mid-test).
pub struct TestApp {
    pub app: App,
    pub recipes_dir: TempDir,
    pub cache_dir: TempDir,
}

impl TestApp {
    /// Build a fresh `App` with the test fixtures, an empty temp recipe
    /// store, and an empty temp cache dir. No popup is open; default
    /// version + model are selected by `App::new`.
    ///
    /// The output path is forced to a stable, machine-independent value
    /// (`/tmp/test.llamafile`) so rendered snapshots don't pick up the
    /// developer's home/desktop directory. Tests that exercise
    /// auto-derivation should clear `app.output_path` explicitly.
    pub fn new() -> Self {
        let recipes_dir = TempDir::new().expect("create recipes tempdir");
        let cache_dir = TempDir::new().expect("create cache tempdir");
        let store = ConfigStore::new(recipes_dir.path().to_path_buf());
        let mut app = App::new(fixtures::builder_config(), cache_dir.path().to_path_buf(), store);
        app.output_path = Some("/tmp/test.llamafile".to_string());
        Self { app, recipes_dir, cache_dir }
    }

    pub fn recipes_path(&self) -> PathBuf {
        self.recipes_dir.path().to_path_buf()
    }
}

impl Default for TestApp {
    fn default() -> Self {
        Self::new()
    }
}

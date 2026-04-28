//! Flow 2 — custom-weights-file build.
//!
//! The spec table describes "version → custom weights file → output →
//! build", but the TUI's Model Select popup only offers Registry / Ollama
//! / LM Studio tabs — there is no in-TUI affordance for picking a local
//! `.gguf` file. Until that affordance is added, the only way for a
//! user-driven custom-model build to reach `prepare_build()` is via a
//! recipe loaded from the CLI / `interactive` mode. This test exercises
//! exactly that path: it pre-stages a recipe with `custom_model_file`
//! set, loads it through the Load Recipe popup, and asserts the
//! resulting `SavedState` matches the spec's contract for Flow #2
//! (`custom_model_file` populated, `model_id` cleared).

mod support;

use std::path::PathBuf;

use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore, SavedState};
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn custom_model_build_via_recipe_load() {
    let TestApp { mut app, recipes_dir, .. } = TestApp::new();

    // Stage a recipe carrying a custom-weights-file selection. We write
    // it directly to the on-disk store the test's App is wired to. This
    // is not a TUI interaction — it stands in for a recipe that already
    // existed before the TUI session started (saved by another tool).
    let store = ConfigStore::new(recipes_dir.path().to_path_buf());
    let mut recipe = BuildRecipe::new();
    recipe.name = Some("custom-build".to_string());
    recipe.state = SavedState {
        version_group: Some("llamafile 0.10".to_string()),
        version: Some("0.10.1_alpha".to_string()),
        custom_model_file: Some("/path/to/local/model.gguf".to_string()),
        output_path: Some("/tmp/custom.llamafile".to_string()),
        ..Default::default()
    };
    store.save(&recipe).expect("stage recipe");

    let mut term = render::terminal();

    // Sanity: the staged recipe is visible to App. If this fails, the
    // ConfigStore directory wiring is the culprit — both stores must see
    // the same `recipes_dir`.
    let recipes_seen = app.list_recipes();
    assert_eq!(
        recipes_seen.len(),
        1,
        "App should see the staged recipe, recipes_dir = {}",
        recipes_dir.path().display()
    );

    // Open the Load Recipe popup with Ctrl+L.
    dispatch::keys(&mut app, &[ctrl('l')]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_load_recipe_popup", frame);

    // The list contains exactly one recipe; Enter loads it.
    dispatch::keys(&mut app, &[enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_dashboard_after_recipe_load", frame);

    // Spec contract for Flow #2: SavedState carries the custom file and
    // no registry model id.
    let saved = app.prepare_build();
    assert_eq!(
        saved.custom_model_file.as_deref(),
        Some("/path/to/local/model.gguf"),
        "custom_model_file should round-trip through recipe load"
    );
    assert!(
        saved.model_id.is_none(),
        "model_id should be cleared when a custom file is loaded, got {:?}",
        saved.model_id
    );

    // Full SavedState snapshot too — catches regressions in any other
    // field that might bleed in from the default `App::new` apply_defaults.
    insta::assert_debug_snapshot!("03_prepare_build", saved);

    // Touch unused capture so the temp dir lives for the assertions.
    let _ = PathBuf::from(recipes_dir.path());
}

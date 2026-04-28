//! Flow 11 — `custom_companion_files` + `custom_gpu_libs` survive a TUI
//! round-trip.
//!
//! Regression for the round-3 PR review: `App` was missing both fields,
//! so `to_saved_state` hardcoded empties and `load_from_recipe` ignored
//! them. A recipe created via the HTTP server's `POST /state/custom-gpu`
//! or `/state/companion-file` endpoints lost those fields the moment a
//! TUI user clicked Build (round-trip through `make_builder →
//! to_saved_state → build_from_state`) or saved the recipe.
//!
//! This test stages such a recipe directly on disk, opens it through
//! the real Ctrl+L → handle_load_recipe path, then exercises the two
//! places that previously truncated the state — `to_saved_state` and
//! `save_recipe` — and asserts both fields survive.

mod support;

use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore, CustomGpuLib, SavedState};
use std::collections::HashMap;
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn custom_companion_and_gpu_libs_round_trip_via_tui() {
    let TestApp { mut app, recipes_dir, .. } = TestApp::new();
    let store = ConfigStore::new(recipes_dir.path().to_path_buf());

    // Stage a recipe carrying both fields populated — mimics what the
    // HTTP server would have written via /state/companion-file and
    // /state/custom-gpu.
    let mut companions = HashMap::new();
    companions.insert("mmproj".to_string(), "/tmp/companion-mmproj.gguf".to_string());

    let recipe_state = SavedState {
        version_group: Some("llamafile 0.10".to_string()),
        version: Some("0.10.1_alpha".to_string()),
        custom_model_file: Some("/tmp/local-weights.gguf".to_string()),
        custom_companion_files: companions,
        custom_gpu_libs: vec![
            CustomGpuLib {
                path: "/tmp/local-cuda.so".to_string(),
                accel: "cuda".to_string(),
                os: "linux".to_string(),
            },
        ],
        output_path: Some("/tmp/custom.llamafile".to_string()),
        ..Default::default()
    };
    let mut recipe = BuildRecipe::new();
    recipe.name = Some("custom-files".to_string());
    recipe.state = recipe_state;
    let recipe_id = recipe.id;
    store.save(&recipe).expect("stage recipe");

    let mut term = render::terminal();

    // Load the recipe through the actual TUI path.
    dispatch::keys(&mut app, &[ctrl('l'), enter()]);
    let _ = render::draw(&mut term, &app);

    // Both fields must now be on App. Pre-fix App didn't even have
    // these fields so the recipe data was unreachable from here.
    assert_eq!(
        app.custom_companion_files.get("mmproj").map(String::as_str),
        Some("/tmp/companion-mmproj.gguf"),
        "load_from_recipe must populate custom_companion_files",
    );
    assert_eq!(app.custom_gpu_libs.len(), 1);
    assert_eq!(app.custom_gpu_libs[0].path, "/tmp/local-cuda.so");
    assert_eq!(app.custom_gpu_libs[0].accel, "cuda");
    assert_eq!(app.custom_gpu_libs[0].os, "linux");

    // Round-trip 1 — `to_saved_state` is the assertion point that
    // `make_builder` and `build_from_state` go through. Pre-fix it
    // hardcoded `vec![]` and `Default::default()` for these fields.
    let saved = app.to_saved_state();
    assert_eq!(
        saved.custom_companion_files.get("mmproj").map(String::as_str),
        Some("/tmp/companion-mmproj.gguf"),
        "to_saved_state must round-trip custom_companion_files",
    );
    assert_eq!(
        saved.custom_gpu_libs.len(),
        1,
        "to_saved_state must round-trip custom_gpu_libs; got {:?}",
        saved.custom_gpu_libs,
    );
    assert_eq!(saved.custom_gpu_libs[0].path, "/tmp/local-cuda.so");

    // Round-trip 2 — actually save the recipe back through the TUI's
    // save path (Ctrl+S → name → Enter), then read the file back
    // through a fresh ConfigStore. Same recipe id so it's an update,
    // not a create.
    app.recipe_id = Some(recipe_id);
    dispatch::keys(&mut app, &[ctrl('s')]);
    dispatch::keys(&mut app, &type_str("custom-files"));
    dispatch::keys(&mut app, &[enter()]);

    let store2 = ConfigStore::new(recipes_dir.path().to_path_buf());
    let written = store2.get(recipe_id).expect("recipe must exist after save");
    assert_eq!(
        written.state.custom_companion_files.get("mmproj").map(String::as_str),
        Some("/tmp/companion-mmproj.gguf"),
        "TUI save must persist custom_companion_files",
    );
    assert_eq!(
        written.state.custom_gpu_libs.len(),
        1,
        "TUI save must persist custom_gpu_libs",
    );
    assert_eq!(written.state.custom_gpu_libs[0].path, "/tmp/local-cuda.so");
}

//! Flow 7 — recipe load. Stage two recipes on disk, open the Load Recipe
//! popup, navigate the list, pick the second one, and verify every
//! observable App field tracks the loaded recipe (not the App's
//! `apply_defaults` seed).
//!
//! Flow 2 already covers the basic path; this test exercises (a) a
//! list with multiple entries, (b) navigation among them, and (c) a
//! richer SavedState (quantization + GPU libs + mmproj toggle).

mod support;

use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore, SavedState};
use llamafile_builder_core::models::{GpuAccel, Os};
use llamafile_builder_tui::app::ModelTab;
use std::collections::HashMap;
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

fn stage_recipe(store: &ConfigStore, name: &str, state: SavedState) {
    let mut recipe = BuildRecipe::new();
    recipe.name = Some(name.to_string());
    recipe.state = state;
    store.save(&recipe).expect("stage recipe");
}

#[test]
fn recipe_load_picks_second_entry_and_round_trips_state() {
    let TestApp { mut app, recipes_dir, .. } = TestApp::new();
    let store = ConfigStore::new(recipes_dir.path().to_path_buf());

    // First recipe: Qwen3-VL multimodal with mmproj disabled. Should
    // show with the default model row but disabled-mmproj indicator.
    let mut quants_a = HashMap::new();
    quants_a.insert("weights".to_string(), "Q4_K_M".to_string());
    stage_recipe(
        &store,
        "qwen3-vl-no-mmproj",
        SavedState {
            version_group: Some("llamafile 0.10".to_string()),
            version: Some("0.10.1_alpha".to_string()),
            model_id: Some("Qwen/Qwen3-VL-4B-Instruct-GGUF".to_string()),
            quantizations: quants_a,
            mmproj_disabled: true,
            output_path: Some("/tmp/qwen-vl.llamafile".to_string()),
            ..Default::default()
        },
    );

    // Second recipe: Mistral 3B with two GPU libs and a BF16 weights
    // override. This is the one we'll load.
    let mut quants_b = HashMap::new();
    quants_b.insert("weights".to_string(), "BF16".to_string());
    stage_recipe(
        &store,
        "mistral-bf16-gpu",
        SavedState {
            version_group: Some("llamafile 0.10".to_string()),
            version: Some("0.10.1_alpha".to_string()),
            model_id: Some("mistralai/Ministral-3-3B-Instruct-2512-GGUF".to_string()),
            quantizations: quants_b,
            gpu_libs: vec!["cuda:linux".to_string(), "rocm:linux".to_string()],
            output_path: Some("/tmp/mistral-gpu.llamafile".to_string()),
            ..Default::default()
        },
    );

    let mut term = render::terminal();

    // Open the Load Recipe popup. Recipes are sorted most-recently-
    // updated first by `ConfigStore::list`, so the second one we staged
    // (mistral-bf16-gpu) comes first.
    dispatch::keys(&mut app, &[ctrl('l')]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_load_recipe_popup_two_entries", frame);

    // Navigate down to the older recipe and load it.
    dispatch::keys(&mut app, &[down(), enter()]);

    // App state should now reflect the older recipe (qwen3-vl-no-mmproj).
    assert_eq!(
        app.model_id.as_deref(),
        Some("Qwen/Qwen3-VL-4B-Instruct-GGUF")
    );
    assert!(
        app.mmproj_disabled,
        "load_from_recipe must restore mmproj_disabled (regression guard for the field round-trip)",
    );
    assert_eq!(app.quantizations.get("weights"), Some(&"Q4_K_M".to_string()));
    assert_eq!(app.output_path.as_deref(), Some("/tmp/qwen-vl.llamafile"));
    assert!(app.ollama_model.is_none() && app.lmstudio_model.is_none());
    assert_eq!(app.model_source, ModelTab::Registry);
    assert!(
        app.gpu_libs.is_empty(),
        "first recipe had no GPU libs, must not pick up state from the other recipe",
    );

    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_dashboard_after_load", frame);

    // SavedState round-trips back through prepare_build.
    insta::assert_debug_snapshot!("03_prepare_build", app.to_saved_state());

    // Now load the *other* recipe (Ctrl+L again, top-of-list, Enter).
    // Verifies that loading a second recipe replaces fields cleanly —
    // mmproj_disabled must clear, GPU libs must populate.
    dispatch::keys(&mut app, &[ctrl('l'), enter()]);

    assert_eq!(
        app.model_id.as_deref(),
        Some("mistralai/Ministral-3-3B-Instruct-2512-GGUF")
    );
    assert!(!app.mmproj_disabled, "switching recipes must clear mmproj_disabled");
    assert_eq!(app.gpu_libs, vec![(GpuAccel::Cuda, Os::Linux), (GpuAccel::Rocm, Os::Linux)]);

    insta::assert_debug_snapshot!("04_prepare_build_after_swap", app.to_saved_state());
}

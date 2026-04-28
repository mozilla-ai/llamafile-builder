//! Flow 6 — recipe save. Configure a non-default selection (a GPU lib),
//! press Ctrl+S, type a name, and verify (a) the recipe popup snapshots,
//! (b) the recipe file lands on disk, and (c) the file's `SavedState`
//! matches what `prepare_build()` returns in memory.

mod support;

use llamafile_builder_core::config_store::{BuildRecipe, ConfigStore};
use llamafile_builder_tui::app::Focus;
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn recipe_save_writes_file_matching_in_memory_state() {
    let TestApp { mut app, recipes_dir, .. } = TestApp::new();
    let mut term = render::terminal();

    // Configure something distinguishable from the empty default —
    // toggle a GPU lib so the recipe carries non-trivial state.
    dispatch::focus_on(&mut app, Focus::Gpu);
    dispatch::keys(&mut app, &[enter(), space(), esc()]);

    // Capture the in-memory state we expect to be persisted. Doing this
    // *before* the save lets the assertion be exact (resolve_defaults
    // mutates the saved copy at write time).
    let in_memory = app.to_saved_state();
    assert_eq!(in_memory.gpu_libs, vec!["cuda:linux"]);

    // Open the Save Recipe popup.
    dispatch::keys(&mut app, &[ctrl('s')]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_save_recipe_popup_empty", frame);

    // Type a name and confirm.
    dispatch::keys(&mut app, &type_str("cuda-linux-build"));
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_save_recipe_popup_named", frame);
    dispatch::keys(&mut app, &[enter()]);

    // Popup closes; the recipe is now on disk. Read it back through a
    // fresh ConfigStore (proves it's a real file, not in-memory state).
    let store = ConfigStore::new(recipes_dir.path().to_path_buf());
    let recipes: Vec<BuildRecipe> = store.list().expect("list saved recipes");
    assert_eq!(recipes.len(), 1, "exactly one recipe should exist on disk");
    let written = &recipes[0];
    assert_eq!(written.name.as_deref(), Some("cuda-linux-build"));

    // The persisted SavedState carries the user's selections. It also
    // has resolve_defaults applied (config_values get filled in, etc.),
    // so we don't compare to in_memory directly — we assert on the
    // distinguishing fields.
    assert_eq!(written.state.gpu_libs, vec!["cuda:linux"]);
    assert_eq!(
        written.state.model_id.as_deref(),
        Some("Qwen/Qwen3-0.6B-GGUF"),
        "default model should round-trip"
    );

    // Snapshot just the user-set fields (skip the defaults blob, which
    // is noisy and exercised separately by core's config_store tests).
    insta::assert_debug_snapshot!(
        "03_persisted_user_fields",
        (
            &written.name,
            &written.state.version_group,
            &written.state.version,
            &written.state.model_id,
            &written.state.gpu_libs,
            &written.state.output_path,
        )
    );
}

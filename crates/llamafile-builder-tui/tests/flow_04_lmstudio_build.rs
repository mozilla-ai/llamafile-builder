//! Flow 4 — LM Studio build. Switch the Model Select popup to the
//! LM Studio tab, pick one of the fake-scaffolded models, and verify
//! `prepare_build()` carries the LM Studio selection.

mod support;

use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::local_sources::make_lmstudio_dir;
use support::render;
use support::snapshots;

#[test]
fn lmstudio_build_pick_test_model() {
    let _settings = snapshots::settings().bind_to_scope();
    let TestApp { mut app, .. } = TestApp::new();

    // Scaffold a fake LM Studio install with two models, one weights-only
    // and one with a vision adapter (mmproj). The scanner expects
    // `<root>/models/lmstudio-community/<dir_name>/*.gguf`.
    let lms_root = tempfile::TempDir::new().expect("lms tempdir");
    make_lmstudio_dir(
        lms_root.path(),
        "TestModel-7B-GGUF",
        &["TestModel-7B-Q4_K_M.gguf"],
    );
    make_lmstudio_dir(
        lms_root.path(),
        "VisionModel-3B-GGUF",
        &["VisionModel-3B-Q5_K_M.gguf", "mmproj-VisionModel-3B-f16.gguf"],
    );
    app.lmstudio_dir = Some(lms_root.path().to_string_lossy().to_string());

    let mut term = render::terminal();

    // Move from Version → Model and open the Model Select popup.
    dispatch::keys(&mut app, &[down(), enter()]);

    // Switch to the LM Studio tab (right arrow twice: Registry → Ollama →
    // LM Studio).
    dispatch::keys(&mut app, &[right(), right()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_model_popup_lmstudio_tab", frame);

    // Pick the first entry (Enter without moving — TestModel-7B-GGUF
    // sorts before VisionModel alphabetically).
    dispatch::keys(&mut app, &[enter()]);

    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_dashboard_after_lmstudio_select", frame);

    // Hard asserts so a regression in model-source clearing fails
    // loudly rather than just diffing a snapshot.
    let saved = app.to_saved_state();
    assert!(
        saved.model_id.is_none(),
        "selecting an LM Studio model must clear the registry model_id; got {:?}",
        saved.model_id,
    );
    assert!(saved.lmstudio_model.is_some(), "lmstudio_model must be populated");
    assert!(saved.ollama_model.is_none(), "ollama_model must stay None");

    insta::assert_debug_snapshot!("03_prepare_build", saved);
}

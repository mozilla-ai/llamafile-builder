//! Smoke test for the test harness itself: prove `support`'s wiring
//! works and `insta` is functional. Flow tests cover the rendered
//! dashboard at decision points along real user paths; this file
//! deliberately doesn't snapshot full frames — a zero-interaction
//! snapshot diffs on any cosmetic tweak (label, padding, key hint
//! text) without flagging a real regression.

mod support;

use support::app::TestApp;
use support::render;

/// Minimum viable render: the dashboard frame is non-empty and
/// includes the structural anchors a user reads. If any of these
/// strings disappear, the dashboard is meaningfully broken — unlike a
/// full-frame snapshot which trips on cosmetic shifts too.
#[test]
fn dashboard_renders_structural_anchors() {
    let TestApp { app, .. } = TestApp::new();
    let mut term = render::terminal();
    let frame = render::draw(&mut term, &app);

    assert!(frame.contains("llamafile builder"), "title missing from frame");
    assert!(frame.contains("Version"), "Version row missing");
    assert!(frame.contains("Model"), "Model row missing");
    assert!(frame.contains("Build"), "Build button missing");
    // Default model from the test fixtures should be visible.
    assert!(frame.contains("Qwen3 0.6B"), "default model name missing");
}

/// SavedState round-trip: prepare_build on a fresh App returns a
/// snapshot whose contents we can pin. This catches regressions in the
/// defaults or in to_saved_state's field plumbing.
#[test]
fn dashboard_default_prepare_build_snapshot() {
    let TestApp { app, .. } = TestApp::new();
    insta::assert_debug_snapshot!(app.to_saved_state());
}

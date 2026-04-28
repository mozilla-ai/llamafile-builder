//! Flow 10 — build error display.
//!
//! When a `BuildResult::Failed` arrives via the build-result channel,
//! `main.rs::run_app` raises a `Popup::BuildError` with the formatted
//! error and updates `BuildStatus::Failed`. The error popup is the only
//! UI surface a user sees on failure (the same text is also logged).
//!
//! This test injects a synthetic failure state — `App::popup =
//! BuildError {…}` plus matching `BuildStatus::Failed` — because the
//! L3 boundary is at App, not the build subsystem (per spec §9). The
//! L1 dismissal test in `events.rs` already covers the keypress
//! contract; here we add the rendered-popup snapshot and verify the
//! dashboard re-renders cleanly after dismissal.

mod support;

use llamafile_builder_tui::app::{BuildStatus, Popup};
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn build_error_popup_renders_and_dismisses() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    // Inject the synthetic failure. The actual build subsystem is
    // exercised by 121 core tests; this slice tests how the TUI
    // *displays* a failure that already occurred.
    let err_text =
        "Insufficient disk space at /tmp: need 6.4 GiB, have 1.2 GiB available";
    app.build_status = BuildStatus::Failed { error: err_text.to_string() };
    app.popup = Popup::BuildError { message: err_text.to_string() };

    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_build_error_popup", frame);

    // Per TUI.md: "Any keypress dismisses." Press something nondescript.
    dispatch::keys(&mut app, &[char(' ')]);
    assert!(matches!(app.popup, Popup::None), "any key should dismiss");

    // Dashboard re-renders without the popup; build_status stays Failed
    // until the user retries (status is informational, not modal).
    assert!(matches!(app.build_status, BuildStatus::Failed { .. }));
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_dashboard_after_dismiss", frame);
}

#[test]
fn build_error_popup_wraps_multiline_messages() {
    // A long error must be visible end-to-end in the popup. If the popup
    // truncates, this snapshot will catch it. (Wrapping correctness is a
    // ratatui Paragraph concern; this test pins the contract.)
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();
    app.popup = Popup::BuildError {
        message: "Build failed: network unreachable while resolving \
            HuggingFace model 'mistralai/Ministral-3-3B-Instruct-2512-GGUF'. \
            Check your connection and retry. If you are behind a proxy, set \
            HTTPS_PROXY before launching."
            .to_string(),
    };
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_long_error_popup", frame);
}

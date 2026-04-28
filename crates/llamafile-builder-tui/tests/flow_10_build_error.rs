//! Flow 10 — build error display.
//!
//! When a `BuildResult::Failed` arrives via the build-result channel,
//! `main.rs::run_app` raises a `Popup::BuildError` with the formatted
//! error and updates `BuildStatus::Failed`. The error popup is the only
//! UI surface a user sees on failure (the same text is also logged).
//!
//! These tests exercise the *real* dispatch — `app::apply_build_result` —
//! rather than pre-stamping `app.popup` directly. The earlier version of
//! this file pre-stamped, which would silently pass even if the
//! `Failed → BuildError` wiring in `run_app` regressed. (Same lesson as
//! the BuildPhase visitor: tests must drive the production path.)
//! `apply_build_result`'s App-state contract is also pinned by unit
//! tests in `app::tests`; here we add the rendered-popup snapshot.

mod support;

use llamafile_builder_tui::app::{apply_build_result, BuildResult, BuildStatus, Popup};
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn failed_build_result_renders_error_popup_and_dismisses() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    let err_text =
        "Insufficient disk space at /tmp: need 6.4 GiB, have 1.2 GiB available";

    // Drive the real dispatch — this is what `run_app` does on
    // `Ok(BuildResult::Failed)` from the oneshot channel. If the
    // dispatch ever stops setting the popup, this test fails.
    apply_build_result(
        &mut app,
        BuildResult::Failed { error: err_text.to_string() },
    );
    assert!(matches!(app.popup, Popup::BuildError { .. }));
    assert!(matches!(app.build_status, BuildStatus::Failed { .. }));

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
fn long_error_message_wraps_in_popup() {
    // A long error must be visible end-to-end — if the popup
    // truncates, this snapshot will catch it. Wrapping correctness is
    // a ratatui Paragraph concern; this test pins the contract.
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    apply_build_result(
        &mut app,
        BuildResult::Failed {
            error: "Build failed: network unreachable while resolving \
                HuggingFace model 'mistralai/Ministral-3-3B-Instruct-2512-GGUF'. \
                Check your connection and retry. If you are behind a proxy, set \
                HTTPS_PROXY before launching."
                .to_string(),
        },
    );

    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_long_error_popup", frame);
}

#[test]
fn complete_build_result_does_not_raise_popup() {
    // Regression guard: only Failed should raise BuildError. If the
    // dispatch starts raising popups for Complete / Cancelled, that's a
    // visible UX regression.
    let TestApp { mut app, .. } = TestApp::new();
    apply_build_result(
        &mut app,
        BuildResult::Complete {
            path: "/tmp/out.llamafile".to_string(),
            size: 100,
        },
    );
    assert!(matches!(app.popup, Popup::None));
    assert!(matches!(app.build_status, BuildStatus::Complete { .. }));
}

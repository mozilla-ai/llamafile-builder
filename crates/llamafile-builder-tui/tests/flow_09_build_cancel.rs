//! Flow 9 — build cancel mid-flight.
//!
//! Press Enter on Build → BuildStatus enters "starting"; Ctrl+C raises
//! the Cancel-build confirmation; `y` accepts → `should_quit` is set.
//!
//! The cancel-flag flip and the `Building → Cancelled` status transition
//! both live in `main.rs::run_app`'s loop, not in `events::handle_key`.
//! Asserting on those would require running the loop with a real spawned
//! build — out of scope for L3 (deferred to L5 pty per spec §5).
//! What this test *can* verify is the intent capture (failure mode A)
//! and the `should_quit` handoff that drives the rest.

mod support;

use llamafile_builder_tui::app::{BuildStatus, Focus, Popup};
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn ctrl_c_during_starting_build_confirms_cancel_and_sets_should_quit() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    // Trigger the build. Enter on the Build button moves the status
    // machine into "starting", which the main loop would then pick up
    // and spawn from. We stop here — the test asserts on the parts
    // events::handle_key controls.
    dispatch::focus_on(&mut app, Focus::Build);
    dispatch::keys(&mut app, &[enter()]);
    assert!(
        matches!(app.build_status, BuildStatus::Building { ref phase, .. } if phase == "starting"),
        "Enter on Build must put status into Building/starting; got {:?}",
        app.build_status,
    );
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_build_starting", frame);

    // Ctrl+C while building opens the cancel-confirmation popup. (The
    // L1 events test already pins down the popup variant; this is the
    // rendered version that catches visual drift in the modal.)
    dispatch::keys(&mut app, &[ctrl('c')]);
    assert!(
        matches!(app.popup, Popup::Confirm { .. }),
        "Ctrl+C during a build must open Confirm; got {:?}",
        app.popup,
    );
    assert!(!app.should_quit, "Confirm popup should not have quit yet");
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_cancel_confirm_popup", frame);

    // 'y' confirms cancellation. The popup closes and `should_quit` is
    // set; the main loop reads that flag, flips the cancel atomic on
    // the build task, and exits. We can only verify the App-side half.
    dispatch::keys(&mut app, &[char('y')]);
    assert!(matches!(app.popup, Popup::None));
    assert!(
        app.should_quit,
        "Confirm/yes must propagate intent via should_quit so run_app exits",
    );
    // BuildStatus stays Building from the App's perspective until the
    // loop receives the Cancelled result on the oneshot channel.
    assert!(matches!(app.build_status, BuildStatus::Building { .. }));
}

#[test]
fn ctrl_c_confirm_no_keeps_build_running() {
    let TestApp { mut app, .. } = TestApp::new();
    dispatch::focus_on(&mut app, Focus::Build);
    dispatch::keys(&mut app, &[enter(), ctrl('c'), char('n')]);
    assert!(matches!(app.popup, Popup::None));
    assert!(!app.should_quit, "Confirm/no must NOT set should_quit");
    assert!(matches!(app.build_status, BuildStatus::Building { .. }));
}

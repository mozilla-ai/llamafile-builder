//! Smoke test for the test harness itself: render the dashboard at the
//! default state and snapshot the resulting buffer. If this passes, the
//! `support` modules are wired up correctly and `insta` is functional.

mod support;

use support::app::TestApp;
use support::render;

#[test]
fn dashboard_default_renders() {
    let TestApp { app, .. } = TestApp::new();
    let mut term = render::terminal();
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!(frame);
}

#[test]
fn dashboard_default_prepare_build_snapshot() {
    let TestApp { app, .. } = TestApp::new();
    insta::assert_debug_snapshot!(app.prepare_build());
}

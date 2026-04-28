//! Flow 8 — `.args` override + Config row lock.
//!
//! Open the .args editor (Enter on Show .args), insert a custom flag,
//! Esc to save. Verify (a) `args_override` is populated, (b) the
//! dashboard summary changes to "manually overridden", and (c) the
//! Config row is skipped during keyboard navigation.

mod support;

use llamafile_builder_tui::app::Focus;
use support::app::TestApp;
use support::dispatch;
use support::keys::*;
use support::render;

#[test]
fn args_edit_creates_override_and_locks_config_row() {
    let TestApp { mut app, .. } = TestApp::new();
    let mut term = render::terminal();

    // Move focus to Show .args, open the editor.
    dispatch::focus_on(&mut app, Focus::ShowArgs);
    dispatch::keys(&mut app, &[enter()]);
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("01_args_editor_open_default", frame);

    // Insert a custom marker at the cursor (top-left). The textarea
    // forwards key events directly, so plain chars get typed.
    dispatch::keys(&mut app, &type_str("# CUSTOM"));
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("02_args_editor_after_typing", frame);

    // Esc saves the modified content as an override and closes the popup.
    dispatch::keys(&mut app, &[esc()]);

    // The override is set; the dashboard's Config summary now reflects
    // it ("manually overridden, check .args").
    assert!(app.args_override.is_some(), "args_override should be set");
    assert!(
        app.args_override.as_deref().unwrap().starts_with("# CUSTOM"),
        "override should start with the typed prefix",
    );
    let frame = render::draw(&mut term, &app);
    insta::assert_snapshot!("03_dashboard_with_override", frame);

    // The Config row is now locked: navigation skips it. Drive a real
    // Down keypress through the dashboard handler — that's the path
    // production uses, and it computes `args_overridden` from
    // `app.args_override.is_some()` inline. Hardcoding the boolean
    // bypasses exactly that computation, so a typo there
    // (`args_file.is_some()` say) wouldn't fail the assertion.
    dispatch::focus_on(&mut app, Focus::Output);
    dispatch::keys(&mut app, &[down()]);
    assert_eq!(
        app.focus, Focus::Gpu,
        "Down from Output must skip the locked Config row and land on Gpu; got {:?}",
        app.focus,
    );

    // SavedState carries the override verbatim.
    let saved = app.to_saved_state();
    assert!(saved.args_override.as_deref().unwrap().starts_with("# CUSTOM"));
    insta::assert_debug_snapshot!(
        "04_prepare_build_override_field",
        (
            saved.args_override.as_ref().map(|s| s.lines().take(3).collect::<Vec<_>>()),
            saved.args_file,
        )
    );
}

/// Regression guard: opening the editor and immediately closing without
/// modifications must NOT set the override. Otherwise the Config row
/// would lock spuriously.
#[test]
fn opening_and_closing_args_editor_unchanged_does_not_lock_config() {
    let TestApp { mut app, .. } = TestApp::new();
    dispatch::focus_on(&mut app, Focus::ShowArgs);
    dispatch::keys(&mut app, &[enter(), esc()]);
    assert!(
        app.args_override.is_none(),
        "no edit -> no override; got {:?}",
        app.args_override,
    );
}

//! Drive `App` state by dispatching key events through the real
//! `events::handle_key` entry point — never by mutating fields directly.
//!
//! Per `specs/TUI_TESTING.md` "Common pitfalls": bypassing `handle_event`
//! to set state directly skips the test of intent capture (failure mode A).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use llamafile_builder_tui::app::{App, Focus, Popup};
use llamafile_builder_tui::events;

/// Dispatch a single key event.
pub fn key(app: &mut App, k: KeyEvent) {
    events::handle_key(app, k);
}

/// Dispatch a sequence of key events in order.
pub fn keys(app: &mut App, ks: &[KeyEvent]) {
    for &k in ks {
        events::handle_key(app, k);
    }
}

/// Navigate dashboard focus to `target` by pressing `Down` until it
/// matches. The dashboard skips disabled rows (Mmproj when no mmproj,
/// Config when args are overridden, …) — `Focus::next` does the right
/// thing, so this loop will land on `target` if it's reachable.
///
/// Panics with the trail it stopped on if `target` is unreachable —
/// usually means the row is hidden in the current state and the test
/// has the wrong expectations.
pub fn focus_on(app: &mut App, target: Focus) {
    assert!(
        matches!(app.popup, Popup::None),
        "focus_on requires no popup, got {:?}",
        app.popup,
    );
    for _ in 0..32 {
        if app.focus == target {
            return;
        }
        events::handle_key(app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    panic!(
        "focus_on({:?}) ran 32 Down presses without landing — stuck at {:?}",
        target, app.focus
    );
}

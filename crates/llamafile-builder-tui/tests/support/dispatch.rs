//! Drive `App` state by dispatching key events through the real
//! `events::handle_key` entry point — never by mutating fields directly.
//!
//! Per `specs/TUI_TESTING.md` "Common pitfalls": bypassing `handle_event`
//! to set state directly skips the test of intent capture (failure mode A).

use crossterm::event::KeyEvent;
use llamafile_builder_tui::app::App;
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

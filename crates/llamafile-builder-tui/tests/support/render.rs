//! Render a `TestBackend` buffer to a snapshot-friendly string.
//!
//! Per `specs/TUI_TESTING.md` §6: emit pure text by default; styles only
//! when semantically meaningful. We aren't rendering styles yet — every
//! styled element in the current TUI also has a textual cue (focus arrow,
//! `*` selection mark, error text, …) — so plain text reads cleanly.

use llamafile_builder_tui::app::App;
use llamafile_builder_tui::ui;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// Build a `Terminal<TestBackend>` at the canonical 80×24 size.
pub fn terminal() -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(80, 24)).expect("construct test terminal")
}

/// Build a `Terminal<TestBackend>` at a caller-chosen size. Use for
/// multi-column popups (Model Select tabs/filter/list) where 80×24 clips.
pub fn terminal_sized(width: u16, height: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(width, height)).expect("construct test terminal")
}

/// Render the app to the terminal and return the buffer as a string. Each
/// row's trailing whitespace is trimmed, rows are joined with `\n`, and
/// the result has no trailing newline — keeps `insta::assert_snapshot!`
/// diffs minimal.
pub fn draw(term: &mut Terminal<TestBackend>, app: &App) -> String {
    term.draw(|frame| ui::draw(frame, app)).expect("draw frame");
    snapshot(term)
}

/// Convert the current backend buffer to a string without re-rendering.
pub fn snapshot(term: &Terminal<TestBackend>) -> String {
    let buf = term.backend().buffer();
    let area = buf.area;
    let mut out = String::with_capacity((area.width as usize + 1) * area.height as usize);
    for y in 0..area.height {
        let mut row = String::with_capacity(area.width as usize);
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            row.push_str(cell.symbol());
        }
        // Trim trailing whitespace; the buffer pads short rows with spaces.
        let trimmed = row.trim_end();
        out.push_str(trimmed);
        if y + 1 < area.height {
            out.push('\n');
        }
    }
    out
}

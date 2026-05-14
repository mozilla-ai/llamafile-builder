//! TUI for `llamafile-builder`. Invoked from the `llamafile-builder` binary
//! via `--tui` (see [`run`]). Integration tests under `tests/` drive the
//! submodules directly against `ratatui::backend::TestBackend`.

pub mod app;
pub mod config_ui;
pub mod events;
pub mod log_layer;
mod runner;
pub mod ui;

pub use runner::run;

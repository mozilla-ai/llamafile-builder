//! TUI library for `llamafile-builder-tui`. The binary in `main.rs` wires
//! these modules to crossterm + tokio; integration tests under `tests/`
//! drive them against `ratatui::backend::TestBackend`.

pub mod app;
pub mod config_ui;
pub mod events;
pub mod log_layer;
pub mod ui;

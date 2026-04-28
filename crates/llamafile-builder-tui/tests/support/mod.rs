//! Shared helpers for TUI integration tests.
//!
//! Per `specs/TUI_TESTING.md` §6: this module bundles fixed config
//! fixtures, a `KeyEvent` builder, an `App` factory wired to a temp
//! recipe store, and a `render` helper that flattens a `TestBackend`
//! buffer into a snapshot-friendly string.
//!
//! The `#[allow(dead_code)]` annotations are because each integration-test
//! binary uses a different subset of these helpers.

#![allow(dead_code)]

pub mod app;
pub mod fixtures;
pub mod keys;
pub mod render;

//! Shared `insta::Settings` for flow tests.
//!
//! Tempdir paths land in SavedState whenever a test scaffolds a fake
//! Ollama / LM Studio install (the scanners record absolute weights
//! paths). Those paths vary per process and per platform, so we filter
//! them to a stable `<TEMPDIR>` placeholder before snapshotting.
//!
//! Usage at the top of a test that needs the filters:
//!
//! ```ignore
//! let _guard = support::snapshots::settings().bind_to_scope();
//! ```
//!
//! The guard lives for the test scope; assertions made within the scope
//! pick up the configured filters automatically.

/// Build the project-wide insta settings preset.
pub fn settings() -> insta::Settings {
    let mut s = insta::Settings::clone_current();

    // macOS: /var/folders/<bucket>/<id>/T/.tmpXxxxxx (per-process tempdirs).
    s.add_filter(
        r"/var/folders/[A-Za-z0-9_+/]+/T/\.tmp[A-Za-z0-9]+",
        "<TEMPDIR>",
    );
    // Linux/macOS: /tmp/.tmpXxxxxx
    s.add_filter(r"/tmp/\.tmp[A-Za-z0-9]+", "<TEMPDIR>");
    // Some platforms produce /tmp/tmp.<random>
    s.add_filter(r"/tmp/tmp\.[A-Za-z0-9]+", "<TEMPDIR>");

    s
}

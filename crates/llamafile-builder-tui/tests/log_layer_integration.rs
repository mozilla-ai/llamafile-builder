//! End-to-end tests for the `TuiLogLayer` → `BuildSignals` pipeline.
//!
//! These drive the real `tracing` machinery — `info!`, `debug!`, etc. —
//! against a subscriber that has only `TuiLogLayer` attached, then read
//! the resulting `LogEntry` off the channel and assert on `BuildSignals`.
//!
//! Why these matter: a flow test that pre-stamps `app.build_status` will
//! happily pass even if the visitor never extracts the `phase` field
//! from a `tracing::info!(phase = %BuildPhase::X, …)` event. That's
//! exactly how the dead-code bug in commit 2f9ef8d shipped — the entire
//! progress-display path was broken at runtime, but every flow test
//! still went green because they never went through the layer.

use std::sync::mpsc;

use llamafile_builder_core::BuildPhase;
use llamafile_builder_tui::app::{LogEntry, LogLevel};
use llamafile_builder_tui::log_layer::TuiLogLayer;
use tracing_subscriber::layer::SubscriberExt;

/// Set up a subscriber that runs only `TuiLogLayer` over the given
/// channel, then run `f` with that subscriber installed for the
/// duration of the closure. Tracing is global; each test sandboxes
/// itself with `with_default` to avoid races against other tests.
fn with_layer<F: FnOnce()>(f: F) -> mpsc::Receiver<LogEntry> {
    let (tx, rx) = mpsc::channel();
    let layer = TuiLogLayer::new(tx);
    let subscriber = tracing_subscriber::registry()
        .with(layer)
        .with(tracing_subscriber::filter::LevelFilter::DEBUG);
    tracing::subscriber::with_default(subscriber, f);
    rx
}

#[test]
fn percent_display_phase_field_populates_signals_phase() {
    let rx = with_layer(|| {
        tracing::info!(phase = %BuildPhase::PreparingZipalign, "preparing zipalign");
    });
    let entry = rx.try_recv().expect("event should reach the channel");
    assert_eq!(
        entry.signals.phase,
        Some(BuildPhase::PreparingZipalign),
        "`%` (Display) emission of BuildPhase must be parsed by the visitor — \
         this is the contract the entire progress UI hangs on",
    );
}

#[test]
fn debug_format_phase_field_also_populates() {
    // `?BuildPhase::X` is what the spec example used; the production
    // emit sites use `%`. Both must be accepted so a future contributor
    // following the spec literally doesn't silently drop the phase.
    let rx = with_layer(|| {
        tracing::info!(phase = ?BuildPhase::Assembling, "assembling");
    });
    let entry = rx.try_recv().expect("event should reach the channel");
    assert_eq!(entry.signals.phase, Some(BuildPhase::Assembling));
}

#[test]
fn unknown_phase_value_does_not_set_phase_or_panic() {
    let rx = with_layer(|| {
        tracing::info!(phase = "garbage", "weird emission");
    });
    let entry = rx.try_recv().expect("event should still reach the channel");
    assert_eq!(entry.signals.phase, None);
}

#[test]
fn percent_display_filename_field_populates_signals_download() {
    let filename = "Qwen3-0.6B-Q8_0.gguf";
    let rx = with_layer(|| {
        tracing::info!(
            filename = %filename,
            bytes_downloaded = 1024_u64,
            bytes_total = 4096_u64,
            "download progress"
        );
    });
    let entry = rx.try_recv().expect("event should reach the channel");
    let dl = entry
        .signals
        .download
        .as_ref()
        .expect("bytes_downloaded + bytes_total must produce a DownloadFields");
    assert_eq!(dl.filename.as_deref(), Some(filename));
    assert_eq!(dl.bytes_downloaded, 1024);
    assert_eq!(dl.bytes_total, 4096);
}

#[test]
fn download_complete_message_marks_finished_alongside_bytes() {
    // Mirrors download.rs's "download complete" event, which carries
    // bytes_downloaded + bytes_total *and* the finish marker. The
    // drain loop must be able to clear the bar even though both signals
    // are present.
    let rx = with_layer(|| {
        tracing::info!(
            bytes_downloaded = 4096_u64,
            bytes_total = 4096_u64,
            "download complete"
        );
    });
    let entry = rx.try_recv().unwrap();
    assert!(entry.signals.download_finished, "finish marker must be set");
    assert!(
        entry.signals.download.is_some(),
        "the same event also carries final progress fields",
    );
}

#[test]
fn already_cached_message_marks_finished_with_no_bytes() {
    let rx = with_layer(|| {
        tracing::info!("already cached");
    });
    let entry = rx.try_recv().unwrap();
    assert!(entry.signals.download_finished);
    assert!(entry.signals.download.is_none());
}

#[test]
fn unrelated_message_leaves_signals_empty() {
    let rx = with_layer(|| {
        tracing::info!("nothing structured here");
    });
    let entry = rx.try_recv().unwrap();
    assert!(entry.signals.phase.is_none());
    assert!(entry.signals.download.is_none());
    assert!(!entry.signals.download_finished);
    assert_eq!(entry.level, LogLevel::Info);
}

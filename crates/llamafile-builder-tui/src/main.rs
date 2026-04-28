use llamafile_builder_tui::{app, events, log_layer, ui};

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use llamafile_builder_core::config_manager::ConfigManager;
use llamafile_builder_core::config_store::ConfigStore;

use app::{build_from_state, App, BuildStatus, LogEntry, LogLevel};

/// Result sent back from the build task.
enum BuildResult {
    Complete { path: String, size: u64 },
    /// User-initiated cancellation. Distinct from `Failed` so the UI can show
    /// a notice instead of an error popup.
    Cancelled,
    Failed { error: String },
}

fn get_config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| {
            if cfg!(target_os = "windows") {
                dirs.data_local_dir().join("llamafile-builder")
            } else {
                dirs.home_dir().join(".llamafile-builder")
            }
        })
        .unwrap_or_else(|| PathBuf::from(".llamafile-builder"))
}

fn get_cache_dir() -> PathBuf {
    get_config_dir().join("cache")
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up tracing with our custom TUI layer
    let (log_tx, log_rx) = mpsc::channel::<LogEntry>();
    let tui_layer = log_layer::TuiLogLayer::new(log_tx);

    tracing_subscriber::registry()
        .with(tui_layer)
        .with(tracing_subscriber::filter::LevelFilter::DEBUG)
        .init();

    // Load config
    let config_dir = get_config_dir().join("config");
    let cm = ConfigManager::new(config_dir);
    let config = cm.load()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let store = ConfigStore::new(get_config_dir().join("recipes"));
    let mut app = App::new(config, get_cache_dir(), store);
    app.add_log(LogLevel::Info, "llamafile-builder TUI started".to_string());

    let result = run_app(&mut terminal, &mut app, log_rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    log_rx: mpsc::Receiver<LogEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Channel for receiving build results from the spawned task
    let mut build_result_rx: Option<tokio::sync::oneshot::Receiver<BuildResult>> = None;
    let cancel_flag = Arc::new(AtomicBool::new(false));

    loop {
        // Drain any pending log messages from the tracing layer.
        // BuildSignals on each entry are populated by TuiLogLayer from
        // structured tracing fields (`phase`, `bytes_downloaded`, …) — never
        // by parsing message text here.
        while let Ok(entry) = log_rx.try_recv() {
            // Download progress
            if let Some(ref dl) = entry.signals.download {
                let is_forward = app.download_info.as_ref()
                    .map(|prev| dl.bytes_downloaded >= prev.bytes_downloaded)
                    .unwrap_or(true);
                if is_forward {
                    let pct = if dl.bytes_total > 0 {
                        (dl.bytes_downloaded as f32 / dl.bytes_total as f32) * 100.0
                    } else {
                        0.0
                    };
                    app.download_info = Some(app::DownloadInfo {
                        filename: dl.filename.clone().unwrap_or_else(|| "file".to_string()),
                        bytes_downloaded: dl.bytes_downloaded,
                        bytes_total: dl.bytes_total,
                        percentage: pct,
                    });
                }
            } else if entry.signals.download_finished {
                app.download_info = None;
            }

            // Build phase
            if let Some(phase) = entry.signals.phase {
                if build_result_rx.is_some() {
                    let (label, progress) = phase_display(phase);
                    app.build_status = BuildStatus::Building {
                        phase: label.to_string(),
                        progress,
                    };
                }
            }

            app.logs.push(entry);
        }

        // Check if a build task finished
        if let Some(ref mut rx) = build_result_rx {
            match rx.try_recv() {
                Ok(BuildResult::Complete { path, size }) => {
                    app.build_status = BuildStatus::Complete { path: path.clone(), size };
                    app.add_log(LogLevel::Info, format!(
                        "Build complete: {} ({})",
                        path,
                        llamafile_builder_core::models::format_size(size)
                    ));
                    build_result_rx = None;
                }
                Ok(BuildResult::Cancelled) => {
                    app.build_status = BuildStatus::Failed {
                        error: "Cancelled".to_string(),
                    };
                    app.add_log(LogLevel::Warn, "Build cancelled".to_string());
                    build_result_rx = None;
                }
                Ok(BuildResult::Failed { error }) => {
                    app.build_status = BuildStatus::Failed { error: error.clone() };
                    app.add_log(LogLevel::Error, format!("Build failed: {}", error));
                    if matches!(app.popup, app::Popup::None) {
                        app.popup = app::Popup::BuildError { message: error.clone() };
                    }
                    build_result_rx = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Build still running -- state is updated incrementally
                    // from the log drain above, nothing to do here
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    // Task panicked or was dropped
                    let msg = "Build task terminated unexpectedly".to_string();
                    app.build_status = BuildStatus::Failed { error: msg.clone() };
                    app.add_log(LogLevel::Error, msg.clone());
                    if matches!(app.popup, app::Popup::None) {
                        app.popup = app::Popup::BuildError { message: msg };
                    }
                    build_result_rx = None;
                }
            }
        }

        terminal.draw(|frame| ui::draw(frame, app))?;

        // Check if a build needs to be started
        if build_result_rx.is_none()
            && matches!(app.build_status, BuildStatus::Building { ref phase, .. } if phase == "starting")
        {
            app.add_log(LogLevel::Info, "Build starting...".to_string());
            cancel_flag.store(false, Ordering::Relaxed);

            match app.make_builder() {
                Ok(_) => {
                    app.build_status = BuildStatus::Building {
                        phase: "initializing".to_string(),
                        progress: 0.0,
                    };

                    let (tx, rx) = tokio::sync::oneshot::channel();
                    build_result_rx = Some(rx);
                    cancel_flag.store(false, Ordering::Relaxed);

                    // Snapshot the user's selections; the build task rebuilds
                    // a Builder from this on a Send-compatible thread.
                    let prepared = app.prepare_build();
                    let config = app.config.clone();
                    let cache_dir = app.cache_dir.clone();
                    let shared_cancel = cancel_flag.clone();

                    tokio::spawn(async move {
                        let mut builder = build_from_state(config, cache_dir, &prepared);
                        builder.set_cancel_flag(shared_cancel);

                        match builder.build().await {
                            Ok(path) => {
                                let size = tokio::fs::metadata(&path)
                                    .await
                                    .map(|m| m.len())
                                    .unwrap_or(0);
                                let _ = tx.send(BuildResult::Complete {
                                    path: path.to_string_lossy().to_string(),
                                    size,
                                });
                            }
                            Err(e) if is_user_cancellation(&e) => {
                                let _ = tx.send(BuildResult::Cancelled);
                            }
                            Err(e) => {
                                let _ = tx.send(BuildResult::Failed {
                                    error: format!("{}", e),
                                });
                            }
                        }
                    });
                }
                Err(e) => {
                    app.build_status = BuildStatus::Failed { error: e.clone() };
                    app.add_log(LogLevel::Error, format!("Build failed: {}", e));
                }
            }
        }

        // Poll for events with 50ms timeout (faster refresh during builds)
        let poll_time = if build_result_rx.is_some() {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_time)? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release/repeat).
                // On Windows, crossterm reports both Press and Release for each keypress.
                if key.kind == event::KeyEventKind::Press {
                    events::handle_key(app, key);
                }
            }
        }

        if app.should_quit {
            if build_result_rx.is_some() {
                cancel_flag.store(true, Ordering::Relaxed);
                app.add_log(LogLevel::Warn, "Cancelling build and exiting...".to_string());
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            break;
        }
    }

    Ok(())
}

/// True when the error chain represents a user-initiated cancellation —
/// either `Error::Cancelled` (the build's own check) or
/// `Error::Download(DownloadError::Cancelled)` (raised mid-download by
/// `Cache::ensure`). User cancellations should surface as a status notice,
/// not an error popup.
fn is_user_cancellation(error: &llamafile_builder_core::Error) -> bool {
    use llamafile_builder_core::{DownloadError, Error};
    matches!(
        error,
        Error::Cancelled | Error::Download(DownloadError::Cancelled)
    )
}

/// Map a `BuildPhase` to its UI label and the progress fraction the dashboard
/// shows. Progress fractions are TUI-only — the core enum carries no
/// presentational meaning. Each phase fires exactly once, so progress is
/// monotonic.
fn phase_display(phase: llamafile_builder_core::BuildPhase) -> (&'static str, f32) {
    use llamafile_builder_core::BuildPhase;
    match phase {
        BuildPhase::PreparingZipalign => ("preparing zipalign", 0.05),
        BuildPhase::PreparingRuntime => ("preparing runtime", 0.1),
        BuildPhase::PreparingModel => ("downloading", 0.3),
        BuildPhase::WritingArgs => ("writing config", 0.7),
        BuildPhase::PreparingGpuLibs => ("preparing GPU libs", 0.8),
        BuildPhase::Assembling => ("assembling", 0.9),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use llamafile_builder_core::{DownloadError, Error};

    #[test]
    fn error_cancelled_is_user_cancellation() {
        assert!(is_user_cancellation(&Error::Cancelled));
    }

    #[test]
    fn download_cancelled_through_wrapper_is_user_cancellation() {
        // Reproduces the case the string matcher used to miss: a download
        // cancelled mid-flight comes back wrapped in Error::Download, whose
        // Display renders as "Download error: Download cancelled".
        let err = Error::Download(DownloadError::Cancelled);
        assert!(is_user_cancellation(&err));
        // Sanity: the formatted string is what we'd previously have had to
        // pattern-match.
        assert_eq!(format!("{}", err), "Download error: Download cancelled");
    }

    #[test]
    fn other_download_errors_are_not_user_cancellation() {
        let mismatch = Error::Download(DownloadError::SizeMismatch {
            expected: 100,
            actual: 50,
        });
        assert!(!is_user_cancellation(&mismatch));
    }

    #[test]
    fn other_top_level_errors_are_not_user_cancellation() {
        assert!(!is_user_cancellation(&Error::Build("zipalign failed".into())));
        assert!(!is_user_cancellation(&Error::NotFound("model".into())));
        assert!(!is_user_cancellation(&Error::Validation("ctx too long".into())));
    }
}

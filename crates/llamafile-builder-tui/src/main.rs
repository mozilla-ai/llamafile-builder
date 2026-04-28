mod app;
mod config_ui;
mod events;
mod log_layer;
mod ui;

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
        // Drain any pending log messages from the tracing layer
        // Also update download/build state from each new entry as it arrives
        while let Ok(entry) = log_rx.try_recv() {
            let msg = &entry.message;

            // Track download state
            if msg.contains("download started") {
                let filename = extract_str_field(msg, "filename=")
                    .unwrap_or_else(|| "file".to_string());
                let total = extract_field(msg, "bytes_total=").unwrap_or(0);
                app.download_info = Some(app::DownloadInfo {
                    filename,
                    bytes_downloaded: 0,
                    bytes_total: total,
                    percentage: 0.0,
                });
            } else if msg.contains("bytes_downloaded=") && msg.contains("bytes_total=") {
                // Our own downloader (reliable single-stream progress)
                if let (Some(dl), Some(total)) = (
                    extract_field(msg, "bytes_downloaded="),
                    extract_field(msg, "bytes_total="),
                ) {
                    let is_forward = app.download_info.as_ref()
                        .map(|prev| dl >= prev.bytes_downloaded)
                        .unwrap_or(true);
                    if is_forward {
                        let filename = extract_str_field(msg, "filename=")
                            .unwrap_or_else(|| "file".to_string());
                        let pct = if total > 0 { (dl as f32 / total as f32) * 100.0 } else { 0.0 };
                        app.download_info = Some(app::DownloadInfo {
                            filename,
                            bytes_downloaded: dl,
                            bytes_total: total,
                            percentage: pct,
                        });
                    }
                }
            } else if msg.contains("download complete") || msg.contains("already cached")
                || msg.contains("already in HF cache")
            {
                app.download_info = None;
            }

            // Update build phase from INFO events
            if matches!(entry.level, app::LogLevel::Info) && build_result_rx.is_some() {
                let new_phase = if msg.contains("preparing zipalign") {
                    Some(("preparing zipalign", 0.05))
                } else if msg.contains("preparing runtime") {
                    Some(("preparing runtime", 0.1))
                } else if msg.contains("download started") || msg.contains("downloading from") || msg.contains("registry file") {
                    Some(("downloading", 0.3))
                } else if msg.contains("writing .args") {
                    Some(("writing config", 0.7))
                } else if msg.contains("GPU lib") {
                    Some(("preparing GPU libs", 0.8))
                } else if msg.contains("bundling") || msg.contains("zipalign -j0") || msg.contains("copying runtime") {
                    Some(("assembling", 0.9))
                } else {
                    None
                };
                if let Some((phase, progress)) = new_phase {
                    app.build_status = BuildStatus::Building {
                        phase: phase.to_string(),
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

/// Extract a u64 field value from a log message like "bytes_downloaded=12345"
fn extract_field(msg: &str, key: &str) -> Option<u64> {
    let start = msg.find(key)? + key.len();
    let rest = &msg[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Extract a string field value from a log message like "filename=model.gguf"
fn extract_str_field(msg: &str, key: &str) -> Option<String> {
    let start = msg.find(key)? + key.len();
    let rest = &msg[start..];
    if rest.starts_with('"') {
        let end = rest[1..].find('"').map(|i| i + 1)?;
        Some(rest[1..end].to_string())
    } else {
        let end = rest.find(|c: char| c == ' ' || c == ',').unwrap_or(rest.len());
        Some(rest[..end].to_string())
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

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

use llamafile_core::config_manager::ConfigManager;
use llamafile_core::config_store::ConfigStore;

use app::{App, BuildStatus, LogEntry, LogLevel};

/// Result sent back from the build task.
enum BuildResult {
    Complete { path: String, size: u64 },
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

    let store = ConfigStore::new(get_config_dir().join("configs"));
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
                        llamafile_core::models::format_size(size)
                    ));
                    build_result_rx = None;
                }
                Ok(BuildResult::Failed { error }) => {
                    app.build_status = BuildStatus::Failed { error: error.clone() };
                    app.add_log(LogLevel::Error, format!("Build failed: {}", error));
                    build_result_rx = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    // Build still running -- state is updated incrementally
                    // from the log drain above, nothing to do here
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    // Task panicked or was dropped
                    app.build_status = BuildStatus::Failed {
                        error: "Build task terminated unexpectedly".to_string(),
                    };
                    app.add_log(LogLevel::Error, "Build task terminated unexpectedly".to_string());
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

                    // Capture everything needed to reconstruct the builder in the task
                    let shared_cancel = cancel_flag.clone();
                    let config = app.config.clone();
                    let cache_dir = app.cache_dir.clone();
                    let version_group = app.version_group.clone();
                    let version = app.version.clone();
                    let custom_runtime = app.custom_runtime.clone();
                    let model_id = app.model_id.clone();
                    let custom_model_file = app.custom_model_file.clone();
                    let ollama_model = app.ollama_model.clone();
                    let lmstudio_model = app.lmstudio_model.clone();
                    let quantizations = app.quantizations.clone();
                    let config_values = app.config_values.clone();
                    let output_path = app.output_path.clone();
                    let gpu_libs = app.gpu_libs.clone();
                    let custom_zipalign = app.custom_zipalign.clone();
                    let args_file = app.args_file.clone();
                    let args_override = app.args_override.clone();
                    let mmproj_disabled = app.mmproj_disabled;

                    tokio::spawn(async move {
                        // Reconstruct builder inside the task (on a Send-compatible thread)
                        let cache = llamafile_core::cache::Cache::new(cache_dir);
                        let mut builder = llamafile_core::Builder::new(config, cache);
                        builder.set_cancel_flag(shared_cancel);

                        // Apply state
                        if let Some(ref rt) = custom_runtime {
                            builder.use_custom_runtime(std::path::Path::new(rt));
                        } else if let (Some(ref g), Some(ref v)) = (&version_group, &version) {
                            builder.select_version(g, v);
                        } else {
                            builder.select_default_version();
                        }

                        if let Some(ref path) = custom_model_file {
                            builder.select_custom_model(std::path::Path::new(path));
                        } else if let Some(ref model) = ollama_model {
                            builder.select_ollama_model(model.clone());
                        } else if let Some(ref model) = lmstudio_model {
                            builder.select_lmstudio_model(model.clone());
                        } else if let Some(ref id) = model_id {
                            builder.select_registry_model(id);
                        } else {
                            builder.select_default_model();
                        }

                        for (role, name) in &quantizations {
                            if let Some(mid) = builder.state().selected_model_id.clone() {
                                if let Some(model) = builder.config().models.iter().find(|m| m.id == mid) {
                                    if let Some(file) = model.files.iter().find(|f| f.role == *role) {
                                        if let Some(idx) = file.quantizations.iter().position(|q| q.name.eq_ignore_ascii_case(name)) {
                                            builder.select_quantization(role, idx);
                                        }
                                    }
                                }
                            }
                        }

                        for (key, value) in &config_values {
                            builder.set_config_value(key, value.clone());
                        }

                        if let Some(ref path) = output_path {
                            builder.set_output_path(std::path::Path::new(path));
                        }

                        for &(accel, os) in &gpu_libs {
                            builder.add_gpu_lib(accel, os);
                        }

                        if let Some(ref path) = custom_zipalign {
                            builder.use_custom_zipalign(std::path::Path::new(path));
                        }

                        if let Some(ref content) = args_override {
                            builder.set_args_override(content);
                        } else if let Some(ref path) = args_file {
                            if let Ok(content) = std::fs::read_to_string(path) {
                                builder.set_args_override(&content);
                            }
                        }

                        if mmproj_disabled {
                            builder.disable_file_role("mmproj");
                        }

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

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
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use llamafile_core::config_manager::ConfigManager;
use llamafile_core::models::{GpuAccel, Os};

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

pub fn parse_gpu_spec(spec: &str) -> Result<(GpuAccel, Vec<Os>), String> {
    let parts: Vec<&str> = spec.split(':').collect();
    let accel = match parts[0].to_lowercase().as_str() {
        "cuda" => GpuAccel::Cuda,
        "rocm" => GpuAccel::Rocm,
        "vulkan" => GpuAccel::Vulkan,
        other => return Err(format!("Unknown GPU type: '{}'", other)),
    };
    let os_list = if parts.len() > 1 {
        match parts[1].to_lowercase().as_str() {
            "linux" => vec![Os::Linux],
            "windows" => vec![Os::Windows],
            "all" => vec![Os::Linux, Os::Windows],
            other => return Err(format!("Unknown OS: '{}'", other)),
        }
    } else if cfg!(target_os = "linux") {
        vec![Os::Linux]
    } else if cfg!(target_os = "windows") {
        vec![Os::Windows]
    } else {
        vec![Os::Linux]
    };
    Ok((accel, os_list))
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
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config, get_cache_dir());
    app.add_log(LogLevel::Info, "llamafile-builder TUI started".to_string());

    let result = run_app(&mut terminal, &mut app, log_rx).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
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
        while let Ok(entry) = log_rx.try_recv() {
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
                    // Still running -- update progress from logs
                    // Look at the latest log to guess the phase
                    if let Some(last) = app.logs.last() {
                        let msg = &last.message;
                        let phase = if msg.contains("downloading") {
                            "downloading"
                        } else if msg.contains("ensure_zipalign") || msg.contains("zipalign") {
                            "preparing zipalign"
                        } else if msg.contains("ensure_runtime") || msg.contains("runtime") {
                            "preparing runtime"
                        } else if msg.contains("ensure_model") || msg.contains("registry file") {
                            "downloading model"
                        } else if msg.contains("write_config") || msg.contains(".args") {
                            "writing config"
                        } else if msg.contains("ensure_gpu") {
                            "preparing GPU libs"
                        } else if msg.contains("create_executable") || msg.contains("bundling") {
                            "assembling"
                        } else {
                            "building"
                        };

                        // Extract percentage from download progress if available
                        let progress = if let Some(pct_pos) = msg.find("percentage=") {
                            let rest = &msg[pct_pos + 11..];
                            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
                            rest[..end].parse::<f32>().unwrap_or(0.0) / 100.0
                        } else {
                            // Rough phase-based progress
                            match phase {
                                "preparing zipalign" => 0.05,
                                "preparing runtime" => 0.1,
                                "downloading model" => 0.3,
                                "writing config" => 0.7,
                                "preparing GPU libs" => 0.8,
                                "assembling" => 0.9,
                                _ => 0.5,
                            }
                        };

                        app.build_status = BuildStatus::Building {
                            phase: phase.to_string(),
                            progress,
                        };
                    }
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

                    // Capture everything needed to reconstruct the builder in the task
                    let config = app.config.clone();
                    let cache_dir = app.cache_dir.clone();
                    let version_group = app.version_group.clone();
                    let version = app.version.clone();
                    let custom_runtime = app.custom_runtime.clone();
                    let model_id = app.model_id.clone();
                    let custom_model_file = app.custom_model_file.clone();
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

                        for spec in &gpu_libs {
                            if let Ok((accel, os_list)) = crate::parse_gpu_spec(spec) {
                                for os in os_list {
                                    builder.add_gpu_lib(accel, os);
                                }
                            }
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
                events::handle_key(app, key);
            }
        }

        // Handle cancellation during build
        if app.should_quit && build_result_rx.is_some() {
            cancel_flag.store(true, Ordering::Relaxed);
            app.add_log(LogLevel::Warn, "Cancelling build...".to_string());
            // Don't quit yet -- wait for the task to finish
            app.should_quit = false;
        }

        if app.should_quit && build_result_rx.is_none() {
            break;
        }
    }

    Ok(())
}

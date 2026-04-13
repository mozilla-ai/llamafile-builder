mod app;
mod events;
mod log_layer;
mod ui;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc;
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
    loop {
        // Drain any pending log messages from the tracing layer
        while let Ok(entry) = log_rx.try_recv() {
            app.logs.push(entry);
        }

        terminal.draw(|frame| ui::draw(frame, app))?;

        // Check if a build needs to be started
        if matches!(app.build_status, BuildStatus::Building { ref phase, .. } if phase == "starting")
        {
            app.add_log(LogLevel::Info, "Build starting...".to_string());

            match app.make_builder() {
                Ok(builder) => {
                    app.build_status = BuildStatus::Building {
                        phase: "downloading".to_string(),
                        progress: 0.1,
                    };
                    while let Ok(entry) = log_rx.try_recv() {
                        app.logs.push(entry);
                    }
                    terminal.draw(|frame| ui::draw(frame, app))?;

                    match builder.build().await {
                        Ok(path) => {
                            while let Ok(entry) = log_rx.try_recv() {
                                app.logs.push(entry);
                            }
                            let size = tokio::fs::metadata(&path)
                                .await
                                .map(|m| m.len())
                                .unwrap_or(0);
                            app.build_status = BuildStatus::Complete {
                                path: path.to_string_lossy().to_string(),
                                size,
                            };
                            app.add_log(LogLevel::Info, format!(
                                "Build complete: {} ({})",
                                path.display(),
                                llamafile_core::models::format_size(size)
                            ));
                        }
                        Err(e) => {
                            while let Ok(entry) = log_rx.try_recv() {
                                app.logs.push(entry);
                            }
                            let msg = format!("{}", e);
                            app.build_status = BuildStatus::Failed {
                                error: msg.clone(),
                            };
                            app.add_log(LogLevel::Error, format!("Build failed: {}", msg));
                        }
                    }
                }
                Err(e) => {
                    app.build_status = BuildStatus::Failed {
                        error: e.clone(),
                    };
                    app.add_log(LogLevel::Error, format!("Build failed: {}", e));
                }
            }
            continue;
        }

        // Poll for events with 100ms timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                events::handle_key(app, key);
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

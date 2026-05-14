mod cli;
mod commands;
mod interactive;
mod log_forwarder;
mod server;
mod state_file;

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

use llamafile_builder_core::cache::Cache;
use llamafile_builder_core::config_manager::ConfigManager;

use cli::{Cli, Commands, LmStudioCmd, ModelsCmd, OllamaCmd};

/// Map error strings to exit codes per spec:
/// 0 = success, 1 = general, 2 = invalid args, 3 = download failed,
/// 4 = build failed, 5 = cancelled
fn exit_code_for_error(error: &str) -> ExitCode {
    let e = error.to_lowercase();
    if e.contains("cancelled") || e.contains("canceled") {
        ExitCode::from(5)
    } else if e.contains("insufficient disk space")
        || e.contains("download")
        || e.contains("http")
        || e.contains("auth")
    {
        // Disk-space failures happen during pre-flight (before any download
        // bytes are written) but logically belong to the download step.
        ExitCode::from(3)
    } else if e.contains("not found") || e.contains("invalid") {
        ExitCode::from(2)
    } else {
        ExitCode::from(4) // build failed
    }
}

pub fn get_recipes_dir() -> PathBuf {
    get_config_dir().join("recipes")
}

pub fn get_config_dir() -> PathBuf {
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

pub fn get_cache_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| {
            if cfg!(target_os = "windows") {
                dirs.data_local_dir()
                    .join("llamafile-builder")
                    .join("cache")
            } else {
                dirs.home_dir().join(".llamafile-builder").join("cache")
            }
        })
        .unwrap_or_else(|| PathBuf::from(".llamafile-builder/cache"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    // TUI installs its own tracing subscriber (with the in-UI log layer),
    // so it has to run before we initialize ours.
    if cli.tui {
        return match llamafile_builder_tui::run(get_config_dir(), get_cache_dir()).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("Error: {e}");
                ExitCode::from(1)
            }
        };
    }

    // Configure tracing subscriber.
    // Default: INFO level. Use RUST_LOG=debug for more detail.
    // The BroadcastLogLayer is a no-op until server::run() calls log_forwarder::init().
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr)
                .with_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                ),
        )
        .with(log_forwarder::BroadcastLogLayer)
        .init();

    // Load configuration
    let config_dir = get_config_dir().join("config");
    let mut cm = ConfigManager::new(config_dir).with_update_mode(cli.parse_update_mode());
    if let Some(ref url) = cli.config_url {
        cm = cm.with_remote_url(url);
    }

    let mut config = match cm.load() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load configuration: {e}");
            return ExitCode::from(1);
        }
    };

    // Check for config updates (NFR-9, NFR-10)
    match cm.update_mode() {
        llamafile_builder_core::UpdateMode::Always => {
            tracing::info!("checking for config updates");
            match cm.check_for_updates().await {
                Ok(llamafile_builder_core::UpdateStatus::ConfigUpdateAvailable(update)) => {
                    tracing::info!("config update available, applying");
                    if let Err(e) = cm.apply_update(&update) {
                        tracing::warn!("failed to apply config update: {e}");
                    } else {
                        // Reload with updated configs
                        match cm.load() {
                            Ok(c) => config = c,
                            Err(e) => tracing::warn!("failed to reload config after update: {e}"),
                        }
                    }
                }
                Ok(llamafile_builder_core::UpdateStatus::BuilderUpdateRequired { min_version }) => {
                    tracing::warn!(
                        min_version,
                        "remote config requires a newer builder version"
                    );
                }
                Ok(llamafile_builder_core::UpdateStatus::UpToDate) => {
                    tracing::debug!("config is up to date");
                }
                Err(e) => {
                    tracing::debug!("config update check failed: {e}");
                }
            }
        }
        llamafile_builder_core::UpdateMode::Ask => {
            // In CLI mode, Ask behaves like Always with a log message.
            // A graphical frontend would show a dialog instead.
            tracing::info!("checking for config updates (use --update-mode=never to skip)");
            match cm.check_for_updates().await {
                Ok(llamafile_builder_core::UpdateStatus::ConfigUpdateAvailable(update)) => {
                    tracing::info!("config update available, applying");
                    if let Err(e) = cm.apply_update(&update) {
                        tracing::warn!("failed to apply config update: {e}");
                    } else {
                        match cm.load() {
                            Ok(c) => config = c,
                            Err(e) => tracing::warn!("failed to reload config after update: {e}"),
                        }
                    }
                }
                Ok(llamafile_builder_core::UpdateStatus::BuilderUpdateRequired { min_version }) => {
                    tracing::warn!(
                        min_version,
                        "remote config requires a newer builder version"
                    );
                }
                Ok(llamafile_builder_core::UpdateStatus::UpToDate) => {
                    tracing::debug!("config is up to date");
                }
                Err(e) => {
                    tracing::debug!("config update check failed: {e}");
                }
            }
        }
        llamafile_builder_core::UpdateMode::Never => {}
    }

    if cli.serve {
        match server::run(config, cli.port, cli.web_dir, cli.no_open).await {
            Ok(()) => return ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!("{e}");
                return ExitCode::from(1);
            }
        }
    }

    if cli.interactive {
        match interactive::run(config).await {
            Ok(()) => return ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!("{e}");
                return ExitCode::from(1);
            }
        }
    }

    let cache = Cache::new(get_cache_dir());

    match cli.command {
        Some(Commands::Versions) => {
            commands::versions::run(&config, cli.json);
            ExitCode::SUCCESS
        }
        Some(Commands::Models { cmd, search, tag }) => {
            match cmd {
                Some(ModelsCmd::Show { id }) => {
                    commands::models::show(&config, &id, cli.json);
                }
                None => {
                    commands::models::list(
                        &config,
                        search.as_deref(),
                        tag.as_deref(),
                        cli.json,
                    );
                }
            }
            ExitCode::SUCCESS
        }
        Some(Commands::Ollama { cmd }) => {
            match cmd {
                OllamaCmd::List { dir } => {
                    commands::scan::ollama_list(dir.as_deref(), cli.json);
                }
            }
            ExitCode::SUCCESS
        }
        Some(Commands::LmStudio { cmd }) => {
            match cmd {
                LmStudioCmd::List { dir } => {
                    commands::scan::lmstudio_list(dir.as_deref(), cli.json);
                }
            }
            ExitCode::SUCCESS
        }
        Some(Commands::SystemInfo) => {
            let info = llamafile_builder_core::system_info::system_info();
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&info).unwrap());
            } else {
                println!("OS:         {} {}", info.os, info.arch);
                println!("CPU cores:  {}", info.cpu_cores);
                println!("Memory:     {:.1} GB", info.memory_gb);
                println!(
                    "NVIDIA GPU: {}",
                    if info.has_nvidia_gpu { "yes" } else { "no" }
                );
                println!(
                    "AMD GPU:    {}",
                    if info.has_amd_gpu { "yes" } else { "no" }
                );
                println!(
                    "Metal:      {}",
                    if info.has_metal { "yes" } else { "no" }
                );
            }
            ExitCode::SUCCESS
        }
        Some(Commands::Recipe { ref cmd }) => {
            let store = llamafile_builder_core::ConfigStore::new(get_recipes_dir());
            let config_dir = get_config_dir();
            match commands::recipe::run(cmd, &config, &store, &config_dir, cli.json) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::from(1)
                }
            }
        }
        Some(Commands::Build(ref args)) => {
            match commands::build::run(args, config, cache).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    exit_code_for_error(&e)
                }
            }
        }
        None => {
            use clap::CommandFactory;
            Cli::command().print_help().ok();
            println!();
            ExitCode::SUCCESS
        }
    }
}

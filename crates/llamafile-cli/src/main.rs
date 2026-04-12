mod cli;
mod commands;

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

use llamafile_core::cache::Cache;
use llamafile_core::config_manager::ConfigManager;

use cli::{Cli, Commands, LmStudioCmd, ModelsCmd, OllamaCmd};

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

    // Configure tracing subscriber.
    // Default: WARN level. Use RUST_LOG=info or RUST_LOG=debug for more detail.
    // The build command will produce INFO-level logs for each phase.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    // Load configuration
    let config_dir = get_config_dir().join("config");
    let mut cm = ConfigManager::new(config_dir).with_update_mode(cli.parse_update_mode());
    if let Some(ref url) = cli.config_url {
        cm = cm.with_remote_url(url);
    }

    let config = match cm.load() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load configuration: {e}");
            return ExitCode::from(1);
        }
    };

    let cache = Cache::new(get_cache_dir());

    // Dispatch
    if cli.serve {
        tracing::error!("Server mode not yet implemented (Phase 6).");
        return ExitCode::from(1);
    }

    if cli.interactive {
        tracing::error!("Interactive mode not yet implemented (Phase 6).");
        return ExitCode::from(1);
    }

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
            let info = llamafile_core::system_info::system_info();
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
        Some(Commands::Build(ref args)) => {
            match commands::build::run(args, config, cache).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    tracing::error!("{e}");
                    ExitCode::from(4)
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

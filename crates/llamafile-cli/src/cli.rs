use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "llamafile-builder",
    about = "Build custom llamafiles with bundled models and configurations",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Start HTTP server mode
    #[arg(long)]
    pub serve: bool,

    /// Port for server mode
    #[arg(long, default_value = "8765")]
    pub port: u16,

    /// Start JSON-RPC interactive mode over stdio
    #[arg(long)]
    pub interactive: bool,

    /// Override config URL for fetching updates
    #[arg(long, env = "LLAMAFILE_CONFIG_URL")]
    pub config_url: Option<String>,

    /// Config update mode: always, never, ask
    #[arg(long, default_value = "never")]
    pub update_mode: String,

    /// Output as JSON (for machine consumption)
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List available llamafile versions
    Versions,

    /// Search and list models
    Models {
        #[command(subcommand)]
        cmd: Option<ModelsCmd>,

        /// Search query (filters by name/organization)
        #[arg(long)]
        search: Option<String>,

        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },

    /// Scan local Ollama models
    Ollama {
        #[command(subcommand)]
        cmd: OllamaCmd,
    },

    /// Scan local LM Studio models
    LmStudio {
        #[command(subcommand)]
        cmd: LmStudioCmd,
    },

    /// Show system information
    SystemInfo,

    /// Build a llamafile
    Build(BuildArgs),
}

#[derive(Subcommand)]
pub enum ModelsCmd {
    /// Show details for a specific model
    Show {
        /// Model ID
        id: String,
    },
}

#[derive(Subcommand)]
pub enum OllamaCmd {
    /// List discovered Ollama models
    List {
        /// Custom Ollama directory
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum LmStudioCmd {
    /// List discovered LM Studio models
    List {
        /// Custom LM Studio directory
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Args)]
pub struct BuildArgs {
    /// Model ID from registry, or path to a local .gguf file
    #[arg(long)]
    pub model: String,

    /// Quantization variant name (e.g. Q4_K_M, Q8_0)
    #[arg(long)]
    pub quantization: Option<String>,

    /// Output file path
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// GPU acceleration to include (repeatable: cuda, rocm, vulkan)
    #[arg(long)]
    pub gpu: Vec<String>,

    /// Custom .args file to use instead of generating one
    #[arg(long)]
    pub args_file: Option<PathBuf>,

    /// Context length override
    #[arg(long)]
    pub context_length: Option<u32>,

    /// llamafile version to use (e.g. "0.10.0")
    #[arg(long)]
    pub version: Option<String>,

    /// Custom llamafile runtime binary
    #[arg(long)]
    pub runtime: Option<PathBuf>,

    /// Custom zipalign binary
    #[arg(long)]
    pub zipalign: Option<PathBuf>,

    /// Skip existing cached files without prompting
    #[arg(long)]
    pub skip_existing: bool,
}

impl Cli {
    pub fn parse_update_mode(&self) -> llamafile_core::UpdateMode {
        match self.update_mode.as_str() {
            "always" => llamafile_core::UpdateMode::Always,
            "never" => llamafile_core::UpdateMode::Never,
            _ => llamafile_core::UpdateMode::Ask,
        }
    }
}

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

    /// Start JSON-RPC interactive mode over stdio
    #[arg(long)]
    pub interactive: bool,

    /// Start HTTP server mode
    #[arg(long)]
    pub serve: bool,

    /// Port for HTTP server (default: 8765)
    #[arg(long, default_value = "8765")]
    pub port: u16,

    /// Directory to serve web UI static files from
    #[arg(long)]
    pub web_dir: Option<std::path::PathBuf>,

    /// Don't auto-open browser when starting server
    #[arg(long)]
    pub no_open: bool,

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

    /// Configure build parameters incrementally
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },

    /// Build a llamafile
    Build(BuildArgs),
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Select llamafile version
    SetVersion {
        /// Version group name (e.g. "llamafile 0.10")
        group: String,
        /// Version string (e.g. "0.10.0")
        version: String,
    },
    /// Select a model from the registry
    SetModel {
        /// Model ID / HuggingFace repo ID (e.g. "Qwen/Qwen3-0.6B-GGUF")
        id: String,
    },
    /// Use a custom local .gguf file
    SetModelFile {
        /// Path to .gguf file
        path: PathBuf,
    },
    /// Select a local Ollama model (e.g. "llama3:8b" or "qwen3:0.6b")
    SetOllamaModel {
        /// Ollama model name with tag (e.g. "llama3:8b")
        name: String,
    },
    /// Select a local LM Studio model (e.g. "Qwen3.5-0.8B-GGUF")
    SetLmstudioModel {
        /// LM Studio model directory name
        name: String,
    },
    /// Select quantization for a file role
    SetQuantization {
        /// File role (e.g. "weights", "mmproj")
        role: String,
        /// Quantization name (e.g. "Q4_K_M", "Q8_0")
        name: String,
    },
    /// Set output file path
    SetOutput {
        /// Output .llamafile path
        path: PathBuf,
    },
    /// Set a config value (e.g. context_length, temperature, server_enabled)
    Set {
        /// Config key
        key: String,
        /// Value (JSON or plain string)
        value: String,
    },
    /// Override .args with a custom file
    SetArgsFile {
        /// Path to custom .args file
        path: PathBuf,
    },
    /// Add a GPU library (e.g. "cuda", "cuda:linux", "rocm:windows")
    AddGpu {
        /// GPU spec: accel or accel:os
        spec: String,
    },
    /// Remove a GPU library
    RemoveGpu {
        /// GPU spec to remove
        spec: String,
    },
    /// Use a custom llamafile runtime binary
    SetRuntime {
        /// Path to llamafile binary
        path: PathBuf,
    },
    /// Use a custom zipalign binary
    SetZipalign {
        /// Path to zipalign binary
        path: PathBuf,
    },
    /// Show current build configuration
    Show,
    /// Preview the .args file content
    ShowArgs,
    /// Reset current recipe state
    Reset,
    /// Create a new build recipe
    New,
    /// List all saved recipes
    List,
    /// Load a saved recipe as current (by name or UUID)
    Load {
        /// Recipe name or UUID
        name_or_id: String,
    },
    /// Name/save the current recipe for later use
    Save {
        /// Name to assign
        name: String,
    },
    /// Delete a saved recipe (by name or UUID)
    Delete {
        /// Recipe name or UUID
        name_or_id: String,
    },
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
    /// Model ID from registry, or path to a local .gguf file.
    /// If omitted, builds from saved config (see 'config' subcommand).
    #[arg(long)]
    pub model: Option<String>,

    /// Quantization for weights (e.g. Q4_K_M, Q8_0)
    #[arg(long)]
    pub quantization: Option<String>,

    /// Quantization for mmproj / vision adapter (e.g. F16, Q8_0)
    #[arg(long)]
    pub mmproj_quantization: Option<String>,

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

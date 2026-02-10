use serde::{Deserialize, Serialize};

/// Individual llamafile release within a version group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamafileRelease {
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub release_date: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Grouped llamafile version (e.g. "llamafile 0.9" containing multiple releases)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlamafileVersionGroup {
    pub name: String,
    pub tag: String, // "latest", "stable", "legacy"
    #[serde(default)]
    pub description: Option<String>,
    pub releases: Vec<LlamafileRelease>,
}

/// Quantization option for a model file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFileQuantization {
    pub name: String,
    pub size_bytes: u64,
    pub download_url: String,
    pub sha256: Option<String>,
}

/// A downloadable file within a model (e.g. weights, mmproj adapter)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelFile {
    pub role: String,
    pub label: Option<String>,
    pub quantizations: Vec<ModelFileQuantization>,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub organization: String,
    pub parameters: String,
    pub param_count: u64,
    pub description: String,
    pub license: String,
    pub huggingface_url: Option<String>,
    pub tags: Vec<String>,
    pub context_lengths: Vec<u32>,
    pub default_context: u32,
    pub files: Vec<ModelFile>,
}

/// Config schema item for data-driven configuration UI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigSchemaItem {
    Section {
        label: String,
    },
    Text {
        key: String,
        label: String,
        description: String,
        default: serde_json::Value,
        value_type: String,
        flag: Option<String>,
    },
    Dropdown {
        key: String,
        label: String,
        description: String,
        default: serde_json::Value,
        options: Vec<serde_json::Value>,
        flag: Option<String>,
    },
    Bool {
        key: String,
        label: String,
        description: String,
        default: bool,
        flag: Option<String>,
    },
}

/// Server configuration for .args file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub context_length: u32,
    pub threads: Option<u32>,   // None = auto
    pub batch_size: u32,
    pub api_key: Option<String>,
    pub enable_web_ui: bool,
    pub flash_attention: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            context_length: 4096,
            threads: None,
            batch_size: 512,
            api_key: None,
            enable_web_ui: true,
            flash_attention: true,
        }
    }
}

/// Compute target configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeConfig {
    pub mode: ComputeMode,
    pub cuda: bool,
    pub rocm: bool,
    pub metal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ComputeMode {
    Cpu,
    Gpu,
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            mode: ComputeMode::Gpu,
            cuda: true,
            rocm: true,
            metal: true,
        }
    }
}

/// Agent tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub enabled: bool,
}

/// Agent configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub enabled: bool,
    pub tools: Vec<AgentTool>,
    pub system_prompt: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tools: vec![
                AgentTool {
                    id: "shell".to_string(),
                    name: "Shell Exec".to_string(),
                    description: "Execute shell commands".to_string(),
                    icon: "💻".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "file_io".to_string(),
                    name: "File I/O".to_string(),
                    description: "Read and write files".to_string(),
                    icon: "📁".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "web_fetch".to_string(),
                    name: "Web Fetch".to_string(),
                    description: "Fetch URLs and web content".to_string(),
                    icon: "🌐".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "python".to_string(),
                    name: "Python REPL".to_string(),
                    description: "Execute Python code".to_string(),
                    icon: "🐍".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "sqlite".to_string(),
                    name: "SQLite".to_string(),
                    description: "Query SQLite databases".to_string(),
                    icon: "🗄️".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "data_analysis".to_string(),
                    name: "Data Analysis".to_string(),
                    description: "Analyze and visualize data".to_string(),
                    icon: "📊".to_string(),
                    enabled: false,
                },
            ],
            system_prompt: r#"# Agent Instructions

You are a helpful coding assistant with access to local tools.

## Guidelines

- Always explain before executing
- Ask for confirmation on destructive operations
- Prefer minimal, targeted changes"#.to_string(),
        }
    }
}

/// Complete build configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    pub llamafile_version: String,
    pub model_id: String,
    pub quantization: String,
    pub server: ServerConfig,
    pub compute: ComputeConfig,
    pub agent: AgentConfig,
    pub output_path: String,
}

/// Build progress status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildProgress {
    pub stage: BuildStage,
    pub progress_pct: f32,
    pub message: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BuildStage {
    Idle,
    DownloadingModel,
    DownloadingRuntime,
    WritingConfig,
    BundlingGpu,
    CreatingExecutable,
    Complete,
    Failed,
    Cancelled,
}

impl Default for BuildProgress {
    fn default() -> Self {
        Self {
            stage: BuildStage::Idle,
            progress_pct: 0.0,
            message: String::new(),
            bytes_downloaded: 0,
            bytes_total: 0,
            error: None,
        }
    }
}

/// System information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub memory_gb: f64,
    pub has_nvidia_gpu: bool,
    pub has_amd_gpu: bool,
    pub has_metal: bool,
}

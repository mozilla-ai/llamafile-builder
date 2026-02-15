use serde::{Deserialize, Serialize};

/// GPU library download info for a specific platform
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuLibrary {
    pub download_url: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// GPU libraries available for different platforms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuLibsByPlatform {
    #[serde(default)]
    pub linux: Option<GpuLibrary>,
    #[serde(default)]
    pub windows: Option<GpuLibrary>,
}

/// GPU libraries for a release (CUDA and ROCm)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuLibs {
    #[serde(default)]
    pub cuda: Option<GpuLibsByPlatform>,
    #[serde(default)]
    pub rocm: Option<GpuLibsByPlatform>,
}

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
    /// GPU libraries available for this release
    #[serde(default)]
    pub gpu_libs: Option<GpuLibs>,
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
    pub context_length: u32,
    pub files: Vec<ModelFile>,
}

/// Config schema item for data-driven configuration UI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigSchemaItem {
    /// A section with nested items, optionally toggleable (if key is present)
    Section {
        label: String,
        /// If present, section is toggleable and this is its key in configValues
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        description: Option<String>,
        /// Default toggle state (only used if key is present)
        #[serde(default)]
        default: Option<bool>,
        /// Flag to emit when section is enabled (like a boolean)
        #[serde(default)]
        flag: Option<String>,
        /// Nested items within this section
        #[serde(default)]
        items: Vec<ConfigSchemaItem>,
    },
    Text {
        key: String,
        label: String,
        description: String,
        default: serde_json::Value,
        value_type: String,
        flag: Option<String>,
        #[serde(default)]
        hidden: bool,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        conditional: Option<String>,
    },
    Dropdown {
        key: String,
        label: String,
        description: String,
        default: serde_json::Value,
        options: Vec<serde_json::Value>,
        flag: Option<String>,
        #[serde(default)]
        hidden: bool,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        conditional: Option<String>,
    },
    Bool {
        key: String,
        label: String,
        description: String,
        default: bool,
        flag: Option<String>,
        #[serde(default)]
        hidden: bool,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        conditional: Option<String>,
    },
    Slider {
        key: String,
        label: String,
        description: String,
        default: f64,
        min: f64,
        max: f64,
        #[serde(default = "default_step")]
        step: f64,
        flag: Option<String>,
        #[serde(default)]
        hidden: bool,
        #[serde(default)]
        required: bool,
        #[serde(default)]
        conditional: Option<String>,
    },
}

fn default_step() -> f64 {
    1.0
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
    /// Configuration values from the schema-driven UI (keys match config.json)
    pub config_values: std::collections::HashMap<String, serde_json::Value>,
    pub compute: ComputeConfig,
    pub agent: AgentConfig,
    pub output_path: String,
    /// Model filename for .args generation (e.g., "model.gguf")
    pub model_filename: String,
    /// Optional mmproj filename for multimodal models
    pub mmproj_filename: Option<String>,
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

/// An Ollama model discovered from local cache
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub model_name: String,     // e.g., "qwen3"
    pub tag: String,            // e.g., "8b"
    pub display_name: String,   // e.g., "qwen3:8b"
    pub weights_path: String,   // Full path to blob file
    pub size_bytes: u64,        // File size
}

/// A GGUF file within an LM Studio model directory
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioFile {
    pub filename: String,       // e.g., "model-Q4_K_M.gguf"
    pub path: String,           // Full path to GGUF file
    pub size_bytes: u64,        // File size
    pub role: String,           // "weights" or "mmproj" (inferred from filename)
}

/// An LM Studio model discovered from local cache
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioModel {
    pub dir_name: String,       // Directory name (e.g., "Qwen2.5-7B-Instruct-GGUF")
    pub display_name: String,   // Display name (same as dir_name, cleaned up)
    pub dir_path: String,       // Full path to model directory
    pub files: Vec<LMStudioFile>, // GGUF files in the directory
    pub total_size_bytes: u64,  // Total size of all files
}

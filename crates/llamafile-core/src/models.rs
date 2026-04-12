use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Llamafile Versions
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuLibrary {
    pub download_url: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuLibsByPlatform {
    #[serde(default)]
    pub linux: Option<GpuLibrary>,
    #[serde(default)]
    pub windows: Option<GpuLibrary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuLibs {
    #[serde(default)]
    pub cuda: Option<GpuLibsByPlatform>,
    #[serde(default)]
    pub rocm: Option<GpuLibsByPlatform>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlamafileRelease {
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub release_date: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub gpu_libs: Option<GpuLibs>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlamafileVersionGroup {
    pub name: String,
    pub tag: String,
    #[serde(default)]
    pub description: Option<String>,
    pub releases: Vec<LlamafileRelease>,
}

// ============================================================================
// Models
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFileQuantization {
    pub name: String,
    pub size_bytes: u64,
    pub download_url: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelFile {
    pub role: String,
    pub label: Option<String>,
    pub quantizations: Vec<ModelFileQuantization>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

// ============================================================================
// Config Schema
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConfigSchemaItem {
    Section {
        label: String,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        default: Option<bool>,
        #[serde(default)]
        flag: Option<String>,
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

// ============================================================================
// Compute & Agent Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ComputeMode {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComputeConfig {
    pub mode: ComputeMode,
    pub cuda: bool,
    pub rocm: bool,
    pub metal: bool,
}

impl Default for ComputeConfig {
    fn default() -> Self {
        Self {
            mode: ComputeMode::Cpu,
            cuda: false,
            rocm: false,
            metal: true, // always on, built-in on macOS
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTool {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
                    icon: "terminal".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "file_io".to_string(),
                    name: "File I/O".to_string(),
                    description: "Read and write files".to_string(),
                    icon: "folder".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "web_fetch".to_string(),
                    name: "Web Fetch".to_string(),
                    description: "Fetch URLs and web content".to_string(),
                    icon: "globe".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "python".to_string(),
                    name: "Python REPL".to_string(),
                    description: "Execute Python code".to_string(),
                    icon: "code".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "sqlite".to_string(),
                    name: "SQLite".to_string(),
                    description: "Query SQLite databases".to_string(),
                    icon: "database".to_string(),
                    enabled: false,
                },
                AgentTool {
                    id: "data_analysis".to_string(),
                    name: "Data Analysis".to_string(),
                    description: "Analyze and visualize data".to_string(),
                    icon: "chart".to_string(),
                    enabled: false,
                },
            ],
            system_prompt: r#"# Agent Instructions

You are a helpful coding assistant with access to local tools.

## Guidelines

- Always explain before executing
- Ask for confirmation on destructive operations
- Prefer minimal, targeted changes"#
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_cores: usize,
    pub memory_gb: f64,
    pub has_nvidia_gpu: bool,
    pub has_amd_gpu: bool,
    pub has_metal: bool,
}

// ============================================================================
// Local Model Sources
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModel {
    pub model_name: String,
    pub tag: String,
    pub display_name: String,
    pub weights_path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioFile {
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LMStudioModel {
    pub dir_name: String,
    pub display_name: String,
    pub dir_path: String,
    pub files: Vec<LMStudioFile>,
    pub total_size_bytes: u64,
}

// ============================================================================
// Component model (new architecture)
// ============================================================================

/// A resolved file that will be included in the final llamafile.
#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub role: ComponentRole,
    pub source: ComponentSource,
    pub zip_path: String,
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentRole {
    /// The llamafile APE binary (base of the output file, not bundled into zip).
    Runtime,
    /// The zipalign tool (used during build, not bundled).
    Zipalign,
    /// Primary model weights (.gguf).
    Weights,
    /// Multimodal projector weights (.gguf).
    MmprojWeights,
    /// GPU acceleration library.
    GpuLib { accel: GpuAccel, os: Os },
    /// .args configuration file.
    ArgsFile,
    /// .args.prompt for agent mode.
    ArgsPrompt,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComponentSource {
    /// Download from HuggingFace using the hf-hub shared cache.
    HuggingFace { repo_id: String, filename: String },
    /// Download from a direct URL (runtimes, GPU libs).
    Url(String),
    /// Use a file already on disk.
    Local(PathBuf),
    /// Generated at build time (e.g. .args from schema).
    Generated,
}

impl Component {
    /// Short display string for the component source (used in tracing spans).
    pub fn source_display(&self) -> String {
        match &self.source {
            ComponentSource::HuggingFace { repo_id, filename } => {
                format!("hf:{}/{}", repo_id, filename)
            }
            ComponentSource::Url(url) => get_filename_from_url(url),
            ComponentSource::Local(path) => {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("local")
                    .to_string()
            }
            ComponentSource::Generated => "generated".to_string(),
        }
    }
}

/// GPU acceleration type. Metal is not included because it is compiled
/// on-the-fly from sources embedded in the llamafile binary itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuAccel {
    Cuda,
    Rocm,
    Vulkan,
}

/// Target OS for GPU libraries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Windows,
}

// ============================================================================
// Helper functions
// ============================================================================

/// Search models by name, organization, or ID, with optional tag filter.
pub fn search_models<'a>(
    models: &'a [Model],
    query: Option<&str>,
    tag: Option<&str>,
) -> Vec<&'a Model> {
    models
        .iter()
        .filter(|m| {
            if let Some(q) = query {
                let q = q.to_lowercase();
                let name_match = m.name.to_lowercase().contains(&q)
                    || m.organization.to_lowercase().contains(&q)
                    || m.id.to_lowercase().contains(&q);
                if !name_match {
                    return false;
                }
            }
            if let Some(t) = tag {
                if !m.tags.iter().any(|mt| mt.eq_ignore_ascii_case(t)) {
                    return false;
                }
            }
            true
        })
        .collect()
}

pub fn get_filename_from_path(path: &str) -> String {
    path.split('/')
        .last()
        .unwrap_or(path)
        .split('\\')
        .last()
        .unwrap_or(path)
        .to_string()
}

pub fn get_filename_from_url(url: &str) -> String {
    url.split('/')
        .last()
        .unwrap_or("file")
        .split('?')
        .next()
        .unwrap_or("file")
        .to_string()
}

pub fn get_default_output_dir() -> String {
    directories::UserDirs::new()
        .and_then(|dirs| {
            dirs.desktop_dir()
                .map(|p| p.to_string_lossy().to_string())
                .or_else(|| dirs.download_dir().map(|p| p.to_string_lossy().to_string()))
                .or_else(|| dirs.document_dir().map(|p| p.to_string_lossy().to_string()))
        })
        .or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| ".".to_string())
}

pub fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

/// Return the default llamafile cache directory for runtime binaries.
/// Used internally by state management for resolved paths.
pub(crate) fn get_llamafile_cache_dir() -> String {
    let base = directories::BaseDirs::new();
    if let Some(base) = base {
        if cfg!(target_os = "windows") {
            base.data_local_dir()
                .join("llamafile")
                .join("bin")
                .to_string_lossy()
                .to_string()
        } else {
            base.home_dir()
                .join(".llamafile")
                .join("bin")
                .to_string_lossy()
                .to_string()
        }
    } else {
        ".".to_string()
    }
}

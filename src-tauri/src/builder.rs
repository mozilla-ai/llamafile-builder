use crate::models::*;
use std::path::Path;
use thiserror::Error;
use tokio::fs;
use tokio::process::Command;

#[derive(Error, Debug)]
pub enum BuildError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Download failed: {0}")]
    Download(String),
    
    #[error("Build process failed: {0}")]
    Process(String),
    
    #[error("Invalid configuration: {0}")]
    Config(String),
    
    #[error("Build cancelled")]
    Cancelled,
}

/// Generate the .args file content from server configuration
pub fn generate_args_file(config: &ServerConfig, agent: &AgentConfig) -> String {
    let mut args = Vec::new();
    
    // Server settings
    args.push(format!("--host {}", config.host));
    args.push(format!("--port {}", config.port));
    args.push(format!("-c {}", config.context_length));
    args.push(format!("-b {}", config.batch_size));
    
    // Threads
    if let Some(threads) = config.threads {
        args.push(format!("-t {}", threads));
    }
    
    // Features
    if config.flash_attention {
        args.push("--flash-attn".to_string());
    }
    
    if !config.enable_web_ui {
        args.push("--no-webui".to_string());
    }
    
    if let Some(ref api_key) = config.api_key {
        args.push(format!("--api-key {}", api_key));
    }
    
    // Agent mode settings
    if agent.enabled {
        // Add system prompt file reference
        args.push("--system-prompt-file .args.prompt".to_string());
        
        // Tool-specific flags would go here
        // This depends on how agentfile implements tool registration
    }
    
    args.join("\n")
}

/// Generate the system prompt file for agent mode
pub fn generate_system_prompt(agent: &AgentConfig) -> String {
    let mut prompt = agent.system_prompt.clone();
    
    // Append tool descriptions if any tools are enabled
    let enabled_tools: Vec<_> = agent.tools.iter().filter(|t| t.enabled).collect();
    
    if !enabled_tools.is_empty() {
        prompt.push_str("\n\n## Available Tools\n\n");
        for tool in enabled_tools {
            prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        }
    }
    
    prompt
}

/// Estimate the final llamafile size
pub fn estimate_size(
    model_size: u64,
    compute: &ComputeConfig,
    agent: &AgentConfig,
) -> u64 {
    let runtime_overhead: u64 = 5_000_000; // ~5 MB base
    
    let gpu_overhead: u64 = if compute.mode == ComputeMode::Gpu {
        let mut overhead = 0u64;
        if compute.cuda { overhead += 180_000_000; }  // ~180 MB
        if compute.rocm { overhead += 60_000_000; }   // ~60 MB
        if compute.metal { overhead += 10_000_000; }  // ~10 MB
        overhead
    } else {
        0
    };
    
    let agent_overhead: u64 = if agent.enabled { 1_000_000 } else { 0 }; // ~1 MB
    
    model_size + runtime_overhead + gpu_overhead + agent_overhead
}

/// Build the llamafile (main orchestration function)
/// 
/// This function coordinates the entire build process:
/// 1. Download model weights to cache
/// 2. Download llamafile runtime
/// 3. Generate .args configuration
/// 4. Bundle everything into a single executable
pub async fn build_llamafile(
    config: &BuildConfig,
    cache_dir: &Path,
    progress_callback: impl Fn(BuildProgress),
    cancel_check: impl Fn() -> bool,
) -> Result<(), BuildError> {
    // Validate configuration
    if config.output_path.is_empty() {
        return Err(BuildError::Config("Output path is required".to_string()));
    }
    
    let output_path = Path::new(&config.output_path);
    
    // Create cache directories
    let models_cache = cache_dir.join("models");
    let runtime_cache = cache_dir.join("runtime");
    fs::create_dir_all(&models_cache).await?;
    fs::create_dir_all(&runtime_cache).await?;
    
    // Step 1: Download model weights
    progress_callback(BuildProgress {
        stage: BuildStage::DownloadingModel,
        progress_pct: 0.0,
        message: "Downloading model weights...".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    });
    
    if cancel_check() {
        return Err(BuildError::Cancelled);
    }
    
    // In production: download model from URL to models_cache
    // let model_path = models_cache.join(&format!("{}.gguf", config.model_id));
    // download_file(model_url, &model_path, ...).await?;
    
    // Step 2: Download runtime
    progress_callback(BuildProgress {
        stage: BuildStage::DownloadingRuntime,
        progress_pct: 60.0,
        message: "Fetching llamafile runtime...".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    });
    
    if cancel_check() {
        return Err(BuildError::Cancelled);
    }
    
    // In production: download llamafile binary
    // let runtime_path = runtime_cache.join(&format!("llamafile-{}", config.llamafile_version));
    // download_file(runtime_url, &runtime_path, ...).await?;
    
    // Step 3: Write configuration
    progress_callback(BuildProgress {
        stage: BuildStage::WritingConfig,
        progress_pct: 75.0,
        message: "Writing .args configuration...".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    });
    
    let args_content = generate_args_file(&config.server, &config.agent);
    let args_path = cache_dir.join("temp.args");
    fs::write(&args_path, &args_content).await?;
    
    // Write system prompt if agent mode is enabled
    if config.agent.enabled {
        let prompt_content = generate_system_prompt(&config.agent);
        let prompt_path = cache_dir.join("temp.args.prompt");
        fs::write(&prompt_path, &prompt_content).await?;
    }
    
    // Step 4: Bundle GPU libraries if needed
    if config.compute.mode == ComputeMode::Gpu {
        progress_callback(BuildProgress {
            stage: BuildStage::BundlingGpu,
            progress_pct: 85.0,
            message: "Bundling GPU libraries...".to_string(),
            bytes_downloaded: 0,
            bytes_total: 0,
            error: None,
        });
        
        if cancel_check() {
            return Err(BuildError::Cancelled);
        }
        
        // In production: bundle CUDA/ROCm/Metal libraries
        // This involves running zipalign to append libraries to the executable
    }
    
    // Step 5: Create final executable
    progress_callback(BuildProgress {
        stage: BuildStage::CreatingExecutable,
        progress_pct: 95.0,
        message: "Creating executable...".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    });
    
    if cancel_check() {
        return Err(BuildError::Cancelled);
    }
    
    // In production: use zipalign to create the final llamafile
    // This combines: runtime + model.gguf + .args + optional GPU libs
    
    // Pseudocode:
    // 1. Copy runtime to output path
    // 2. zipalign -j0 output.llamafile model.gguf
    // 3. zipalign -j0 output.llamafile .args
    // 4. If agent: zipalign -j0 output.llamafile .args.prompt
    // 5. If GPU: zipalign GPU libraries
    // 6. chmod +x output.llamafile
    
    // For now, simulate success
    progress_callback(BuildProgress {
        stage: BuildStage::Complete,
        progress_pct: 100.0,
        message: "Build complete!".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    });
    
    Ok(())
}

/// Make a file executable on Unix systems
#[cfg(unix)]
pub async fn make_executable(path: &Path) -> Result<(), BuildError> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).await?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).await?;
    Ok(())
}

#[cfg(not(unix))]
pub async fn make_executable(_path: &Path) -> Result<(), BuildError> {
    // No-op on Windows
    Ok(())
}

/// Run zipalign to append a file to the llamafile
pub async fn zipalign_append(
    llamafile_path: &Path,
    file_to_add: &Path,
    internal_name: &str,
) -> Result<(), BuildError> {
    // This requires the zipalign tool from cosmopolitan libc
    // In production, we'd either bundle zipalign or implement the ZIP append logic in Rust
    
    let status = Command::new("zipalign")
        .arg("-j0")
        .arg(llamafile_path)
        .arg(file_to_add)
        .arg(internal_name)
        .status()
        .await?;
    
    if !status.success() {
        return Err(BuildError::Process(format!(
            "zipalign failed with exit code: {:?}",
            status.code()
        )));
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_args_file() {
        let config = ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            context_length: 8192,
            threads: Some(8),
            batch_size: 512,
            api_key: None,
            enable_web_ui: true,
            flash_attention: true,
        };
        
        let agent = AgentConfig::default();
        let args = generate_args_file(&config, &agent);
        
        assert!(args.contains("--host 0.0.0.0"));
        assert!(args.contains("--port 8080"));
        assert!(args.contains("-c 8192"));
        assert!(args.contains("-t 8"));
        assert!(args.contains("--flash-attn"));
        assert!(!args.contains("--no-webui"));
    }
    
    #[test]
    fn test_estimate_size() {
        let model_size = 5_000_000_000u64; // 5 GB
        
        // CPU only
        let cpu_config = ComputeConfig {
            mode: ComputeMode::Cpu,
            cuda: false,
            rocm: false,
            metal: false,
        };
        let agent = AgentConfig::default();
        let cpu_size = estimate_size(model_size, &cpu_config, &agent);
        assert!(cpu_size < 5_100_000_000); // Should be ~5 GB + overhead
        
        // GPU with all libraries
        let gpu_config = ComputeConfig {
            mode: ComputeMode::Gpu,
            cuda: true,
            rocm: true,
            metal: true,
        };
        let gpu_size = estimate_size(model_size, &gpu_config, &agent);
        assert!(gpu_size > 5_200_000_000); // Should include GPU libs
    }
}

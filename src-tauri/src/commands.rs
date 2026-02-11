use crate::build_state::{BuildState, BuildStateManager};
use crate::models::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tauri::State;

/// Global app state
pub struct AppState {
    pub build_state: BuildStateManager,
    pub download_cancel_flag: Arc<AtomicBool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            build_state: BuildStateManager::new(),
            download_cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Get available llamafile versions (loaded from versions.json)
#[tauri::command]
pub async fn get_llamafile_versions() -> Result<Vec<LlamafileVersionGroup>, String> {
    let json = include_str!("../versions.json");
    serde_json::from_str(json).map_err(|e| format!("Failed to parse versions.json: {}", e))
}

/// Get available models (loaded from models.json)
#[tauri::command]
pub async fn get_models() -> Result<Vec<Model>, String> {
    let json = include_str!("../models.json");
    serde_json::from_str(json).map_err(|e| format!("Failed to parse models.json: {}", e))
}

/// Get configuration schema (loaded from config.json)
#[tauri::command]
pub async fn get_config_schema() -> Result<Vec<ConfigSchemaItem>, String> {
    let json = include_str!("../config.json");
    serde_json::from_str(json).map_err(|e| format!("Failed to parse config.json: {}", e))
}

/// Generate .args file content from config values and model filenames
#[tauri::command(rename_all = "camelCase")]
pub async fn generate_args_content(
    config_values: std::collections::HashMap<String, serde_json::Value>,
    model_filename: String,
    mmproj_filename: Option<String>,
) -> Result<String, String> {
    use crate::builder::generate_args_from_schema;

    let schema = get_config_schema().await?;
    Ok(generate_args_from_schema(
        &schema,
        &config_values,
        &model_filename,
        mmproj_filename.as_deref(),
    ))
}

/// Search models by query
#[tauri::command]
pub async fn search_models(query: String, tag: Option<String>) -> Result<Vec<Model>, String> {
    let all_models = get_models().await?;
    let query_lower = query.to_lowercase();

    let filtered: Vec<Model> = all_models
        .into_iter()
        .filter(|m| {
            let matches_query = query.is_empty()
                || m.name.to_lowercase().contains(&query_lower)
                || m.organization.to_lowercase().contains(&query_lower);

            let matches_tag = tag.as_ref().map_or(true, |t| {
                let t_lower = t.to_lowercase();
                m.tags.iter().any(|mt| mt.to_lowercase() == t_lower)
            });

            matches_query && matches_tag
        })
        .collect();

    Ok(filtered)
}

/// Get detailed model information
#[tauri::command]
pub async fn get_model_details(model_id: String) -> Result<Option<Model>, String> {
    let models = get_models().await?;
    Ok(models.into_iter().find(|m| m.id == model_id))
}

/// Start the build process
#[tauri::command]
pub async fn start_build(
    config: BuildConfig,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use std::time::Duration;
    use tokio::time::sleep;

    let stages = vec![
        (BuildStage::DownloadingModel, "Downloading model weights...", 60.0),
        (BuildStage::DownloadingRuntime, "Fetching llamafile runtime...", 75.0),
        (BuildStage::WritingConfig, "Writing .args configuration...", 82.0),
        (BuildStage::BundlingGpu, "Bundling GPU libraries...", 95.0),
        (BuildStage::CreatingExecutable, "Creating executable...", 100.0),
    ];

    for (stage, message, progress) in stages {
        let progress_update = BuildProgress {
            stage: stage.clone(),
            progress_pct: progress,
            message: message.to_string(),
            bytes_downloaded: 0,
            bytes_total: 0,
            error: None,
        };

        app_handle.emit("build-progress", &progress_update)
            .map_err(|e| e.to_string())?;

        // Simulate work
        sleep(Duration::from_millis(1500)).await;
    }

    // Final completion
    let complete = BuildProgress {
        stage: BuildStage::Complete,
        progress_pct: 100.0,
        message: "Build complete!".to_string(),
        bytes_downloaded: 0,
        bytes_total: 0,
        error: None,
    };

    app_handle.emit("build-progress", &complete)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Cancel the current build (placeholder - build cancellation handled in JS)
#[tauri::command]
pub async fn cancel_build() -> Result<(), String> {
    Ok(())
}

/// Get current build status (placeholder - build status managed in JS)
#[tauri::command]
pub async fn get_build_status() -> Result<BuildProgress, String> {
    Ok(BuildProgress::default())
}

/// Open file dialog to select output path
#[tauri::command(rename_all = "camelCase")]
pub async fn select_output_path(app_handle: tauri::AppHandle, default_path: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;
    use std::path::Path;

    let (tx, rx) = oneshot::channel();

    let path = Path::new(&default_path);
    let mut dialog = app_handle.dialog().file();

    // Set starting directory from the default path
    if let Some(parent) = path.parent() {
        dialog = dialog.set_directory(parent);
    }

    // Set default filename
    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
        dialog = dialog.set_file_name(filename);
    }

    dialog
        .add_filter("llamafile", &["llamafile"])
        .save_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    rx.await.map_err(|_| "Dialog cancelled".to_string())?
        .map_or(Ok(None), |p| Ok(Some(p)))
}

/// Get default output path
#[tauri::command(rename_all = "camelCase")]
pub async fn get_default_output_path(gguf_filename: Option<String>, model_name: Option<String>) -> Result<String, String> {
    // Find a suitable output directory: Desktop > Downloads > Documents > Home
    let output_dir = directories::UserDirs::new()
        .and_then(|dirs| {
            dirs.desktop_dir().map(|p| p.to_path_buf())
                .or_else(|| dirs.download_dir().map(|p| p.to_path_buf()))
                .or_else(|| dirs.document_dir().map(|p| p.to_path_buf()))
        })
        .or_else(|| dirs::home_dir())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // Determine filename: prefer GGUF filename, fall back to model name
    let base_name = if let Some(gguf) = gguf_filename {
        // Remove .gguf extension if present
        gguf.trim_end_matches(".gguf")
            .trim_end_matches(".GGUF")
            .to_string()
    } else if let Some(name) = model_name {
        // Sanitize model name
        name.to_lowercase()
            .replace(" ", "-")
            .replace(".", "-")
    } else {
        "output".to_string()
    };

    let path = output_dir.join(format!("{}.llamafile", base_name));
    Ok(path.to_string_lossy().to_string())
}

/// Get system information
#[tauri::command]
pub async fn get_system_info() -> Result<SystemInfo, String> {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let cpu_cores = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);

    let has_metal = cfg!(target_os = "macos");

    Ok(SystemInfo {
        os,
        arch,
        cpu_cores,
        memory_gb: 16.0,
        has_nvidia_gpu: false,
        has_amd_gpu: false,
        has_metal,
    })
}

/// Open file dialog to select a llamafile executable from disk
#[tauri::command]
pub async fn select_llamafile_path(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select llamafile executable")
        .pick_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    rx.await.map_err(|_| "Dialog cancelled".to_string())?
        .map_or(Ok(None), |p| Ok(Some(p)))
}

/// Get the platform-appropriate cache directory for downloaded llamafile binaries
/// Returns the directory path; caller should append filename from download URL
#[tauri::command]
pub async fn get_llamafile_cache_dir() -> Result<String, String> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| "Could not determine home directory".to_string())?;

    let path = if cfg!(target_os = "windows") {
        base.data_local_dir()
            .join("llamafile")
            .join("bin")
    } else {
        base.home_dir()
            .join(".llamafile")
            .join("bin")
    };

    Ok(path.to_string_lossy().to_string())
}

/// Get the platform-appropriate model cache directory
#[tauri::command]
pub async fn get_model_cache_dir() -> Result<String, String> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| "Could not determine home directory".to_string())?;

    let path = if cfg!(target_os = "windows") {
        base.data_local_dir()
            .join("llamafile")
            .join("models")
    } else {
        base.home_dir()
            .join(".llamafile")
            .join("models")
    };

    Ok(path.to_string_lossy().to_string())
}

/// Open file dialog to select a model file (.gguf) from disk
#[tauri::command]
pub async fn select_model_file_path(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select GGUF model file")
        .add_filter("GGUF Model", &["gguf"])
        .pick_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    rx.await.map_err(|_| "Dialog cancelled".to_string())?
        .map_or(Ok(None), |p| Ok(Some(p)))
}

/// Open directory picker to select model download directory
#[tauri::command]
pub async fn select_model_download_dir(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select model download directory")
        .pick_folder(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    rx.await.map_err(|_| "Dialog cancelled".to_string())?
        .map_or(Ok(None), |p| Ok(Some(p)))
}

/// Run zipalign binary with given args, streaming output via build-log events
#[tauri::command]
pub async fn run_zipalign(
    args: Vec<String>,
    app_handle: tauri::AppHandle,
) -> Result<i32, String> {
    use tokio::process::Command;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let bin_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin");
    let zipalign_path = bin_dir.join("zipalign");

    if !zipalign_path.exists() {
        let msg = format!("zipalign not found at {}", zipalign_path.display());
        app_handle.emit("build-log", &msg).ok();
        return Err(msg);
    }

    // Log the command being run
    let cmd_line = format!("$ zipalign {}", args.join(" "));
    app_handle.emit("build-log", &cmd_line).map_err(|e| e.to_string())?;

    let mut child = Command::new(&zipalign_path)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn zipalign: {}", e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let handle_out = app_handle.clone();
    let stdout_task = tokio::spawn(async move {
        if let Some(out) = stdout {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                handle_out.emit("build-log", &line).ok();
            }
        }
    });

    let handle_err = app_handle.clone();
    let stderr_task = tokio::spawn(async move {
        if let Some(err) = stderr {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                handle_err.emit("build-log", &line).ok();
            }
        }
    });

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let status = child.wait().await.map_err(|e| format!("Failed to wait on zipalign: {}", e))?;
    let code = status.code().unwrap_or(-1);

    let exit_msg = format!("zipalign exited with code {}", code);
    app_handle.emit("build-log", &exit_msg).ok();

    Ok(code)
}

/// Download a file from URL with progress events
/// Returns the destination path on success, or an error string
/// Error string will be "AUTH_REQUIRED" for 401/403 errors on HF URLs
/// Error string will be "CANCELLED" if download was cancelled
/// Error string will be "SIZE_MISMATCH" or "CHECKSUM_MISMATCH" for validation errors
#[tauri::command(rename_all = "camelCase")]
pub async fn download_file(
    url: String,
    dest_path: String,
    expected_size: Option<u64>,
    expected_sha256: Option<String>,
    resume: Option<bool>,
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    use crate::download::{download_file as do_download, DownloadError};
    use std::path::Path;

    let dest = Path::new(&dest_path);

    // Reset cancel flag before starting
    state.download_cancel_flag.store(false, Ordering::Relaxed);
    let cancel_flag = state.download_cancel_flag.clone();

    // Get app-stored HF token if any
    let app_token = get_stored_hf_token(&app_handle).ok().flatten();

    let handle = app_handle.clone();
    let result = do_download(
        &url,
        dest,
        expected_size,
        expected_sha256.as_deref(),
        app_token.as_deref(),
        resume.unwrap_or(false),
        |progress| {
            handle.emit("download-progress", &progress).ok();
        },
        cancel_flag,
    )
    .await;

    match result {
        Ok(()) => Ok(dest_path),
        Err(DownloadError::AuthRequired { .. }) => Err("AUTH_REQUIRED".to_string()),
        Err(DownloadError::Cancelled) => Err("CANCELLED".to_string()),
        Err(DownloadError::SizeMismatch { expected, actual }) => {
            Err(format!("SIZE_MISMATCH:{}:{}", expected, actual))
        }
        Err(DownloadError::ChecksumMismatch { expected, actual }) => {
            Err(format!("CHECKSUM_MISMATCH:{}:{}", expected, actual))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Cancel the current download
#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>) -> Result<(), String> {
    state.download_cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
}

/// Get detailed file info including partial file detection
#[tauri::command(rename_all = "camelCase")]
pub async fn get_file_info(
    path: String,
    expected_size: Option<u64>,
    expected_sha256: Option<String>,
    verify_checksum: Option<bool>,
) -> Result<crate::download::FileInfo, String> {
    use crate::download::get_file_info as do_get_file_info;
    use std::path::Path;

    let p = Path::new(&path);
    Ok(do_get_file_info(
        p,
        expected_size,
        expected_sha256.as_deref(),
        verify_checksum.unwrap_or(false),
    ).await)
}

/// Simple check if a file exists (for backwards compatibility)
#[tauri::command(rename_all = "camelCase")]
pub async fn file_exists(
    path: String,
    expected_sha256: Option<String>,
) -> Result<bool, String> {
    use std::path::Path;

    let p = Path::new(&path);
    if !p.exists() {
        return Ok(false);
    }

    // If checksum provided, verify it
    if let Some(expected) = expected_sha256 {
        use sha2::{Digest, Sha256};
        match tokio::fs::read(p).await {
            Ok(contents) => {
                let mut hasher = Sha256::new();
                hasher.update(&contents);
                let actual = hex::encode(hasher.finalize());
                Ok(actual == expected)
            }
            Err(_) => Ok(false),
        }
    } else {
        Ok(true)
    }
}

/// Delete a file (for cleaning up partial downloads)
#[tauri::command]
pub async fn delete_file(path: String) -> Result<(), String> {
    use crate::download::delete_file as do_delete;
    use std::path::Path;

    do_delete(Path::new(&path))
        .await
        .map_err(|e| e.to_string())
}

/// Copy a file from src to dest
#[tauri::command]
pub async fn copy_file(src: String, dest: String) -> Result<(), String> {
    use std::path::Path;

    let src_path = Path::new(&src);
    let dest_path = Path::new(&dest);

    // Create parent directory if it doesn't exist
    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    tokio::fs::copy(src_path, dest_path)
        .await
        .map_err(|e| format!("Failed to copy file: {}", e))?;

    Ok(())
}

/// Write content to a temporary file and return its path
#[tauri::command]
pub async fn write_temp_file(content: String, filename: String) -> Result<String, String> {
    let temp_dir = std::env::temp_dir().join("llamafile-builder");

    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let file_path = temp_dir.join(&filename);

    tokio::fs::write(&file_path, content)
        .await
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    Ok(file_path.to_string_lossy().to_string())
}

/// Make a file executable (Unix only, no-op on Windows)
#[tauri::command]
pub async fn make_executable(path: String) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        use std::path::Path;

        let p = Path::new(&path);
        let metadata = tokio::fs::metadata(p)
            .await
            .map_err(|e| format!("Failed to get file metadata: {}", e))?;

        let mut perms = metadata.permissions();
        perms.set_mode(0o755);

        tokio::fs::set_permissions(p, perms)
            .await
            .map_err(|e| format!("Failed to set permissions: {}", e))?;
    }

    Ok(())
}

/// Calculate SHA256 checksum of a file
#[tauri::command]
pub async fn calculate_sha256(path: String) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::path::Path;
    use tokio::io::AsyncReadExt;

    let p = Path::new(&path);
    if !p.exists() {
        return Err("File not found".to_string());
    }

    let mut file = tokio::fs::File::open(p)
        .await
        .map_err(|e| e.to_string())?;

    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer

    loop {
        let n = file.read(&mut buffer).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Get the current HF token (from env, HF CLI cache, or app storage)
#[tauri::command]
pub async fn get_hf_token(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    use crate::download::resolve_hf_token;

    let app_token = get_stored_hf_token(&app_handle).ok().flatten();
    Ok(resolve_hf_token(app_token.as_deref()))
}

/// Store HF token in app data directory
#[tauri::command]
pub async fn set_hf_token(
    token: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    std::fs::create_dir_all(&app_dir).map_err(|e| e.to_string())?;

    let token_path = app_dir.join("hf_token");
    std::fs::write(&token_path, token.trim()).map_err(|e| e.to_string())?;

    Ok(())
}

/// Clear stored HF token
#[tauri::command]
pub async fn clear_hf_token(app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let token_path = app_dir.join("hf_token");
    if token_path.exists() {
        std::fs::remove_file(&token_path).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Internal helper to get stored HF token from app data directory
fn get_stored_hf_token(app_handle: &tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri::Manager;

    let app_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?;

    let token_path = app_dir.join("hf_token");
    if token_path.exists() {
        let token = std::fs::read_to_string(&token_path)
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }

    Ok(None)
}

/// Open a URL in the system's default browser
#[tauri::command]
pub async fn open_url(url: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;

    app_handle
        .shell()
        .open(&url, None)
        .map_err(|e| e.to_string())
}

/// Validate configuration before build
#[tauri::command]
pub async fn validate_config(config: BuildConfig) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();

    // Get context_length from config_values (default to 4096 if not set)
    let context_length = config.config_values
        .get("context_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(4096) as u32;

    if context_length > 32768 {
        warnings.push("Large context lengths require significant RAM".to_string());
    }

    if config.compute.mode == ComputeMode::Cpu && context_length > 8192 {
        warnings.push("CPU inference with large context may be slow".to_string());
    }

    if config.agent.enabled && config.agent.tools.iter().any(|t| t.id == "shell" && t.enabled) {
        warnings.push("Shell execution tool enabled — use with caution".to_string());
    }

    Ok(warnings)
}

// ============================================================================
// Build State Commands
// ============================================================================

/// Get the current build state
#[tauri::command]
pub async fn get_build_state(state: State<'_, AppState>) -> Result<BuildState, String> {
    Ok(state.build_state.get_state())
}

/// Select a model from the registry
#[tauri::command(rename_all = "camelCase")]
pub async fn select_registry_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.select_registry_model(model_id))
}

/// Select a custom model file (switches to custom mode)
#[tauri::command]
pub async fn select_custom_model(
    path: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.select_custom_model(path))
}

/// Add a companion file in custom mode (e.g., mmproj)
#[tauri::command]
pub async fn add_custom_companion_file(
    role: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.add_custom_companion_file(role, path))
}

/// Remove a companion file in custom mode
#[tauri::command]
pub async fn remove_custom_companion_file(
    role: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.remove_custom_companion_file(role))
}

/// Select quantization for a file role
#[tauri::command]
pub async fn select_file_quantization(
    role: String,
    idx: usize,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.select_file_quantization(role, idx))
}

/// Use a custom file for a specific role (within registry model)
#[tauri::command]
pub async fn use_custom_file_for_role(
    role: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.use_custom_file_for_role(role, path))
}

/// Clear custom file for a role (revert to registry)
#[tauri::command]
pub async fn clear_custom_file_for_role(
    role: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.clear_custom_file_for_role(role))
}

/// Select a llamafile version from the registry
#[tauri::command]
pub async fn select_llamafile_version(
    group_name: String,
    version: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.select_llamafile_version(group_name, version))
}

/// Use a custom llamafile binary
#[tauri::command]
pub async fn use_custom_llamafile(
    path: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.use_custom_llamafile(path))
}

/// Set a configuration value
#[tauri::command]
pub async fn set_config_value(
    key: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.set_config_value(key, value))
}

/// Set the output path
#[tauri::command]
pub async fn set_output_path(
    path: String,
    state: State<'_, AppState>,
) -> Result<BuildState, String> {
    Ok(state.build_state.set_output_path(path))
}

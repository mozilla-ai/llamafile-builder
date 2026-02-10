use crate::models::*;
use std::sync::Mutex;
use tauri::State;
use tauri::Emitter;

/// Global build state
pub struct AppState {
    pub build_progress: Mutex<BuildProgress>,
    pub build_cancelled: Mutex<bool>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            build_progress: Mutex::new(BuildProgress::default()),
            build_cancelled: Mutex::new(false),
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

/// Cancel the current build
#[tauri::command]
pub async fn cancel_build(state: State<'_, AppState>) -> Result<(), String> {
    let mut cancelled = state.build_cancelled.lock().map_err(|e| e.to_string())?;
    *cancelled = true;
    Ok(())
}

/// Get current build status
#[tauri::command]
pub async fn get_build_status(state: State<'_, AppState>) -> Result<BuildProgress, String> {
    let progress = state.build_progress.lock().map_err(|e| e.to_string())?;
    Ok(progress.clone())
}

/// Open file dialog to select output path
#[tauri::command]
pub async fn select_output_path(app_handle: tauri::AppHandle, default_name: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter("llamafile", &["llamafile"])
        .save_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    let result = rx.await.map_err(|e| e.to_string())?;
    Ok(result)
}

/// Get default output path
#[tauri::command]
pub async fn get_default_output_path(model_name: String) -> Result<String, String> {
    let desktop = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let safe_name = model_name
        .to_lowercase()
        .replace(" ", "-")
        .replace(".", "-");

    let path = desktop.join(format!("{}.llamafile", safe_name));
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

    let (tx, rx) = tokio::sync::oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select llamafile executable")
        .pick_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    let result = rx.await.map_err(|e| e.to_string())?;
    Ok(result)
}

/// Get the platform-appropriate cache path for a downloaded llamafile version
#[tauri::command]
pub async fn get_llamafile_cache_path(version: String) -> Result<String, String> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| "Could not determine home directory".to_string())?;

    let path = if cfg!(target_os = "windows") {
        base.data_local_dir()
            .join("llamafile")
            .join("v")
            .join(&version)
            .join("bin")
            .join("llamafile.exe")
    } else {
        base.home_dir()
            .join(".llamafile")
            .join("v")
            .join(&version)
            .join("bin")
            .join("llamafile")
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

    let (tx, rx) = tokio::sync::oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select GGUF model file")
        .add_filter("GGUF Model", &["gguf"])
        .pick_file(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    let result = rx.await.map_err(|e| e.to_string())?;
    Ok(result)
}

/// Open directory picker to select model download directory
#[tauri::command]
pub async fn select_model_download_dir(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();

    app_handle
        .dialog()
        .file()
        .set_title("Select model download directory")
        .pick_folder(move |path| {
            let _ = tx.send(path.map(|p| p.to_string()));
        });

    let result = rx.await.map_err(|e| e.to_string())?;
    Ok(result)
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
#[tauri::command]
pub async fn download_file(
    url: String,
    dest_path: String,
    expected_sha256: Option<String>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    use crate::download::{download_file as do_download, DownloadError};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let dest = Path::new(&dest_path);
    let cancelled = Arc::new(AtomicBool::new(false));

    // Get app-stored HF token if any
    let app_token = get_stored_hf_token(&app_handle).ok().flatten();

    let handle = app_handle.clone();
    let result = do_download(
        &url,
        dest,
        expected_sha256.as_deref(),
        app_token.as_deref(),
        |progress| {
            handle.emit("download-progress", &progress).ok();
        },
        || cancelled.load(Ordering::Relaxed),
    )
    .await;

    match result {
        Ok(()) => Ok(dest_path),
        Err(DownloadError::AuthRequired { .. }) => Err("AUTH_REQUIRED".to_string()),
        Err(DownloadError::Cancelled) => Err("CANCELLED".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// Check if a file exists, optionally verifying its SHA256 checksum
#[tauri::command]
pub async fn file_exists(
    path: String,
    expected_sha256: Option<String>,
) -> Result<bool, String> {
    use crate::download::verify_cached_file;
    use std::path::Path;

    let p = Path::new(&path);
    Ok(verify_cached_file(p, expected_sha256.as_deref()).await)
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

/// Validate configuration before build
#[tauri::command]
pub async fn validate_config(config: BuildConfig) -> Result<Vec<String>, String> {
    let mut warnings = Vec::new();

    if config.server.context_length > 32768 {
        warnings.push("Large context lengths require significant RAM".to_string());
    }

    if config.compute.mode == ComputeMode::Cpu && config.server.context_length > 8192 {
        warnings.push("CPU inference with large context may be slow".to_string());
    }

    if config.agent.enabled && config.agent.tools.iter().any(|t| t.id == "shell" && t.enabled) {
        warnings.push("Shell execution tool enabled — use with caution".to_string());
    }

    Ok(warnings)
}

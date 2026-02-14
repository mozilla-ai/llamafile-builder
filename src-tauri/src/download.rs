use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use thiserror::Error;
use serde::{Deserialize, Serialize};

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SHA256 mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },

    #[error("Download cancelled")]
    Cancelled,

    #[error("Authentication required (HTTP {status})")]
    AuthRequired { status: u16 },

    #[error("Server does not support resume")]
    ResumeNotSupported,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub url: String,
    pub filename: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub percentage: f32,
    pub resuming: bool,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub exists: bool,
    pub size: u64,
    pub expected_size: Option<u64>,
    pub is_complete: bool,      // Size matches expected (if known)
    pub is_partial: bool,       // File exists but smaller than expected
    pub checksum_valid: Option<bool>, // None if not checked, Some(true/false) if checked
}

/// Check if a URL is from Hugging Face (should include auth token)
pub fn is_huggingface_url(url: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host.ends_with("huggingface.co") || host.ends_with("hf.co");
        }
    }
    false
}

/// Resolve HF token from multiple sources (in priority order):
/// 1. HF_TOKEN environment variable
/// 2. ~/.cache/huggingface/token (created by huggingface-cli login)
/// 3. App-stored token (passed as parameter)
pub fn resolve_hf_token(app_token: Option<&str>) -> Option<String> {
    // 1. Check environment variable
    if let Ok(token) = std::env::var("HF_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // Also check HUGGING_FACE_HUB_TOKEN (alternative env var)
    if let Ok(token) = std::env::var("HUGGING_FACE_HUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // 2. Check HF CLI cache file
    if let Some(home) = dirs::home_dir() {
        let hf_token_path = home.join(".cache").join("huggingface").join("token");
        if let Ok(token) = std::fs::read_to_string(&hf_token_path) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }

    // 3. Use app-provided token
    if let Some(token) = app_token {
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    None
}

/// Get detailed file info including partial detection
pub async fn get_file_info(
    path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    verify_checksum: bool,
) -> FileInfo {
    if !path.exists() {
        return FileInfo {
            exists: false,
            size: 0,
            expected_size,
            is_complete: false,
            is_partial: false,
            checksum_valid: None,
        };
    }

    let size = tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let is_complete = expected_size.map(|exp| size == exp).unwrap_or(false);
    let is_partial = expected_size.map(|exp| size > 0 && size < exp).unwrap_or(false);

    let checksum_valid = if verify_checksum && is_complete {
        if let Some(expected) = expected_sha256 {
            match tokio::fs::read(path).await {
                Ok(contents) => {
                    let mut hasher = Sha256::new();
                    hasher.update(&contents);
                    let actual = hex::encode(hasher.finalize());
                    Some(actual == expected)
                }
                Err(_) => Some(false),
            }
        } else {
            None
        }
    } else {
        None
    };

    FileInfo {
        exists: true,
        size,
        expected_size,
        is_complete,
        is_partial,
        checksum_valid,
    }
}

/// Download a file with progress reporting, resume support, and cancellation
pub async fn download_file<F>(
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    app_hf_token: Option<&str>,
    resume: bool,
    progress_callback: F,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), DownloadError>
where
    F: Fn(DownloadProgress),
{
    // Ensure parent directory exists
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let client = Client::builder()
        .user_agent("llamafile-builder/0.1.0")
        .build()?;

    // Check if we should resume
    let existing_size = if resume && dest.exists() {
        tokio::fs::metadata(dest).await.map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    let mut request = client.get(url);

    // Add HF auth header only for HuggingFace URLs
    if is_huggingface_url(url) {
        if let Some(token) = resolve_hf_token(app_hf_token) {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
    }

    // Add Range header for resume
    if existing_size > 0 {
        request = request.header("Range", format!("bytes={}-", existing_size));
    }

    let response = request.send().await?;
    let status = response.status();

    // Check for auth errors
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(DownloadError::AuthRequired { status: status.as_u16() });
    }

    // Check if server supports resume (206 Partial Content)
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let starting_offset = if resuming { existing_size } else { 0 };

    // If we requested resume but server doesn't support it, start fresh
    if existing_size > 0 && !resuming && status.is_success() {
        // Server returned 200 OK instead of 206, start from beginning
    }

    // Check for other errors
    let response = response.error_for_status()?;

    let content_length = response.content_length().unwrap_or(0);
    let total_size = if resuming {
        starting_offset + content_length
    } else {
        content_length
    };

    let filename = dest.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    // Open file for writing (append if resuming)
    let mut file = if resuming {
        OpenOptions::new()
            .write(true)
            .append(true)
            .open(dest)
            .await?
    } else {
        File::create(dest).await?
    };

    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = starting_offset;

    // For checksum, we need to hash the entire file if resuming
    // We'll verify at the end by reading the whole file

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) {
            drop(file);
            // Don't delete - let the frontend decide
            return Err(DownloadError::Cancelled);
        }

        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        progress_callback(DownloadProgress {
            url: url.to_string(),
            filename: filename.clone(),
            bytes_downloaded: downloaded,
            bytes_total: total_size,
            percentage: if total_size > 0 {
                (downloaded as f32 / total_size as f32) * 100.0
            } else {
                0.0
            },
            resuming,
        });
    }

    file.flush().await?;
    drop(file);

    // Verify size if provided
    let final_size = tokio::fs::metadata(dest).await?.len();
    if let Some(expected) = expected_size {
        if final_size != expected {
            return Err(DownloadError::SizeMismatch {
                expected,
                actual: final_size,
            });
        }
    }

    // Verify checksum if provided (need to hash entire file)
    if let Some(expected) = expected_sha256 {
        let mut file = File::open(dest).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer

        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            return Err(DownloadError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    Ok(())
}

/// Delete a partial/incomplete file
pub async fn delete_file(path: &Path) -> Result<(), std::io::Error> {
    if path.exists() {
        tokio::fs::remove_file(path).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_huggingface_url() {
        assert!(is_huggingface_url("https://huggingface.co/foo/bar/resolve/main/file.gguf"));
        assert!(is_huggingface_url("https://hf.co/foo/bar"));
        assert!(is_huggingface_url("https://cdn-lfs.huggingface.co/something"));
        assert!(!is_huggingface_url("https://github.com/foo/bar"));
        assert!(!is_huggingface_url("https://example.com/huggingface.co/fake"));
    }
}

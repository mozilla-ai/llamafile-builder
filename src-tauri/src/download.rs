use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
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

    #[error("Download cancelled")]
    Cancelled,

    #[error("Authentication required (HTTP {status})")]
    AuthRequired { status: u16 },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub url: String,
    pub filename: String,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub percentage: f32,
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

/// Download a file with progress reporting and optional HF authentication
pub async fn download_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    app_hf_token: Option<&str>,
    progress_callback: F,
    cancel_check: impl Fn() -> bool,
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

    let mut request = client.get(url);

    // Add HF auth header only for HuggingFace URLs
    if is_huggingface_url(url) {
        if let Some(token) = resolve_hf_token(app_hf_token) {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
    }

    let response = request.send().await?;
    let status = response.status();

    // Check for auth errors
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(DownloadError::AuthRequired { status: status.as_u16() });
    }

    // Check for other errors
    let response = response.error_for_status()?;

    let total_size = response.content_length().unwrap_or(0);
    let filename = dest.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();

    let mut file = File::create(dest).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256::new();

    while let Some(chunk) = stream.next().await {
        if cancel_check() {
            // Clean up partial file
            drop(file);
            tokio::fs::remove_file(dest).await.ok();
            return Err(DownloadError::Cancelled);
        }

        let chunk = chunk?;
        file.write_all(&chunk).await?;
        hasher.update(&chunk);

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
        });
    }

    file.flush().await?;
    drop(file);

    // Verify checksum if provided
    if let Some(expected) = expected_sha256 {
        let actual = hex::encode(hasher.finalize());
        if actual != expected {
            tokio::fs::remove_file(dest).await.ok();
            return Err(DownloadError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    Ok(())
}

/// Check if a file exists and optionally verify its checksum
pub async fn verify_cached_file(path: &Path, expected_sha256: Option<&str>) -> bool {
    if !path.exists() {
        return false;
    }

    // If no checksum provided, just check existence
    let Some(expected) = expected_sha256 else {
        return true;
    };

    match tokio::fs::read(path).await {
        Ok(contents) => {
            let mut hasher = Sha256::new();
            hasher.update(&contents);
            let actual = hex::encode(hasher.finalize());
            actual == expected
        }
        Err(_) => false,
    }
}

/// Format bytes as human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1500), "1 KB");
        assert_eq!(format_bytes(1_500_000), "1.4 MB");
        assert_eq!(format_bytes(5_000_000_000), "4.66 GB");
    }

    #[test]
    fn test_is_huggingface_url() {
        assert!(is_huggingface_url("https://huggingface.co/foo/bar/resolve/main/file.gguf"));
        assert!(is_huggingface_url("https://hf.co/foo/bar"));
        assert!(is_huggingface_url("https://cdn-lfs.huggingface.co/something"));
        assert!(!is_huggingface_url("https://github.com/foo/bar"));
        assert!(!is_huggingface_url("https://example.com/huggingface.co/fake"));
    }
}

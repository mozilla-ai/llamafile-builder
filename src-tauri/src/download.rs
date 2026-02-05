use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use thiserror::Error;

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
}

pub struct DownloadProgress {
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    pub percentage: f32,
}

/// Download a file with progress reporting
pub async fn download_file<F>(
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    progress_callback: F,
    cancel_check: impl Fn() -> bool,
) -> Result<(), DownloadError>
where
    F: Fn(DownloadProgress),
{
    let client = Client::builder()
        .user_agent("llamafile-builder/0.1.0")
        .build()?;
    
    let response = client.get(url).send().await?;
    let total_size = response.content_length().unwrap_or(0);
    
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

/// Check if a file exists and has the expected checksum
pub async fn verify_cached_file(path: &Path, expected_sha256: &str) -> bool {
    if !path.exists() {
        return false;
    }
    
    match tokio::fs::read(path).await {
        Ok(contents) => {
            let mut hasher = Sha256::new();
            hasher.update(&contents);
            let actual = hex::encode(hasher.finalize());
            actual == expected_sha256
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
}

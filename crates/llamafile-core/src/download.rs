use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::error::DownloadError;

/// File status information for cache checks.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub exists: bool,
    pub size: u64,
    pub expected_size: Option<u64>,
    pub is_complete: bool,
    pub is_partial: bool,
    pub checksum_valid: Option<bool>,
}

pub(crate) fn is_huggingface_url(url: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host.ends_with("huggingface.co") || host.ends_with("hf.co");
        }
    }
    false
}

pub(crate) fn resolve_hf_token(app_token: Option<&str>) -> Option<String> {
    if let Ok(token) = std::env::var("HF_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    if let Ok(token) = std::env::var("HUGGING_FACE_HUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let hf_token_path = home.join(".cache").join("huggingface").join("token");
        if let Ok(token) = std::fs::read_to_string(&hf_token_path) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    if let Some(token) = app_token {
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    None
}

#[tracing::instrument(skip_all, fields(path = %path.display()))]
pub async fn get_file_info(
    path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    verify_checksum: bool,
) -> FileInfo {
    if !path.exists() {
        tracing::debug!("file does not exist");
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
    let is_partial = expected_size
        .map(|exp| size > 0 && size < exp)
        .unwrap_or(false);

    tracing::debug!(size, is_complete, is_partial, "file info");

    let checksum_valid = if verify_checksum && is_complete {
        if let Some(expected) = expected_sha256 {
            tracing::info!("verifying SHA256 checksum");
            match crate::file_ops::calculate_sha256(&path.to_string_lossy()).await {
                Ok(actual) => {
                    let valid = actual == expected;
                    if valid {
                        tracing::info!("checksum valid");
                    } else {
                        tracing::warn!(expected, actual, "checksum mismatch");
                    }
                    Some(valid)
                }
                Err(e) => {
                    tracing::error!(error = %e, "checksum calculation failed");
                    Some(false)
                }
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

/// Download a file from a URL with resume support, SHA256 verification, and cancellation.
///
/// Progress is reported via `tracing` events at INFO level with fields:
/// `bytes_downloaded`, `bytes_total`, `percentage`, `filename`.
#[tracing::instrument(skip(cancel_flag, hf_token), fields(filename))]
pub async fn download_file(
    url: &str,
    dest: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
    hf_token: Option<&str>,
    resume: bool,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let filename = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    tracing::Span::current().record("filename", &filename.as_str());

    let client = Client::builder()
        .user_agent("llamafile-builder/0.1.0")
        .build()?;

    let existing_size = if resume && dest.exists() {
        tokio::fs::metadata(dest)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
    } else {
        0
    };

    let mut request = client.get(url);

    if is_huggingface_url(url) {
        if let Some(token) = resolve_hf_token(hf_token) {
            tracing::debug!("using HuggingFace auth token");
            request = request.header("Authorization", format!("Bearer {}", token));
        }
    }

    if existing_size > 0 {
        tracing::info!(existing_size, "resuming download");
        request = request.header("Range", format!("bytes={}-", existing_size));
    }

    tracing::info!("starting download");
    let response = request.send().await?;
    let status = response.status();

    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        tracing::error!(status = %status, "authentication required");
        return Err(DownloadError::AuthRequired {
            status: status.as_u16(),
        });
    }

    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT;
    let starting_offset = if resuming { existing_size } else { 0 };

    if existing_size > 0 && !resuming {
        tracing::warn!("server does not support resume, restarting download");
    }

    let response = response.error_for_status()?;

    let content_length = response.content_length().unwrap_or(0);
    let total_size = if resuming {
        starting_offset + content_length
    } else {
        content_length
    };

    tracing::info!(
        total_bytes = total_size,
        resuming,
        "download started"
    );

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
    let mut last_log_time = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::Relaxed) {
            tracing::warn!("download cancelled by user");
            drop(file);
            return Err(DownloadError::Cancelled);
        }

        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        // Log progress at ~2/sec
        let now = std::time::Instant::now();
        if now.duration_since(last_log_time).as_millis() >= 500 {
            last_log_time = now;
            let percentage = if total_size > 0 {
                (downloaded as f64 / total_size as f64) * 100.0
            } else {
                0.0
            };
            tracing::info!(
                bytes_downloaded = downloaded,
                bytes_total = total_size,
                percentage = format!("{:.1}", percentage),
                "downloading"
            );
        }
    }

    // Final progress
    tracing::info!(
        bytes_downloaded = downloaded,
        bytes_total = total_size,
        "download complete"
    );

    file.flush().await?;
    drop(file);

    let final_size = tokio::fs::metadata(dest).await?.len();
    if let Some(expected) = expected_size {
        if final_size != expected {
            tracing::error!(expected, actual = final_size, "size mismatch");
            return Err(DownloadError::SizeMismatch {
                expected,
                actual: final_size,
            });
        }
    }

    if let Some(expected) = expected_sha256 {
        tracing::info!("verifying SHA256 checksum");
        let dest_path = dest.to_path_buf();
        let actual = tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut file = std::fs::File::open(&dest_path).map_err(DownloadError::Io)?;
            let mut hasher = Sha256::new();
            let mut buffer = vec![0u8; 256 * 1024];
            loop {
                let n = file.read(&mut buffer).map_err(DownloadError::Io)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            Ok::<String, DownloadError>(hex::encode(hasher.finalize()))
        })
        .await
        .map_err(|e| DownloadError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))??;

        if actual != expected {
            tracing::error!(expected, actual, "checksum mismatch");
            return Err(DownloadError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        tracing::info!("checksum verified");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_huggingface_url() {
        assert!(is_huggingface_url(
            "https://huggingface.co/foo/bar/resolve/main/file.gguf"
        ));
        assert!(is_huggingface_url("https://hf.co/foo/bar"));
        assert!(is_huggingface_url(
            "https://cdn-lfs.huggingface.co/something"
        ));
        assert!(!is_huggingface_url("https://github.com/foo/bar"));
        assert!(!is_huggingface_url(
            "https://example.com/huggingface.co/fake"
        ));
    }

    #[test]
    fn is_huggingface_url_with_query_string() {
        assert!(is_huggingface_url(
            "https://huggingface.co/foo/bar?download=true"
        ));
    }

    #[test]
    fn is_huggingface_url_http_scheme() {
        assert!(is_huggingface_url("http://huggingface.co/foo"));
    }

    #[test]
    fn is_huggingface_url_no_scheme_returns_false() {
        assert!(!is_huggingface_url("huggingface.co/foo/bar"));
    }

    #[test]
    fn is_huggingface_url_empty_and_garbage() {
        assert!(!is_huggingface_url(""));
        assert!(!is_huggingface_url("not a url"));
        assert!(is_huggingface_url("ftp://huggingface.co/x"));
    }

    #[test]
    fn resolve_hf_token_prefers_hf_token_env() {
        temp_env::with_vars(
            [
                ("HF_TOKEN", Some("from-env")),
                ("HUGGING_FACE_HUB_TOKEN", Some("from-hub")),
            ],
            || {
                assert_eq!(
                    resolve_hf_token(Some("from-app")),
                    Some("from-env".to_string())
                );
            },
        );
    }

    #[test]
    fn resolve_hf_token_falls_back_to_hub_token_env() {
        temp_env::with_vars(
            [
                ("HF_TOKEN", None::<&str>),
                ("HUGGING_FACE_HUB_TOKEN", Some("from-hub")),
            ],
            || {
                let result = resolve_hf_token(Some("from-app"));
                assert!(result.is_some());
                assert_eq!(result.unwrap(), "from-hub");
            },
        );
    }

    #[test]
    fn resolve_hf_token_empty_env_is_skipped() {
        temp_env::with_vars(
            [
                ("HF_TOKEN", Some("")),
                ("HUGGING_FACE_HUB_TOKEN", Some("")),
            ],
            || {
                let result = resolve_hf_token(Some("app-token"));
                assert!(result.is_some());
            },
        );
    }

    // Note: resolve_hf_token with app_token and no-env tests are omitted because
    // the outcome depends on whether ~/.cache/huggingface/token exists on the machine.

    #[tokio::test]
    async fn get_file_info_missing_file() {
        let info = get_file_info(
            Path::new("/tmp/llamafile-test-does-not-exist-12345"),
            Some(100),
            Some("abc"),
            false,
        )
        .await;
        assert!(!info.exists);
        assert_eq!(info.size, 0);
        assert!(!info.is_complete);
        assert!(!info.is_partial);
        assert!(info.checksum_valid.is_none());
    }

    #[tokio::test]
    async fn get_file_info_partial_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"partial data").unwrap();
        let info = get_file_info(tmp.path(), Some(99999), None, false).await;
        assert!(info.exists);
        assert_eq!(info.size, 12);
        assert!(!info.is_complete);
        assert!(info.is_partial);
    }

    #[tokio::test]
    async fn get_file_info_complete_file_no_checksum() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello").unwrap();
        let info = get_file_info(tmp.path(), Some(5), None, false).await;
        assert!(info.exists);
        assert!(info.is_complete);
        assert!(!info.is_partial);
        assert!(info.checksum_valid.is_none());
    }

    #[tokio::test]
    async fn get_file_info_complete_with_valid_checksum() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let data = b"checksum test data";
        std::fs::write(tmp.path(), data).unwrap();
        let expected_sha = hex::encode(Sha256::digest(data));
        let info = get_file_info(
            tmp.path(),
            Some(data.len() as u64),
            Some(&expected_sha),
            true,
        )
        .await;
        assert!(info.is_complete);
        assert_eq!(info.checksum_valid, Some(true));
    }

    #[tokio::test]
    async fn get_file_info_complete_with_invalid_checksum() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let data = b"data";
        std::fs::write(tmp.path(), data).unwrap();
        let info = get_file_info(
            tmp.path(),
            Some(data.len() as u64),
            Some("0000000000000000000000000000000000000000000000000000000000000000"),
            true,
        )
        .await;
        assert!(info.is_complete);
        assert_eq!(info.checksum_valid, Some(false));
    }

    #[tokio::test]
    async fn get_file_info_skip_checksum_when_not_requested() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let data = b"skip";
        std::fs::write(tmp.path(), data).unwrap();
        let info = get_file_info(
            tmp.path(),
            Some(data.len() as u64),
            Some("anything"),
            false,
        )
        .await;
        assert!(info.is_complete);
        assert!(info.checksum_valid.is_none());
    }
}

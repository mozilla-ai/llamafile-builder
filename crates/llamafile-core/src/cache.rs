use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::download;
use crate::error::DownloadError;
use crate::models::{Component, ComponentRole, ComponentSource, DownloadProgress};

/// Status of a cached component.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheStatus {
    /// File exists and checksum matches (or no checksum to check).
    Valid(PathBuf),
    /// File exists but is smaller than expected (can resume).
    Partial {
        path: PathBuf,
        bytes_downloaded: u64,
    },
    /// Not cached.
    Missing,
}

/// Manages downloaded files for runtimes, GPU libraries, and other non-HF components.
/// HuggingFace model downloads go through hf-hub's shared cache at
/// `~/.cache/huggingface/hub/` for interoperability with llama.cpp and other tools.
pub struct Cache {
    base_dir: PathBuf,
}

impl Cache {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Directory for cached llamafile runtime binaries.
    pub fn runtime_dir(&self) -> PathBuf {
        self.base_dir.join("bin")
    }

    /// Directory for cached GPU libraries, versioned.
    pub fn gpu_lib_dir(&self, version: &str) -> PathBuf {
        let safe_version: String = version
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.base_dir.join("v").join(safe_version)
    }

    /// Directory for cached configuration files.
    pub fn config_dir(&self) -> PathBuf {
        self.base_dir.join("config")
    }

    /// Determine the local path where a component would be cached.
    pub fn path_for(&self, component: &Component) -> Option<PathBuf> {
        match &component.source {
            ComponentSource::Url(url) => {
                let filename = crate::models::get_filename_from_url(url);
                let dir = match &component.role {
                    ComponentRole::Runtime | ComponentRole::Zipalign => self.runtime_dir(),
                    ComponentRole::GpuLib { .. } => {
                        // GPU libs need a version dir, but we don't have it here.
                        // The builder will use gpu_lib_dir() directly.
                        self.base_dir.join("gpu")
                    }
                    _ => self.base_dir.join("downloads"),
                };
                Some(dir.join(filename))
            }
            ComponentSource::HuggingFace {
                repo_id, filename, ..
            } => {
                // For HF downloads, we store in our own cache but the builder
                // should prefer using hf-hub's native cache for interop.
                // This path is a fallback for when we download manually.
                let safe_repo = repo_id.replace('/', "--");
                Some(
                    self.base_dir
                        .join("hf")
                        .join(safe_repo)
                        .join(filename),
                )
            }
            ComponentSource::Local(path) => Some(path.clone()),
            ComponentSource::Generated => None,
        }
    }

    /// Check if a component is already cached without downloading.
    pub fn check(&self, component: &Component) -> CacheStatus {
        match &component.source {
            ComponentSource::Local(path) => {
                if path.exists() {
                    CacheStatus::Valid(path.clone())
                } else {
                    CacheStatus::Missing
                }
            }
            ComponentSource::Generated => CacheStatus::Valid(PathBuf::new()),
            ComponentSource::Url(_) | ComponentSource::HuggingFace { .. } => {
                let path = match self.path_for(component) {
                    Some(p) => p,
                    None => return CacheStatus::Missing,
                };
                if !path.exists() {
                    return CacheStatus::Missing;
                }
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if let Some(expected) = component.expected_size {
                    if size == expected {
                        CacheStatus::Valid(path)
                    } else if size > 0 && size < expected {
                        CacheStatus::Partial {
                            path,
                            bytes_downloaded: size,
                        }
                    } else {
                        CacheStatus::Missing
                    }
                } else if size > 0 {
                    CacheStatus::Valid(path)
                } else {
                    CacheStatus::Missing
                }
            }
        }
    }

    /// Ensure a component is available locally. Downloads if needed.
    ///
    /// For `ComponentSource::Url`: downloads with resume support to our cache.
    /// For `ComponentSource::HuggingFace`: resolves the download URL via hf-hub,
    ///   then downloads with our custom downloader for progress reporting.
    /// For `ComponentSource::Local`: verifies the file exists.
    /// For `ComponentSource::Generated`: no-op, returns empty path.
    pub async fn ensure(
        &self,
        component: &Component,
        hf_token: Option<&str>,
        progress_tx: mpsc::Sender<DownloadProgress>,
        cancel_flag: Arc<AtomicBool>,
    ) -> std::result::Result<PathBuf, DownloadError> {
        match &component.source {
            ComponentSource::Local(path) => {
                if path.exists() {
                    Ok(path.clone())
                } else {
                    Err(DownloadError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Local file not found: {}", path.display()),
                    )))
                }
            }
            ComponentSource::Generated => Ok(PathBuf::new()),
            ComponentSource::Url(url) => {
                let dest = self
                    .path_for(component)
                    .expect("URL components always have a cache path");

                // Check if already cached and complete
                if dest.exists() {
                    if let Some(expected) = component.expected_size {
                        let size = tokio::fs::metadata(&dest)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        if size == expected {
                            return Ok(dest);
                        }
                    }
                }

                let resume = dest.exists();
                download::download_file(
                    url,
                    &dest,
                    component.expected_size,
                    component.expected_sha256.as_deref(),
                    hf_token,
                    resume,
                    progress_tx,
                    cancel_flag,
                )
                .await?;
                Ok(dest)
            }
            ComponentSource::HuggingFace {
                repo_id, filename, ..
            } => {
                // Use hf-hub to resolve the download URL (with auth),
                // then download with our custom downloader for progress.
                let api = hf_hub::api::tokio::ApiBuilder::new()
                    .with_progress(false)
                    .build()
                    .map_err(|e| {
                        DownloadError::Io(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to create HF API client: {e}"),
                        ))
                    })?;

                let repo = api.model(repo_id.clone());

                // Try to get the file from the HF cache first.
                // hf-hub's get() downloads if not cached, but we want to use
                // our own downloader for progress reporting. So we check the
                // cache first, and only if missing, resolve the URL and download.
                let cache_path = repo.get(filename).await;
                match cache_path {
                    Ok(path) => Ok(path),
                    Err(_) => {
                        // If hf-hub can't get it (e.g. no network, auth needed),
                        // fall back to direct URL download with our downloader.
                        let url = format!(
                            "https://huggingface.co/{}/resolve/main/{}",
                            repo_id, filename
                        );
                        let dest = self
                            .path_for(component)
                            .expect("HF components always have a cache path");
                        let resume = dest.exists();
                        download::download_file(
                            &url,
                            &dest,
                            component.expected_size,
                            component.expected_sha256.as_deref(),
                            hf_token,
                            resume,
                            progress_tx,
                            cancel_flag,
                        )
                        .await?;
                        Ok(dest)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ComponentRole, ComponentSource};

    #[test]
    fn cache_dirs_are_under_base() {
        let cache = Cache::new(PathBuf::from("/tmp/test-cache"));
        assert_eq!(cache.runtime_dir(), PathBuf::from("/tmp/test-cache/bin"));
        assert_eq!(
            cache.gpu_lib_dir("0.10.0"),
            PathBuf::from("/tmp/test-cache/v/0.10.0")
        );
        assert_eq!(
            cache.config_dir(),
            PathBuf::from("/tmp/test-cache/config")
        );
    }

    #[test]
    fn gpu_lib_dir_sanitizes_version() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        assert_eq!(
            cache.gpu_lib_dir("0.10.0 - alpha/beta"),
            PathBuf::from("/tmp/c/v/0.10.0_-_alpha_beta")
        );
    }

    #[test]
    fn path_for_url_component() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/llamafile_0.10.0".to_string()),
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        let path = cache.path_for(&component).unwrap();
        assert_eq!(path, PathBuf::from("/tmp/c/bin/llamafile_0.10.0"));
    }

    #[test]
    fn path_for_hf_component() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::Weights,
            source: ComponentSource::HuggingFace {
                repo_id: "Qwen/Qwen3-0.6B-GGUF".to_string(),
                filename: "qwen3-0.6b-q8_0.gguf".to_string(),
            },
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        let path = cache.path_for(&component).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/c/hf/Qwen--Qwen3-0.6B-GGUF/qwen3-0.6b-q8_0.gguf")
        );
    }

    #[test]
    fn path_for_local_returns_original_path() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::Weights,
            source: ComponentSource::Local(PathBuf::from("/my/model.gguf")),
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        assert_eq!(
            cache.path_for(&component).unwrap(),
            PathBuf::from("/my/model.gguf")
        );
    }

    #[test]
    fn path_for_generated_returns_none() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::ArgsFile,
            source: ComponentSource::Generated,
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        assert!(cache.path_for(&component).is_none());
    }

    #[test]
    fn check_local_existing_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"data").unwrap();
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::Weights,
            source: ComponentSource::Local(tmp.path().to_path_buf()),
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        assert_eq!(
            cache.check(&component),
            CacheStatus::Valid(tmp.path().to_path_buf())
        );
    }

    #[test]
    fn check_local_missing_file() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::Weights,
            source: ComponentSource::Local(PathBuf::from("/nonexistent/file.gguf")),
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        assert_eq!(cache.check(&component), CacheStatus::Missing);
    }

    #[test]
    fn check_generated_is_always_valid() {
        let cache = Cache::new(PathBuf::from("/tmp/c"));
        let component = Component {
            role: ComponentRole::ArgsFile,
            source: ComponentSource::Generated,
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        };
        assert_eq!(
            cache.check(&component),
            CacheStatus::Valid(PathBuf::new())
        );
    }

    #[test]
    fn check_url_partial_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        // Create a partial file in the cache
        let bin_dir = cache.runtime_dir();
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("llamafile_0.10"), b"partial").unwrap();

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/llamafile_0.10".to_string()),
            zip_path: String::new(),
            expected_size: Some(1_000_000),
            expected_sha256: None,
        };
        match cache.check(&component) {
            CacheStatus::Partial {
                bytes_downloaded, ..
            } => {
                assert_eq!(bytes_downloaded, 7); // "partial" is 7 bytes
            }
            other => panic!("Expected Partial, got {:?}", other),
        }
    }

    #[test]
    fn check_url_complete_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let bin_dir = cache.runtime_dir();
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("llamafile_0.10"), b"12345").unwrap();

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/llamafile_0.10".to_string()),
            zip_path: String::new(),
            expected_size: Some(5), // matches "12345"
            expected_sha256: None,
        };
        match cache.check(&component) {
            CacheStatus::Valid(path) => {
                assert!(path.ends_with("llamafile_0.10"));
            }
            other => panic!("Expected Valid, got {:?}", other),
        }
    }

    #[test]
    fn check_url_missing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/llamafile_0.10".to_string()),
            zip_path: String::new(),
            expected_size: Some(100),
            expected_sha256: None,
        };
        assert_eq!(cache.check(&component), CacheStatus::Missing);
    }
}

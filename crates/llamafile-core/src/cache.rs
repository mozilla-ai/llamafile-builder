use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::download;
use crate::error::DownloadError;
use crate::models::{Component, ComponentRole, ComponentSource};

/// Status of a cached component.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheStatus {
    /// File exists and size matches (or no expected size to check).
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
    hf_token: Option<String>,
}

impl Cache {
    pub fn new(base_dir: PathBuf) -> Self {
        Self {
            base_dir,
            hf_token: None,
        }
    }

    pub fn with_hf_token(mut self, token: String) -> Self {
        self.hf_token = Some(token);
        self
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
                    ComponentRole::GpuLib { .. } => self.base_dir.join("gpu"),
                    _ => self.base_dir.join("downloads"),
                };
                Some(dir.join(filename))
            }
            ComponentSource::HuggingFace {
                repo_id, filename, ..
            } => {
                let safe_repo = repo_id.replace('/', "--");
                Some(self.base_dir.join("hf").join(safe_repo).join(filename))
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
                    tracing::debug!(path = %path.display(), "local file exists");
                    CacheStatus::Valid(path.clone())
                } else {
                    tracing::debug!(path = %path.display(), "local file missing");
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
                    tracing::debug!(path = %path.display(), "not cached");
                    return CacheStatus::Missing;
                }
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if let Some(expected) = component.expected_size {
                    if size == expected {
                        tracing::debug!(path = %path.display(), size, "cached and complete");
                        CacheStatus::Valid(path)
                    } else if size > 0 && size < expected {
                        tracing::debug!(path = %path.display(), size, expected, "partial download");
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
    /// For `Url`: downloads with resume to our cache directory.
    /// For `HuggingFace`: tries hf-hub first (shared cache), falls back to direct download.
    /// For `Local`: verifies the file exists.
    /// For `Generated`: no-op, returns empty path.
    pub async fn ensure(
        &self,
        component: &Component,
        cancel_flag: Arc<AtomicBool>,
    ) -> std::result::Result<PathBuf, DownloadError> {
        match &component.source {
            ComponentSource::Local(path) => {
                if path.exists() {
                    tracing::info!(path = %path.display(), "using local file");
                    Ok(path.clone())
                } else {
                    tracing::error!(path = %path.display(), "local file not found");
                    Err(DownloadError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("Local file not found: {}", path.display()),
                    )))
                }
            }
            ComponentSource::Generated => {
                tracing::debug!("generated component, no download needed");
                Ok(PathBuf::new())
            }
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
                            tracing::info!(path = %dest.display(), "already cached");
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
                    self.hf_token.as_deref(),
                    resume,
                    cancel_flag,
                )
                .await?;
                Ok(dest)
            }
            ComponentSource::HuggingFace {
                repo_id, filename, ..
            } => {
                tracing::info!(repo_id, filename, "resolving HuggingFace model");

                // Try hf-hub first (uses standard ~/.cache/huggingface/hub/ layout)
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
                match repo.get(filename).await {
                    Ok(path) => {
                        tracing::info!(path = %path.display(), "resolved from HF cache");
                        Ok(path)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "hf-hub failed, falling back to direct download");
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
                            self.hf_token.as_deref(),
                            resume,
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
                assert_eq!(bytes_downloaded, 7);
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
            expected_size: Some(5),
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

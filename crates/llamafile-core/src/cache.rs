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
}

impl Cache {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Directory for cached llamafile runtime binaries.
    pub fn runtime_dir(&self) -> PathBuf {
        self.base_dir.join("bin")
    }

    /// Determine the local path where a component would be cached.
    /// For URL sources, returns a path in our cache. For HuggingFace sources,
    /// checks hf-hub's shared cache. For Local sources, returns the path as-is.
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
                let hf_cache = hf_hub::Cache::from_env();
                let hf_repo = hf_cache.model(repo_id.clone());
                hf_repo.get(filename)
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
            ComponentSource::HuggingFace {
                repo_id, filename, ..
            } => {
                let hf_cache = hf_hub::Cache::from_env();
                let hf_repo = hf_cache.model(repo_id.clone());
                match hf_repo.get(filename) {
                    Some(path) => {
                        tracing::debug!(path = %path.display(), "in HF cache");
                        CacheStatus::Valid(path)
                    }
                    None => CacheStatus::Missing,
                }
            }
            ComponentSource::Url(_) => {
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
                    None,
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
                self.download_hf(repo_id, filename).await
            }
        }
    }

    /// Download a file via hf-hub with local cache check first.
    ///
    /// hf-hub's `download_with_progress` always re-downloads (it doesn't check
    /// its own cache), so we use `hf_hub::Cache` for the local check and only
    /// call the network path on a cache miss.
    async fn download_hf(
        &self,
        repo_id: &str,
        filename: &str,
    ) -> std::result::Result<PathBuf, DownloadError> {
        // Check hf-hub's own cache first (pure filesystem, no network)
        let hf_cache = hf_hub::Cache::from_env();
        let hf_repo = hf_cache.model(repo_id.to_string());
        if let Some(path) = hf_repo.get(filename) {
            tracing::info!(path = %path.display(), "already in HF cache");
            return Ok(path);
        }

        // Cache miss — download with progress
        let api = hf_hub::api::tokio::ApiBuilder::new()
            .with_progress(false)
            .build()
            .map_err(|e| {
                DownloadError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create HF API client: {e}"),
                ))
            })?;
        let repo = api.model(repo_id.to_string());
        let progress = TracingProgress::new(filename.to_string());
        match repo.download_with_progress(filename, progress).await {
            Ok(path) => {
                tracing::info!(path = %path.display(), "download complete");
                Ok(path)
            }
            Err(e) => {
                let err_msg = e.to_string();
                // Surface lock file path so the user can remove it if stale
                if let Some(lock_path) = extract_lock_path(&err_msg) {
                    tracing::error!(
                        "HF download blocked by lock file: {}\n\
                         If no other download is running, remove it with:\n  \
                         rm {}", lock_path.display(), lock_path.display()
                    );
                }
                Err(DownloadError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("HF download failed: {err_msg}"),
                )))
            }
        }
    }
}

/// Extract a lock file path from an hf-hub error message like
/// "Lock acquisition failed: /path/to/file.lock"
fn extract_lock_path(err_msg: &str) -> Option<PathBuf> {
    let prefix = "Lock acquisition failed: ";
    let start = err_msg.find(prefix)? + prefix.len();
    let path_str = err_msg[start..].trim();
    if path_str.ends_with(".lock") {
        Some(PathBuf::from(path_str))
    } else {
        None
    }
}

/// Progress reporter that emits tracing events during HF downloads.
/// Uses a shared atomic counter so all clones (one per parallel chunk)
/// contribute to the same total.
#[derive(Clone)]
struct TracingProgress {
    filename: String,
    total: u64,
    shared_downloaded: Arc<std::sync::atomic::AtomicU64>,
    last_reported_pct: Arc<std::sync::atomic::AtomicU32>,
    last_log: std::time::Instant,
}

impl TracingProgress {
    fn new(filename: String) -> Self {
        Self {
            filename,
            total: 0,
            shared_downloaded: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_reported_pct: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            last_log: std::time::Instant::now(),
        }
    }
}

impl hf_hub::api::tokio::Progress for TracingProgress {
    async fn init(&mut self, size: usize, filename: &str) {
        self.total = size as u64;
        self.shared_downloaded.store(0, std::sync::atomic::Ordering::Relaxed);
        self.filename = filename.to_string();
        tracing::info!(
            filename = %self.filename,
            bytes_total = self.total,
            "download started"
        );
    }

    async fn update(&mut self, size: usize) {
        let downloaded = self.shared_downloaded.fetch_add(size as u64, std::sync::atomic::Ordering::Relaxed) + size as u64;
        let now = std::time::Instant::now();
        if now.duration_since(self.last_log).as_millis() >= 500 {
            self.last_log = now;
            let percentage = if self.total > 0 {
                (downloaded as f64 / self.total as f64) * 100.0
            } else {
                0.0
            };
            tracing::debug!(
                filename = %self.filename,
                bytes_downloaded = downloaded,
                bytes_total = self.total,
                percentage = format!("{:.1}", percentage),
                "downloading"
            );
            // INFO at 10% milestones -- use atomic CAS so only one clone reports each step
            let pct_10 = (percentage as u32) / 10 * 10; // round down to nearest 10
            if pct_10 > 0 {
                let prev = self.last_reported_pct.load(std::sync::atomic::Ordering::Relaxed);
                if pct_10 > prev {
                    if self.last_reported_pct.compare_exchange(
                        prev, pct_10,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed,
                    ).is_ok() {
                        tracing::info!(
                            filename = %self.filename,
                            percentage = format!("{:.0}%", percentage),
                            "downloading"
                        );
                    }
                }
            }
        }
    }

    async fn finish(&mut self) {
        tracing::info!(
            filename = %self.filename,
            bytes_total = self.total,
            "download complete"
        );
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

//! Pre-flight disk-space check.
//!
//! Before [`Builder::build`](crate::Builder::build) starts downloading
//! components, this module estimates how many bytes need to land at each
//! destination on disk and verifies that enough free space is available.
//! The goal is to fail fast with a clear error message instead of dying
//! mid-download with an `ENOSPC` after multi-GB transfers have already
//! happened.
//!
//! Three destinations are tracked:
//!
//! - **Cache** (`Cache::base_dir()`): runtime binaries, zipalign, GPU libs
//!   downloaded over plain HTTP.
//! - **HuggingFace cache** (`~/.cache/huggingface/hub/`): model weights and
//!   mmproj files downloaded via `hf-hub`. Shared with `llama.cpp`, etc.
//! - **Output**: the parent directory of the final `.llamafile`. The output
//!   ends up roughly the size of the runtime plus all bundled files, since
//!   `zipalign` appends content uncompressed.
//!
//! Components that have no `expected_size`, components that are already
//! cached, and `Local` / `Generated` sources do not contribute to the
//! requirement. The requirement therefore drops to zero for an offline
//! rebuild from a fully populated cache.

use std::path::{Path, PathBuf};

use crate::cache::{Cache, CacheStatus};
use crate::models::{format_size, Component, ComponentRole, ComponentSource};

/// Why a particular block of disk space is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// llamafile-builder's own cache (runtimes, zipalign, GPU libs).
    Cache,
    /// HuggingFace shared cache (`~/.cache/huggingface/hub/`).
    HuggingFaceCache,
    /// The final output `.llamafile` location.
    Output,
}

impl Purpose {
    pub fn label(self) -> &'static str {
        match self {
            Purpose::Cache => "cache",
            Purpose::HuggingFaceCache => "huggingface cache",
            Purpose::Output => "output",
        }
    }
}

/// One destination's slice of the total requirement, before destinations are
/// grouped by filesystem.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskRequirement {
    /// A directory or path the bytes will land near. Must exist (or have an
    /// existing ancestor) for the check to query free space against it.
    pub destination: PathBuf,
    /// Bytes that need to be written at this destination.
    pub bytes: u64,
    pub purpose: Purpose,
}

/// Result of a disk-space query for a single destination.
#[derive(Debug, Clone, PartialEq)]
pub struct DiskSpaceReport {
    /// The path the check was performed against (the nearest existing ancestor
    /// of the original `destination`).
    pub checked_path: PathBuf,
    pub bytes_required: u64,
    pub bytes_available: u64,
    pub purposes: Vec<Purpose>,
}

impl DiskSpaceReport {
    pub fn is_sufficient(&self) -> bool {
        self.bytes_available >= self.bytes_required
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiskSpaceError {
    #[error(
        "Insufficient disk space at {path}: need {required_h} ({required} bytes) for {purposes}, only {available_h} ({available} bytes) available",
        path = path.display(),
        required_h = format_size(*required),
        available_h = format_size(*available),
        purposes = purposes.join(", "),
    )]
    Insufficient {
        path: PathBuf,
        required: u64,
        available: u64,
        purposes: Vec<String>,
    },

    #[error("Could not query free space at {path}: {source}", path = path.display())]
    Query {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Estimate the disk-space requirements for a build with the given components,
/// cache, and output path. Components with no expected size or that are
/// already cached contribute nothing.
///
/// The output destination is sized as the sum of all bundled components'
/// expected sizes (runtime + weights + mmproj + GPU libs). `zipalign` appends
/// uncompressed, so this is a close approximation rather than a hard cap.
pub fn estimate(
    components: &[Component],
    cache: &Cache,
    output_path: &Path,
) -> Vec<DiskRequirement> {
    let mut requirements = Vec::new();

    let mut cache_bytes: u64 = 0;
    let mut hf_bytes: u64 = 0;
    let mut output_bytes: u64 = 0;

    for component in components {
        // Bundle output size: every component that ends up inside the
        // llamafile contributes to the final file size, regardless of where
        // its bytes are sourced from. Generated content (.args) is tiny and
        // ignored. The Zipalign component is the build tool itself, not
        // bundled.
        let bundled = !matches!(
            component.role,
            ComponentRole::Zipalign | ComponentRole::ArgsFile | ComponentRole::ArgsPrompt
        );
        if bundled {
            // Try expected_size first (download metadata); fall back to the
            // file size on disk for Local sources.
            if let Some(size) = component_size_for_output(component) {
                output_bytes = output_bytes.saturating_add(size);
            }
        }

        // Download size: only for not-yet-cached URL/HF sources with a known
        // expected_size. For partial URL downloads, only the remainder will
        // be fetched (Cache::ensure resumes from the existing file).
        let Some(expected) = component.expected_size else {
            continue;
        };
        if expected == 0 {
            continue;
        }
        let bytes_to_fetch = match cache.check(component) {
            CacheStatus::Valid(_) => continue,
            CacheStatus::Partial { bytes_downloaded, .. } => {
                expected.saturating_sub(bytes_downloaded)
            }
            CacheStatus::Missing => expected,
        };
        if bytes_to_fetch == 0 {
            continue;
        }
        match &component.source {
            ComponentSource::Url(_) => {
                cache_bytes = cache_bytes.saturating_add(bytes_to_fetch);
            }
            ComponentSource::HuggingFace { .. } => {
                hf_bytes = hf_bytes.saturating_add(bytes_to_fetch);
            }
            ComponentSource::Local(_) | ComponentSource::Generated => {}
        }
    }

    if cache_bytes > 0 {
        requirements.push(DiskRequirement {
            destination: cache.base_dir().to_path_buf(),
            bytes: cache_bytes,
            purpose: Purpose::Cache,
        });
    }
    if hf_bytes > 0 {
        if let Some(hf_dir) = hf_cache_dir() {
            requirements.push(DiskRequirement {
                destination: hf_dir,
                bytes: hf_bytes,
                purpose: Purpose::HuggingFaceCache,
            });
        }
    }
    if output_bytes > 0 {
        requirements.push(DiskRequirement {
            destination: output_path.to_path_buf(),
            bytes: output_bytes,
            purpose: Purpose::Output,
        });
    }

    requirements
}

fn component_size_for_output(component: &Component) -> Option<u64> {
    if let Some(size) = component.expected_size {
        return Some(size);
    }
    if let ComponentSource::Local(path) = &component.source {
        return std::fs::metadata(path).ok().map(|m| m.len());
    }
    None
}

/// Resolve the HuggingFace shared cache directory. Must match the directory
/// that the actual download path (`Cache::download_hf` → `hf_hub::Cache::from_env`)
/// will land bytes in, otherwise the pre-check probes a different filesystem
/// than the download will.
///
/// hf-hub 0.4 only honours `HF_HOME`, falling back to
/// `<home>/.cache/huggingface/hub/`. It does **not** look at
/// `HUGGINGFACE_HUB_CACHE`; we mirror that here intentionally.
fn hf_cache_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("HF_HOME") {
        return Some(PathBuf::from(p).join("hub"));
    }
    let home = dirs::home_dir()?;
    Some(home.join(".cache").join("huggingface").join("hub"))
}

/// Run a disk-space pre-check against real filesystems.
///
/// Group requirements by their filesystem so two destinations on the same
/// volume don't appear to each have the full available space. Returns a
/// per-filesystem report on success. The first filesystem with insufficient
/// space short-circuits with [`DiskSpaceError::Insufficient`].
pub fn check(requirements: &[DiskRequirement]) -> Result<Vec<DiskSpaceReport>, DiskSpaceError> {
    check_with(requirements, &|p| fs4::available_space(p))
}

/// Variant of [`check`] that takes a custom `available_space` function. Used
/// by tests in this module to simulate a tight filesystem without writing to
/// real disks. Not exposed publicly — callers should use [`check`].
fn check_with(
    requirements: &[DiskRequirement],
    available_space_fn: &dyn Fn(&Path) -> std::io::Result<u64>,
) -> Result<Vec<DiskSpaceReport>, DiskSpaceError> {
    // Each entry pairs the grouping key (filesystem id, with canonical path
    // as fallback when the platform can't report a device id) with its
    // accumulated report.
    let mut grouped: Vec<(GroupKey, DiskSpaceReport)> = Vec::new();

    for req in requirements {
        let probe = nearest_existing_ancestor(&req.destination).ok_or_else(|| {
            DiskSpaceError::Query {
                path: req.destination.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no existing ancestor directory",
                ),
            }
        })?;
        let canonical = std::fs::canonicalize(&probe).unwrap_or(probe.clone());
        let key = group_key_for(&canonical);

        if let Some((_, existing)) = grouped.iter_mut().find(|(k, _)| *k == key) {
            existing.bytes_required = existing.bytes_required.saturating_add(req.bytes);
            if !existing.purposes.contains(&req.purpose) {
                existing.purposes.push(req.purpose);
            }
        } else {
            let available =
                available_space_fn(&canonical).map_err(|e| DiskSpaceError::Query {
                    path: canonical.clone(),
                    source: e,
                })?;
            grouped.push((
                key,
                DiskSpaceReport {
                    checked_path: canonical,
                    bytes_required: req.bytes,
                    bytes_available: available,
                    purposes: vec![req.purpose],
                },
            ));
        }
    }

    for (_, report) in &grouped {
        if !report.is_sufficient() {
            return Err(DiskSpaceError::Insufficient {
                path: report.checked_path.clone(),
                required: report.bytes_required,
                available: report.bytes_available,
                purposes: report
                    .purposes
                    .iter()
                    .map(|p| p.label().to_string())
                    .collect(),
            });
        }
    }

    Ok(grouped.into_iter().map(|(_, r)| r).collect())
}

/// Identity of the filesystem hosting a path. The first variant is preferred
/// — on Unix `dev()` returns a stable id that's identical for any two paths
/// on the same volume; on Windows we approximate that with the volume prefix
/// of the canonical path (e.g. `\\?\C:`), which is shared by every path on
/// the same drive letter or UNC share. The second is a fallback for
/// platforms where neither is available (or the metadata/canonicalize call
/// fails); in that case we degrade to canonical-path equality, which is the
/// pre-fix behaviour.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupKey {
    Filesystem(u64),
    CanonicalPath(PathBuf),
}

fn group_key_for(path: &Path) -> GroupKey {
    match filesystem_id(path) {
        Some(id) => GroupKey::Filesystem(id),
        None => GroupKey::CanonicalPath(path.to_path_buf()),
    }
}

#[cfg(unix)]
fn filesystem_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| m.dev())
}

// `std::os::windows::fs::MetadataExt::volume_serial_number` would be the
// direct analogue of `dev()` here, but it's gated behind the unstable
// `windows_by_handle` feature (rust-lang/rust#63010) and therefore unusable
// on stable. Hash the volume prefix of the canonical path instead — every
// path on the same drive letter or UNC share canonicalizes to the same
// prefix, so this gives the same grouping in the common case without an
// FFI dependency.
#[cfg(windows)]
fn filesystem_id(path: &Path) -> Option<u64> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::path::Component;

    let canonical = std::fs::canonicalize(path).ok()?;
    match canonical.components().next()? {
        Component::Prefix(p) => {
            let mut hasher = DefaultHasher::new();
            p.as_os_str().hash(&mut hasher);
            Some(hasher.finish())
        }
        _ => None,
    }
}

#[cfg(not(any(unix, windows)))]
fn filesystem_id(_path: &Path) -> Option<u64> {
    None
}

/// Walk up the path until we find a component that exists on disk. Used so
/// that the disk-space check works even when the cache or output directory
/// hasn't been created yet — its parent (or some grand-parent) will still
/// resolve to a real filesystem. `pop()` returns false at the root, which
/// terminates the loop.
fn nearest_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut p = path.to_path_buf();
    loop {
        if p.exists() {
            return Some(p);
        }
        if !p.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ComponentRole, ComponentSource, GpuAccel, Os};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn url_component(role: ComponentRole, size: Option<u64>) -> Component {
        Component {
            role,
            source: ComponentSource::Url("https://example.com/file".to_string()),
            zip_path: String::new(),
            expected_size: size,
            expected_sha256: None,
        }
    }

    fn hf_component(role: ComponentRole, filename: &str, size: Option<u64>) -> Component {
        Component {
            role,
            source: ComponentSource::HuggingFace {
                repo_id: "fake/repo".to_string(),
                filename: filename.to_string(),
            },
            zip_path: format!("/zip/{}", filename),
            expected_size: size,
            expected_sha256: None,
        }
    }

    fn local_component(role: ComponentRole, path: PathBuf, size: Option<u64>) -> Component {
        Component {
            role,
            source: ComponentSource::Local(path),
            zip_path: String::new(),
            expected_size: size,
            expected_sha256: None,
        }
    }

    fn generated_component(role: ComponentRole) -> Component {
        Component {
            role,
            source: ComponentSource::Generated,
            zip_path: String::new(),
            expected_size: None,
            expected_sha256: None,
        }
    }

    // ── estimate ──

    #[test]
    fn estimate_url_runtime_charges_cache_and_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());
        let output = tmp.path().join("out.llamafile");

        let components = vec![
            url_component(ComponentRole::Runtime, Some(50_000_000)),
        ];

        let reqs = estimate(&components, &cache, &output);

        // Cache + Output (Runtime is bundled into output as the base file)
        let cache_req = reqs.iter().find(|r| r.purpose == Purpose::Cache).unwrap();
        assert_eq!(cache_req.bytes, 50_000_000);
        let out_req = reqs.iter().find(|r| r.purpose == Purpose::Output).unwrap();
        assert_eq!(out_req.bytes, 50_000_000);
    }

    #[test]
    fn estimate_hf_weights_charges_hf_cache_and_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());
        let output = tmp.path().join("out.llamafile");

        let components = vec![
            hf_component(ComponentRole::Weights, "model.gguf", Some(2_000_000_000)),
        ];

        let reqs = estimate(&components, &cache, &output);

        assert!(reqs.iter().any(|r| r.purpose == Purpose::HuggingFaceCache
            && r.bytes == 2_000_000_000));
        assert!(
            reqs.iter().any(|r| r.purpose == Purpose::Output && r.bytes == 2_000_000_000)
        );
        // No URL component, no cache requirement
        assert!(reqs.iter().all(|r| r.purpose != Purpose::Cache));
    }

    #[test]
    fn estimate_partial_url_component_charges_only_the_remainder() {
        // A previously-interrupted runtime download leaves a partial file in
        // the cache. Cache::ensure resumes and only fetches the missing
        // bytes, so the pre-check must reserve only that remainder.
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let bin_dir = cache.runtime_dir();
        std::fs::create_dir_all(&bin_dir).unwrap();
        // 200_000 of 1_000_000 already on disk
        std::fs::write(bin_dir.join("file"), vec![0u8; 200_000]).unwrap();

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/file".to_string()),
            zip_path: String::new(),
            expected_size: Some(1_000_000),
            expected_sha256: None,
        };

        let reqs = estimate(&[component], &cache, &tmp.path().join("out.llamafile"));
        let cache_req = reqs
            .iter()
            .find(|r| r.purpose == Purpose::Cache)
            .expect("partial download still needs cache space");
        assert_eq!(
            cache_req.bytes, 800_000,
            "expected to reserve only the remainder, not the full size"
        );
        // Output still uses the full expected size (the bundled file is
        // complete by the time output is written).
        let out_req = reqs.iter().find(|r| r.purpose == Purpose::Output).unwrap();
        assert_eq!(out_req.bytes, 1_000_000);
    }

    #[test]
    fn estimate_skips_already_cached_url_component_for_cache_but_keeps_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        // Pre-populate the cache: write a file matching what the URL
        // component's path_for would resolve to.
        let bin_dir = cache.runtime_dir();
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("file"), vec![0u8; 100]).unwrap();

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url("https://example.com/file".to_string()),
            zip_path: String::new(),
            expected_size: Some(100),
            expected_sha256: None,
        };

        let reqs = estimate(&[component], &cache, &tmp.path().join("out.llamafile"));

        // No cache requirement (already complete on disk)
        assert!(reqs.iter().all(|r| r.purpose != Purpose::Cache));
        // But output requirement remains: the runtime is still bundled
        assert!(reqs.iter().any(|r| r.purpose == Purpose::Output && r.bytes == 100));
    }

    #[test]
    fn estimate_local_source_contributes_only_to_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let model_path = tmp.path().join("model.gguf");
        std::fs::write(&model_path, vec![0u8; 1234]).unwrap();

        let components = vec![
            local_component(ComponentRole::Weights, model_path.clone(), None),
        ];
        let reqs = estimate(&components, &cache, &tmp.path().join("out.llamafile"));

        // No cache or HF requirement for Local source
        assert!(reqs.iter().all(|r| r.purpose != Purpose::Cache
            && r.purpose != Purpose::HuggingFaceCache));
        // Output uses on-disk size
        let out = reqs.iter().find(|r| r.purpose == Purpose::Output).unwrap();
        assert_eq!(out.bytes, 1234);
    }

    #[test]
    fn estimate_generated_components_contribute_nothing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let components = vec![
            generated_component(ComponentRole::ArgsFile),
            generated_component(ComponentRole::ArgsPrompt),
        ];
        let reqs = estimate(&components, &cache, &tmp.path().join("out.llamafile"));
        assert!(reqs.is_empty(), "got {:?}", reqs);
    }

    #[test]
    fn estimate_zipalign_charges_cache_only_not_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let components = vec![
            url_component(ComponentRole::Zipalign, Some(5_000_000)),
        ];
        let reqs = estimate(&components, &cache, &tmp.path().join("out.llamafile"));

        // Cache requirement: yes (zipalign is downloaded)
        assert!(reqs.iter().any(|r| r.purpose == Purpose::Cache && r.bytes == 5_000_000));
        // Output requirement: no (zipalign is a build tool, not bundled)
        assert!(reqs.iter().all(|r| r.purpose != Purpose::Output));
    }

    #[test]
    fn estimate_full_build_aggregates_per_destination() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let components = vec![
            url_component(ComponentRole::Runtime, Some(50_000_000)),
            url_component(ComponentRole::Zipalign, Some(5_000_000)),
            hf_component(ComponentRole::Weights, "weights.gguf", Some(2_000_000_000)),
            hf_component(ComponentRole::MmprojWeights, "mmproj.gguf", Some(500_000_000)),
            Component {
                role: ComponentRole::GpuLib { accel: GpuAccel::Cuda, os: Os::Linux },
                source: ComponentSource::Url("https://example.com/cuda.so".to_string()),
                zip_path: "ggml-cuda.so".to_string(),
                expected_size: Some(100_000_000),
                expected_sha256: None,
            },
            generated_component(ComponentRole::ArgsFile),
        ];

        let reqs = estimate(&components, &cache, &tmp.path().join("out.llamafile"));

        // Cache: runtime (50M) + zipalign (5M) + GPU lib (100M) = 155M
        let cache_bytes = reqs.iter()
            .find(|r| r.purpose == Purpose::Cache).unwrap().bytes;
        assert_eq!(cache_bytes, 50_000_000 + 5_000_000 + 100_000_000);

        // HF: weights (2G) + mmproj (500M) = 2.5G
        let hf_bytes = reqs.iter()
            .find(|r| r.purpose == Purpose::HuggingFaceCache).unwrap().bytes;
        assert_eq!(hf_bytes, 2_000_000_000 + 500_000_000);

        // Output: runtime + weights + mmproj + GPU lib (zipalign + .args excluded)
        let out_bytes = reqs.iter()
            .find(|r| r.purpose == Purpose::Output).unwrap().bytes;
        assert_eq!(out_bytes, 50_000_000 + 2_000_000_000 + 500_000_000 + 100_000_000);
    }

    #[test]
    fn estimate_ignores_components_without_expected_size_for_downloads() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache = Cache::new(tmp.path().to_path_buf());

        let components = vec![
            // URL with unknown size — can't reserve space, so skip
            url_component(ComponentRole::Runtime, None),
        ];
        let reqs = estimate(&components, &cache, &tmp.path().join("out.llamafile"));
        assert!(reqs.iter().all(|r| r.purpose != Purpose::Cache));
    }

    // ── check_with ──

    #[test]
    fn check_with_passes_when_space_is_plenty() {
        let tmp = tempfile::TempDir::new().unwrap();
        let req = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 1_000_000,
            purpose: Purpose::Cache,
        };
        let result = check_with(&[req], &|_p| Ok(10_000_000));
        let reports = result.expect("should pass");
        assert_eq!(reports.len(), 1);
        assert!(reports[0].is_sufficient());
        assert_eq!(reports[0].bytes_required, 1_000_000);
        assert_eq!(reports[0].bytes_available, 10_000_000);
    }

    #[test]
    fn check_with_errors_when_insufficient() {
        let tmp = tempfile::TempDir::new().unwrap();
        let req = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 5_000_000_000,
            purpose: Purpose::HuggingFaceCache,
        };
        let result = check_with(&[req], &|_p| Ok(1_000_000_000));
        match result {
            Err(DiskSpaceError::Insufficient {
                required, available, purposes, ..
            }) => {
                assert_eq!(required, 5_000_000_000);
                assert_eq!(available, 1_000_000_000);
                assert_eq!(purposes, vec!["huggingface cache".to_string()]);
            }
            other => panic!("expected Insufficient, got {:?}", other),
        }
    }

    #[test]
    fn check_with_groups_same_filesystem_destinations() {
        // Two requirements at the same canonical path should accumulate.
        let tmp = tempfile::TempDir::new().unwrap();
        let req1 = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 600_000_000,
            purpose: Purpose::Cache,
        };
        let req2 = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 600_000_000,
            purpose: Purpose::Output,
        };

        // Each requirement individually fits in 1GB, but combined (1.2GB) does
        // not. Verify the check correctly accumulates.
        let result = check_with(&[req1, req2], &|_p| Ok(1_000_000_000));
        match result {
            Err(DiskSpaceError::Insufficient { required, .. }) => {
                assert_eq!(required, 1_200_000_000);
            }
            other => panic!("expected Insufficient (combined), got {:?}", other),
        }
    }

    /// Regression: prior to the fs-id grouping, two distinct paths that
    /// happened to live on the same partition were each checked
    /// independently and each saw the full available space, so a combined
    /// requirement that overflowed the filesystem could pass the pre-check.
    /// (This is the case the reviewer flagged: cache_dir vs. output dir,
    /// which on a typical setup share `$HOME`'s filesystem.)
    #[test]
    #[cfg(any(unix, windows))]
    fn check_with_groups_distinct_paths_on_same_filesystem() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        let output_dir = tmp.path().join("out");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(&output_dir).unwrap();

        // Distinct directories, same filesystem.
        let reqs = vec![
            DiskRequirement {
                destination: cache_dir,
                bytes: 600_000_000,
                purpose: Purpose::Cache,
            },
            DiskRequirement {
                destination: output_dir,
                bytes: 600_000_000,
                purpose: Purpose::Output,
            },
        ];

        // 600M each fits in 1G; combined 1.2G does not.
        let result = check_with(&reqs, &|_p| Ok(1_000_000_000));
        match result {
            Err(DiskSpaceError::Insufficient {
                required,
                available,
                purposes,
                ..
            }) => {
                assert_eq!(required, 1_200_000_000);
                assert_eq!(available, 1_000_000_000);
                assert!(purposes.contains(&"cache".to_string()));
                assert!(purposes.contains(&"output".to_string()));
            }
            other => panic!("expected Insufficient (combined), got {:?}", other),
        }
    }

    #[test]
    #[cfg(any(unix, windows))]
    fn check_with_returns_single_report_per_filesystem() {
        // Three sibling subdirectories in one TempDir → all on the same fs →
        // one report.
        let tmp = tempfile::TempDir::new().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        let c = tmp.path().join("c");
        for p in [&a, &b, &c] {
            std::fs::create_dir_all(p).unwrap();
        }
        let reqs = vec![
            DiskRequirement { destination: a, bytes: 100, purpose: Purpose::Cache },
            DiskRequirement { destination: b, bytes: 200, purpose: Purpose::HuggingFaceCache },
            DiskRequirement { destination: c, bytes: 300, purpose: Purpose::Output },
        ];
        let reports = check_with(&reqs, &|_p| Ok(1_000_000)).unwrap();
        assert_eq!(reports.len(), 1, "expected one fs report, got {:?}", reports);
        assert_eq!(reports[0].bytes_required, 600);
        assert_eq!(reports[0].purposes.len(), 3);
    }

    #[test]
    fn check_with_calls_query_function_once_per_filesystem() {
        let tmp = tempfile::TempDir::new().unwrap();
        let req1 = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 100,
            purpose: Purpose::Cache,
        };
        let req2 = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 200,
            purpose: Purpose::Output,
        };

        let calls = Mutex::new(0u32);
        let counter = |_p: &Path| -> std::io::Result<u64> {
            *calls.lock().unwrap() += 1;
            Ok(10_000_000)
        };
        let _ = check_with(&[req1, req2], &counter).unwrap();
        assert_eq!(*calls.lock().unwrap(), 1, "should query each fs only once");
    }

    #[test]
    fn check_with_uses_nearest_existing_ancestor() {
        // The destination doesn't exist yet, but its parent does.
        let tmp = tempfile::TempDir::new().unwrap();
        let req = DiskRequirement {
            destination: tmp.path().join("does/not/exist/yet"),
            bytes: 100,
            purpose: Purpose::Output,
        };
        let probed = Mutex::new(None::<PathBuf>);
        let probe = |p: &Path| -> std::io::Result<u64> {
            *probed.lock().unwrap() = Some(p.to_path_buf());
            Ok(10_000)
        };
        let result = check_with(&[req], &probe).unwrap();
        assert_eq!(result.len(), 1);
        // Probed path must be a real, existing directory
        let probed_path = probed.lock().unwrap().clone().unwrap();
        assert!(probed_path.exists(), "probed {:?} should exist", probed_path);
    }

    #[test]
    fn check_with_real_fs4_returns_a_value() {
        // Sanity check: fs4 is callable on this platform and returns a sane
        // value for a real path (the temp dir).
        let tmp = tempfile::TempDir::new().unwrap();
        let req = DiskRequirement {
            destination: tmp.path().to_path_buf(),
            bytes: 1, // need essentially nothing
            purpose: Purpose::Cache,
        };
        let result = check(&[req]).expect("real disk check should succeed");
        assert_eq!(result.len(), 1);
        assert!(
            result[0].bytes_available > 0,
            "expected a positive available_space reading"
        );
    }

}

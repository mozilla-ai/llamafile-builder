use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::args;
use crate::cache::Cache;
use crate::config_manager::BuilderConfig;
use crate::models::*;
use crate::state::BuildState;
use crate::validation::{self, ValidationMessage};

// ============================================================================
// Build event types
// ============================================================================

/// Events emitted by the builder during a build.
#[derive(Debug, Clone)]
pub enum BuildEvent {
    /// Progress update (stage, percentage, message).
    Progress(BuildProgress),
    /// Log message.
    Log(String),
    /// Download progress for the current file.
    Download(Option<DownloadProgress>),
    /// A decision is needed from the user (interactive mode only).
    NeedDecision(DecisionRequest),
    /// Build completed successfully.
    Complete { output_path: PathBuf, size: u64 },
    /// Build failed.
    Failed { error: String },
}

/// Requests for user decisions during interactive builds.
#[derive(Debug, Clone)]
pub enum DecisionRequest {
    FileExists(FileExistsVariant),
    HfTokenNeeded,
}

/// User decisions sent back to the builder.
#[derive(Debug, Clone)]
pub enum Decision {
    FileExists(FileExistsChoice),
    HfToken(Option<String>),
}

/// Policy for handling existing cached files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileExistsPolicy {
    /// Use cached files without prompting (CLI default).
    SkipExisting,
    /// Always re-download everything.
    RedownloadAll,
    /// Send NeedDecision events for user to decide (server/TUI).
    Interactive,
}

// ============================================================================
// GPU library selection
// ============================================================================

/// A selected GPU library to include in the build.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuLibSelection {
    pub accel: GpuAccel,
    pub os: Os,
    pub source: ComponentSource,
}

// ============================================================================
// Builder
// ============================================================================

/// The main entry point for constructing a llamafile.
///
/// All selection methods modify internal state. `resolved_components()`
/// materializes the state into a list of components to assemble.
/// `build()` orchestrates the download-and-assemble pipeline.
pub struct Builder {
    config: BuilderConfig,
    state: BuildState,
    cache: Cache,
    cancel_flag: Arc<AtomicBool>,
    custom_zipalign: Option<PathBuf>,
    args_override: Option<String>,
    agent_config: AgentConfig,
    gpu_libs: Vec<GpuLibSelection>,
}

impl Builder {
    pub fn new(config: BuilderConfig, cache: Cache) -> Self {
        Self {
            config,
            state: BuildState::default(),
            cache,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            custom_zipalign: None,
            args_override: None,
            agent_config: AgentConfig::default(),
            gpu_libs: Vec::new(),
        }
    }

    // ── State queries ──

    pub fn state(&self) -> &BuildState {
        &self.state
    }

    pub fn config(&self) -> &BuilderConfig {
        &self.config
    }

    /// Preview the .args content that would be generated.
    pub fn resolved_args(&self) -> String {
        if let Some(ref override_args) = self.args_override {
            return override_args.clone();
        }
        let model_filename = self
            .state
            .resolved_weights_filename
            .as_deref()
            .unwrap_or("model.gguf");
        let mmproj_filename = self.state.resolved_mmproj_filename.as_deref();
        let config_values = if self.state.config_values.is_empty() {
            args::default_config_values(&self.config.config_schema)
        } else {
            self.state.config_values.clone()
        };
        args::generate_args_from_schema(
            &self.config.config_schema,
            &config_values,
            model_filename,
            mmproj_filename,
        )
    }

    /// Validate the current build state.
    pub fn validate(&self) -> Vec<ValidationMessage> {
        validation::validate(&self.state, &self.config)
    }

    /// Check if the current state has blocking errors.
    pub fn can_build(&self) -> bool {
        !self.validate().iter().any(|m| m.is_error())
    }

    // ── Version / binaries ──

    pub fn select_version(&mut self, group: &str, version: &str) {
        self.state.select_llamafile_version(
            group.to_string(),
            version.to_string(),
            &self.config.versions,
        );
    }

    pub fn use_custom_runtime(&mut self, path: &Path) {
        self.state
            .use_custom_llamafile(path.to_string_lossy().to_string());
    }

    pub fn use_custom_zipalign(&mut self, path: &Path) {
        self.custom_zipalign = Some(path.to_path_buf());
    }

    // ── Model ──

    pub fn select_registry_model(&mut self, model_id: &str) {
        self.state
            .select_registry_model(model_id.to_string(), &self.config.models);
    }

    pub fn select_custom_model(&mut self, path: &Path) {
        self.state
            .select_custom_model(path.to_string_lossy().to_string(), &self.config.models);
    }

    pub fn select_ollama_model(&mut self, model: OllamaModel) {
        self.state
            .select_ollama_model(model, &self.config.models);
    }

    pub fn select_lmstudio_model(&mut self, model: LMStudioModel) {
        self.state
            .select_lmstudio_model(model, &self.config.models);
    }

    pub fn select_quantization(&mut self, role: &str, index: usize) {
        self.state.select_file_quantization(
            role.to_string(),
            index,
            &self.config.models,
        );
    }

    // ── Configuration ──

    pub fn set_config_value(&mut self, key: &str, value: serde_json::Value) {
        self.state
            .config_values
            .insert(key.to_string(), value);
    }

    pub fn set_args_override(&mut self, raw_args: &str) {
        self.args_override = Some(raw_args.to_string());
    }

    pub fn clear_args_override(&mut self) {
        self.args_override = None;
    }

    // ── GPU libraries ──

    pub fn add_gpu_lib(&mut self, accel: GpuAccel, os: Os) {
        // Find the URL from the selected version's GPU libs
        let source = self.find_gpu_lib_url(accel, os);
        if let Some(source) = source {
            // Remove existing entry for same (accel, os)
            self.gpu_libs
                .retain(|g| !(g.accel == accel && g.os == os));
            self.gpu_libs.push(GpuLibSelection { accel, os, source });
        }
    }

    pub fn add_custom_gpu_lib(&mut self, path: &Path, accel: GpuAccel, os: Os) {
        self.gpu_libs
            .retain(|g| !(g.accel == accel && g.os == os));
        self.gpu_libs.push(GpuLibSelection {
            accel,
            os,
            source: ComponentSource::Local(path.to_path_buf()),
        });
    }

    pub fn remove_gpu_lib(&mut self, accel: GpuAccel, os: Os) {
        self.gpu_libs
            .retain(|g| !(g.accel == accel && g.os == os));
    }

    pub fn clear_gpu_libs(&mut self) {
        self.gpu_libs.clear();
    }

    // ── Agent ──

    pub fn set_agent_config(&mut self, config: AgentConfig) {
        self.agent_config = config;
    }

    // ── Output ──

    pub fn set_output_path(&mut self, path: &Path) {
        self.state.output_path = path.to_string_lossy().to_string();
    }

    // ── Execution ──

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    /// Run the build pipeline.
    ///
    /// Sends `BuildEvent`s to `event_tx` as the build progresses.
    /// For interactive builds, listens for `Decision`s on `decision_rx`.
    pub async fn build(
        &self,
        event_tx: mpsc::Sender<BuildEvent>,
        _decision_rx: Option<mpsc::Receiver<Decision>>,
        _policy: FileExistsPolicy,
    ) -> crate::Result<PathBuf> {
        self.cancel_flag.store(false, Ordering::Relaxed);

        let hf_token = crate::download::resolve_hf_token(None);

        // Phase 1: Ensure runtime
        self.send_progress(&event_tx, BuildStage::DownloadingRuntime, 0.0, "Preparing runtime...").await;
        let runtime_path = self.ensure_runtime(hf_token.as_deref(), &event_tx).await?;
        self.check_cancelled()?;

        // Phase 2: Ensure model files
        self.send_progress(&event_tx, BuildStage::DownloadingModel, 20.0, "Preparing model files...").await;
        let (weights_path, mmproj_path) = self.ensure_model_files(hf_token.as_deref(), &event_tx).await?;
        self.check_cancelled()?;

        let _weights_filename = weights_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("model.gguf")
            .to_string();
        let _mmproj_filename = mmproj_path.as_ref().and_then(|p| {
            p.file_name().and_then(|n| n.to_str()).map(|s| s.to_string())
        });

        // Phase 3: Write .args
        self.send_progress(&event_tx, BuildStage::WritingConfig, 50.0, "Writing configuration...").await;
        let args_content = self.resolved_args();
        let args_path = crate::file_ops::write_temp_file(&args_content, ".args")
            .await
            .map_err(|e| crate::Error::Build(format!("Failed to write .args: {e}")))?;
        self.check_cancelled()?;

        // Phase 4: Ensure GPU libraries
        self.send_progress(&event_tx, BuildStage::BundlingGpu, 60.0, "Preparing GPU libraries...").await;
        let gpu_lib_paths = self.ensure_gpu_libs(hf_token.as_deref(), &event_tx).await;
        self.check_cancelled()?;

        // Phase 5: Create executable
        self.send_progress(&event_tx, BuildStage::CreatingExecutable, 80.0, "Creating executable...").await;
        let output_path = PathBuf::from(&self.state.output_path);

        // Copy runtime to output
        let _ = event_tx.send(BuildEvent::Log(format!(
            "Copying runtime to {}...",
            output_path.display()
        ))).await;
        crate::file_ops::copy_file(
            &runtime_path.to_string_lossy(),
            &output_path.to_string_lossy(),
        )
        .await
        .map_err(|e| crate::Error::Build(format!("Failed to copy runtime: {e}")))?;

        // Make executable
        crate::zipalign::make_executable(&output_path).await?;

        // Bundle files with zipalign
        let mut files_to_bundle: Vec<PathBuf> = Vec::new();
        files_to_bundle.push(PathBuf::from(&args_path));
        files_to_bundle.push(weights_path.clone());
        if let Some(ref mmproj) = mmproj_path {
            files_to_bundle.push(mmproj.clone());
        }
        for gpu_path in &gpu_lib_paths {
            files_to_bundle.push(gpu_path.clone());
        }

        // Write agent prompt if enabled
        if self.agent_config.enabled {
            let prompt = args::generate_system_prompt(&self.agent_config);
            let prompt_path = crate::file_ops::write_temp_file(&prompt, ".args.prompt")
                .await
                .map_err(|e| crate::Error::Build(format!("Failed to write .args.prompt: {e}")))?;
            files_to_bundle.push(PathBuf::from(prompt_path));
        }

        let file_refs: Vec<&Path> = files_to_bundle.iter().map(|p| p.as_path()).collect();

        let _ = event_tx.send(BuildEvent::Log(format!(
            "Running zipalign with {} files...",
            file_refs.len()
        ))).await;

        let log_lines = crate::zipalign::zipalign_bundle(
            &output_path,
            &file_refs,
            self.custom_zipalign.as_deref(),
        )
        .await?;

        for line in &log_lines {
            let _ = event_tx.send(BuildEvent::Log(line.clone())).await;
        }

        // Done
        let final_size = tokio::fs::metadata(&output_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        let _ = event_tx.send(BuildEvent::Complete {
            output_path: output_path.clone(),
            size: final_size,
        }).await;

        Ok(output_path)
    }

    // ── Internal helpers ──

    fn check_cancelled(&self) -> crate::Result<()> {
        if self.cancel_flag.load(Ordering::Relaxed) {
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn send_progress(
        &self,
        tx: &mpsc::Sender<BuildEvent>,
        stage: BuildStage,
        pct: f32,
        msg: &str,
    ) {
        let _ = tx.send(BuildEvent::Progress(BuildProgress {
            stage,
            progress_pct: pct,
            message: msg.to_string(),
            bytes_downloaded: 0,
            bytes_total: 0,
            error: None,
        })).await;
    }

    async fn ensure_runtime(
        &self,
        hf_token: Option<&str>,
        event_tx: &mpsc::Sender<BuildEvent>,
    ) -> crate::Result<PathBuf> {
        let path = self.state.resolved_llamafile_path.as_ref().ok_or_else(|| {
            crate::Error::Build("No llamafile version selected".to_string())
        })?;

        // If it's a custom path, just verify it exists
        if let crate::state::LlamafileSource::Custom = self.state.llamafile_source {
            let p = PathBuf::from(path);
            if p.exists() {
                return Ok(p);
            } else {
                return Err(crate::Error::NotFound(format!(
                    "Custom llamafile not found: {}",
                    path
                )));
            }
        }

        // Registry: find the release and download if needed
        let release = self.find_selected_release().ok_or_else(|| {
            crate::Error::Build("Selected release not found in config".to_string())
        })?;

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url(release.download_url.clone()),
            zip_path: String::new(),
            expected_size: Some(release.size_bytes),
            expected_sha256: Some(release.sha256.clone()),
        };

        let (progress_tx, mut progress_rx) = mpsc::channel::<DownloadProgress>(32);

        // Forward download progress to build events
        let event_tx2 = event_tx.clone();
        let progress_forwarder = tokio::spawn(async move {
            while let Some(dp) = progress_rx.recv().await {
                let _ = event_tx2.send(BuildEvent::Download(Some(dp))).await;
            }
        });

        let result = self.cache.ensure(
            &component,
            hf_token,
            progress_tx,
            self.cancel_flag.clone(),
        ).await;

        progress_forwarder.abort();
        let _ = event_tx.send(BuildEvent::Download(None)).await;

        result.map_err(|e| crate::Error::Download(e))
    }

    async fn ensure_model_files(
        &self,
        hf_token: Option<&str>,
        event_tx: &mpsc::Sender<BuildEvent>,
    ) -> crate::Result<(PathBuf, Option<PathBuf>)> {
        // Weights
        let weights_path = match &self.state.resolved_weights_path {
            Some(p) if !p.is_empty() => {
                let _ = event_tx.send(BuildEvent::Log(format!("Using model weights: {p}"))).await;
                PathBuf::from(p)
            }
            _ => {
                // Registry model: need to download
                let _ = event_tx.send(BuildEvent::Log("Downloading model weights...".to_string())).await;
                self.download_registry_file("weights", hf_token, event_tx).await?
            }
        };

        // Mmproj (optional)
        let mmproj_path = match &self.state.resolved_mmproj_path {
            Some(p) if !p.is_empty() => {
                let _ = event_tx.send(BuildEvent::Log(format!("Using mmproj: {p}"))).await;
                Some(PathBuf::from(p))
            }
            _ => {
                // Check if registry model has mmproj
                if self.state.resolved_mmproj_filename.is_some() {
                    let _ = event_tx.send(BuildEvent::Log("Downloading mmproj...".to_string())).await;
                    Some(self.download_registry_file("mmproj", hf_token, event_tx).await?)
                } else {
                    None
                }
            }
        };

        Ok((weights_path, mmproj_path))
    }

    async fn download_registry_file(
        &self,
        role: &str,
        hf_token: Option<&str>,
        event_tx: &mpsc::Sender<BuildEvent>,
    ) -> crate::Result<PathBuf> {
        let model_id = self.state.selected_model_id.as_ref().ok_or_else(|| {
            crate::Error::Build("No model selected for download".to_string())
        })?;
        let model = self.config.models.iter().find(|m| &m.id == model_id).ok_or_else(|| {
            crate::Error::NotFound(format!("Model not found: {model_id}"))
        })?;
        let file = model.files.iter().find(|f| f.role == role).ok_or_else(|| {
            crate::Error::NotFound(format!("No {role} file for model {model_id}"))
        })?;
        let sel = self.state.model_file_selections.get(role);
        let quant_idx = sel.map(|s| s.quantization_idx).unwrap_or(0);
        let quant = file.quantizations.get(quant_idx).ok_or_else(|| {
            crate::Error::Build(format!("Quantization index {} out of range", quant_idx))
        })?;

        let component = Component {
            role: if role == "weights" {
                ComponentRole::Weights
            } else {
                ComponentRole::MmprojWeights
            },
            source: ComponentSource::Url(quant.download_url.clone()),
            zip_path: format!("/zip/{}", get_filename_from_url(&quant.download_url)),
            expected_size: Some(quant.size_bytes),
            expected_sha256: quant.sha256.clone(),
        };

        let (progress_tx, mut progress_rx) = mpsc::channel::<DownloadProgress>(32);
        let event_tx2 = event_tx.clone();
        let progress_forwarder = tokio::spawn(async move {
            while let Some(dp) = progress_rx.recv().await {
                let _ = event_tx2.send(BuildEvent::Download(Some(dp))).await;
            }
        });

        let result = self.cache.ensure(
            &component,
            hf_token,
            progress_tx,
            self.cancel_flag.clone(),
        ).await;

        progress_forwarder.abort();
        let _ = event_tx.send(BuildEvent::Download(None)).await;

        result.map_err(|e| crate::Error::Download(e))
    }

    async fn ensure_gpu_libs(
        &self,
        hf_token: Option<&str>,
        event_tx: &mpsc::Sender<BuildEvent>,
    ) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if self.gpu_libs.is_empty() {
            let _ = event_tx.send(BuildEvent::Log("No GPU libraries to bundle.".to_string())).await;
            return paths;
        }

        for lib in &self.gpu_libs {
            let _ = event_tx.send(BuildEvent::Log(format!(
                "Preparing {:?} library for {:?}...",
                lib.accel, lib.os
            ))).await;

            let ext = match lib.os {
                Os::Windows => ".dll",
                Os::Linux => ".so",
            };
            let accel_name = match lib.accel {
                GpuAccel::Cuda => "cuda",
                GpuAccel::Rocm => "rocm",
                GpuAccel::Vulkan => "vulkan",
            };

            let component = Component {
                role: ComponentRole::GpuLib {
                    accel: lib.accel,
                    os: lib.os,
                },
                source: lib.source.clone(),
                zip_path: format!("ggml-{}{}", accel_name, ext),
                expected_size: None,
                expected_sha256: None,
            };

            let (progress_tx, _progress_rx) = mpsc::channel::<DownloadProgress>(32);

            match self.cache.ensure(
                &component,
                hf_token,
                progress_tx,
                self.cancel_flag.clone(),
            ).await {
                Ok(path) => {
                    let _ = event_tx.send(BuildEvent::Log(format!(
                        "GPU library ready: {}",
                        path.display()
                    ))).await;
                    paths.push(path);
                }
                Err(e) => {
                    // GPU lib failures are non-fatal
                    let _ = event_tx.send(BuildEvent::Log(format!(
                        "Warning: GPU library {:?}/{:?} failed: {}",
                        lib.accel, lib.os, e
                    ))).await;
                }
            }
        }

        paths
    }

    fn find_selected_release(&self) -> Option<LlamafileRelease> {
        let group_name = self.state.selected_version_group.as_ref()?;
        let version = self.state.selected_release_version.as_ref()?;
        let group = self.config.versions.iter().find(|g| &g.name == group_name)?;
        group.releases.iter().find(|r| &r.version == version).cloned()
    }

    fn find_gpu_lib_url(&self, accel: GpuAccel, os: Os) -> Option<ComponentSource> {
        let release = self.find_selected_release()?;
        let gpu_libs = release.gpu_libs.as_ref()?;
        let by_platform = match accel {
            GpuAccel::Cuda => gpu_libs.cuda.as_ref()?,
            GpuAccel::Rocm => gpu_libs.rocm.as_ref()?,
            GpuAccel::Vulkan => return None, // Not in current data format
        };
        let lib = match os {
            Os::Linux => by_platform.linux.as_ref()?,
            Os::Windows => by_platform.windows.as_ref()?,
        };
        Some(ComponentSource::Url(lib.download_url.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BuilderConfig {
        BuilderConfig {
            versions: serde_json::from_str(include_str!("../data/versions.json")).unwrap(),
            models: serde_json::from_str(include_str!("../data/models.json")).unwrap(),
            config_schema: serde_json::from_str(include_str!("../data/config.json")).unwrap(),
            config_version: 1,
            min_builder_version: None,
        }
    }

    fn test_cache() -> Cache {
        Cache::new(PathBuf::from("/tmp/llamafile-test-cache"))
    }

    #[test]
    fn builder_new_has_empty_state() {
        let builder = Builder::new(test_config(), test_cache());
        assert!(builder.state().selected_model_id.is_none());
        assert!(builder.state().output_path.is_empty());
        assert!(builder.gpu_libs.is_empty());
    }

    #[test]
    fn select_registry_model_updates_state() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.select_registry_model("qwen3-0.6b");
        assert_eq!(
            builder.state().selected_model_id.as_deref(),
            Some("qwen3-0.6b")
        );
        assert!(builder.state().output_path.ends_with(".llamafile"));
        assert!(builder.state().resolved_weights_filename.is_some());
    }

    #[test]
    fn select_version_updates_state() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();

        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);
        assert!(builder.state().resolved_llamafile_path.is_some());
    }

    #[test]
    fn resolved_args_produces_valid_output() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.select_registry_model("qwen3-0.6b");
        let args = builder.resolved_args();
        assert!(args.contains("-m"));
        assert!(args.contains("/zip/"));
        assert!(args.ends_with("..."));
    }

    #[test]
    fn args_override_replaces_generated_args() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.select_registry_model("qwen3-0.6b");
        builder.set_args_override("-m\n/zip/custom.gguf\n...");
        assert_eq!(builder.resolved_args(), "-m\n/zip/custom.gguf\n...");

        builder.clear_args_override();
        assert!(builder.resolved_args().contains("/zip/"));
    }

    #[test]
    fn add_and_remove_gpu_libs() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();

        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);

        builder.add_gpu_lib(GpuAccel::Cuda, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 1);

        // Adding same (accel, os) replaces
        builder.add_gpu_lib(GpuAccel::Cuda, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 1);

        builder.add_gpu_lib(GpuAccel::Rocm, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 2);

        builder.remove_gpu_lib(GpuAccel::Cuda, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 1);

        builder.clear_gpu_libs();
        assert!(builder.gpu_libs.is_empty());
    }

    #[test]
    fn add_custom_gpu_lib() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.add_custom_gpu_lib(
            Path::new("/my/ggml-cuda.so"),
            GpuAccel::Cuda,
            Os::Linux,
        );
        assert_eq!(builder.gpu_libs.len(), 1);
        assert_eq!(
            builder.gpu_libs[0].source,
            ComponentSource::Local(PathBuf::from("/my/ggml-cuda.so"))
        );
    }

    #[test]
    fn validate_empty_state_has_errors() {
        let builder = Builder::new(test_config(), test_cache());
        let msgs = builder.validate();
        assert!(msgs.iter().any(|m| m.is_error()));
        assert!(!builder.can_build());
    }

    #[test]
    fn validate_complete_state_has_no_errors() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();

        let mut builder = Builder::new(config, test_cache());
        builder.select_registry_model("qwen3-0.6b");
        builder.select_version(&group_name, &release_version);
        // Output path is auto-set by select_registry_model

        let msgs = builder.validate();
        let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
        assert!(errors.is_empty(), "Unexpected errors: {:?}", errors);
    }

    #[test]
    fn set_config_value_updates_state() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.set_config_value("context_length", serde_json::json!(16384));
        assert_eq!(
            builder.state().config_values.get("context_length"),
            Some(&serde_json::json!(16384))
        );
    }

    #[test]
    fn set_agent_config() {
        let mut builder = Builder::new(test_config(), test_cache());
        let mut agent = AgentConfig::default();
        agent.enabled = true;
        builder.set_agent_config(agent.clone());
        assert!(builder.agent_config.enabled);
    }

    #[test]
    fn set_output_path() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.set_output_path(Path::new("/tmp/my-output.llamafile"));
        assert_eq!(builder.state().output_path, "/tmp/my-output.llamafile");
    }

    #[test]
    fn use_custom_runtime() {
        let mut builder = Builder::new(test_config(), test_cache());
        builder.use_custom_runtime(Path::new("/my/llamafile"));
        assert_eq!(
            builder.state().resolved_llamafile_path.as_deref(),
            Some("/my/llamafile")
        );
    }
}

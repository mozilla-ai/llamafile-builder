use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::args;
use crate::cache::Cache;
use crate::config_manager::BuilderConfig;
use crate::models::*;
use crate::state::BuildState;
use crate::validation::{self, ValidationMessage};

// ============================================================================
// Build phases
// ============================================================================

/// A coarse-grained phase of the build pipeline. Emitted as a structured
/// `phase` field on tracing events at the start of each phase, so observers
/// (TUIs, dashboards) can drive a progress display without parsing log
/// message text.
///
/// All emit-on-entry phases below fire at most once per build, in the
/// order listed. `PreparingGpuLibs` is conditional: it fires only when
/// the user has selected at least one GPU library. New phases must be
/// added explicitly; do not infer phase from message strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildPhase {
    PreparingZipalign,
    PreparingRuntime,
    PreparingModel,
    WritingArgs,
    PreparingGpuLibs,
    Assembling,
}

impl BuildPhase {
    /// Stable kebab-case identifier for this phase. Used as the value of
    /// the `phase` tracing field via the `%` (Display) formatter.
    pub fn as_str(self) -> &'static str {
        match self {
            BuildPhase::PreparingZipalign => "preparing-zipalign",
            BuildPhase::PreparingRuntime => "preparing-runtime",
            BuildPhase::PreparingModel => "preparing-model",
            BuildPhase::WritingArgs => "writing-args",
            BuildPhase::PreparingGpuLibs => "preparing-gpu-libs",
            BuildPhase::Assembling => "assembling",
        }
    }

    /// Parse a phase identifier emitted by [`BuildPhase::as_str`]. The
    /// kebab-case Display form is the *only* canonical wire format —
    /// emit sites must use `%` (Display), not `?` (Debug). An emit site
    /// using `?` produces a value `from_str` can't map; the consume-side
    /// visitor sees `None` and the `log_layer_integration` tests would
    /// catch that on the next run.
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "preparing-zipalign" => BuildPhase::PreparingZipalign,
            "preparing-runtime" => BuildPhase::PreparingRuntime,
            "preparing-model" => BuildPhase::PreparingModel,
            "writing-args" => BuildPhase::WritingArgs,
            "preparing-gpu-libs" => BuildPhase::PreparingGpuLibs,
            "assembling" => BuildPhase::Assembling,
            _ => return None,
        })
    }
}

impl std::fmt::Display for BuildPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
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
    /// Expected download size in bytes (registry selections only).
    pub expected_size: Option<u64>,
    /// Expected SHA256 of the downloaded file (registry selections only).
    pub expected_sha256: Option<String>,
}

fn gpu_accel_name(accel: GpuAccel) -> &'static str {
    match accel {
        GpuAccel::Cuda => "cuda",
        GpuAccel::Rocm => "rocm",
        GpuAccel::Vulkan => "vulkan",
    }
}

fn gpu_lib_component(lib: &GpuLibSelection) -> Component {
    let ext = match lib.os {
        Os::Windows => ".dll",
        Os::Linux => ".so",
    };
    Component {
        role: ComponentRole::GpuLib {
            accel: lib.accel,
            os: lib.os,
        },
        source: lib.source.clone(),
        zip_path: format!("ggml-{}{}", gpu_accel_name(lib.accel), ext),
        expected_size: lib.expected_size,
        expected_sha256: lib.expected_sha256.clone(),
    }
}

// ============================================================================
// Builder
// ============================================================================

/// The main entry point for constructing a llamafile.
///
/// All selection methods modify internal state. `build()` orchestrates the
/// download-and-assemble pipeline. All operations are observable via `tracing`.
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

    /// Materialize the current state into a flat list of components that
    /// `build()` would assemble. Useful for previewing what will be built.
    pub fn resolved_components(&self) -> Vec<Component> {
        let mut components = Vec::new();

        // Zipalign
        if let Some(ref custom) = self.custom_zipalign {
            components.push(Component {
                role: ComponentRole::Zipalign,
                source: ComponentSource::Local(custom.clone()),
                zip_path: String::new(),
                expected_size: None,
                expected_sha256: None,
            });
        } else if let Some(release) = self.find_selected_release() {
            if let Some(ref url) = release.zipalign_url {
                components.push(Component {
                    role: ComponentRole::Zipalign,
                    source: ComponentSource::Url(url.clone()),
                    zip_path: String::new(),
                    expected_size: release.zipalign_size_bytes,
                    expected_sha256: release.zipalign_sha256.clone(),
                });
            }
        }

        // Runtime
        if let Some(release) = self.find_selected_release() {
            components.push(Component {
                role: ComponentRole::Runtime,
                source: ComponentSource::Url(release.download_url.clone()),
                zip_path: String::new(),
                expected_size: Some(release.size_bytes),
                expected_sha256: Some(release.sha256.clone()),
            });
        } else if let Some(ref path) = self.state.custom_llamafile_path {
            components.push(Component {
                role: ComponentRole::Runtime,
                source: ComponentSource::Local(PathBuf::from(path)),
                zip_path: String::new(),
                expected_size: None,
                expected_sha256: None,
            });
        }

        // Model weights
        if let Some(ref path) = self.state.resolved_weights_path {
            if !path.is_empty() {
                components.push(Component {
                    role: ComponentRole::Weights,
                    source: ComponentSource::Local(PathBuf::from(path)),
                    zip_path: format!(
                        "/zip/{}",
                        self.state.resolved_weights_filename.as_deref().unwrap_or("model.gguf")
                    ),
                    expected_size: None,
                    expected_sha256: None,
                });
            }
        } else if let Some(ref model_id) = self.state.selected_model_id {
            if let Some(comp) = self.registry_file_component(model_id, "weights", ComponentRole::Weights) {
                components.push(comp);
            }
        }

        // Mmproj weights
        if let Some(ref path) = self.state.resolved_mmproj_path {
            if !path.is_empty() {
                components.push(Component {
                    role: ComponentRole::MmprojWeights,
                    source: ComponentSource::Local(PathBuf::from(path)),
                    zip_path: format!(
                        "/zip/{}",
                        self.state.resolved_mmproj_filename.as_deref().unwrap_or("mmproj.gguf")
                    ),
                    expected_size: None,
                    expected_sha256: None,
                });
            }
        } else if let Some(ref model_id) = self.state.selected_model_id {
            if self.state.resolved_mmproj_filename.is_some() {
                if let Some(comp) = self.registry_file_component(model_id, "mmproj", ComponentRole::MmprojWeights) {
                    components.push(comp);
                }
            }
        }

        // .args
        components.push(Component {
            role: ComponentRole::ArgsFile,
            source: ComponentSource::Generated,
            zip_path: "/zip/.args".to_string(),
            expected_size: None,
            expected_sha256: None,
        });

        // GPU libraries
        for lib in &self.gpu_libs {
            components.push(gpu_lib_component(lib));
        }

        // Agent prompt
        if self.agent_config.enabled {
            components.push(Component {
                role: ComponentRole::ArgsPrompt,
                source: ComponentSource::Generated,
                zip_path: "/zip/.args.prompt".to_string(),
                expected_size: None,
                expected_sha256: None,
            });
        }

        components
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
        let mut msgs = validation::validate(&self.state, &self.config);

        // GPU-on-wrong-platform checks
        for lib in &self.gpu_libs {
            match lib.accel {
                GpuAccel::Cuda | GpuAccel::Rocm | GpuAccel::Vulkan => {
                    if cfg!(target_os = "macos") {
                        msgs.push(ValidationMessage::Warning(format!(
                            "{:?} GPU libraries are not supported on macOS. \
                             Metal acceleration is built into the llamafile binary.",
                            lib.accel
                        )));
                    }
                }
            }
        }

        // Agent shell tool warning
        if self.agent_config.enabled {
            if self.agent_config.tools.iter().any(|t| t.id == "shell" && t.enabled) {
                msgs.push(ValidationMessage::Warning(
                    "Agent shell execution tool is enabled. \
                     This allows the llamafile to run arbitrary commands."
                        .to_string(),
                ));
            }
        }

        msgs
    }

    /// Check if the current state has blocking errors.
    pub fn can_build(&self) -> bool {
        !self.validate().iter().any(|m| m.is_error())
    }

    // ── Version / binaries ──

    /// Select the default version: the version group marked `default: true`,
    /// or the first group if none is marked. Picks the first release within
    /// that group. Returns false if no versions are available.
    pub fn select_default_version(&mut self) -> bool {
        let group = self.config.versions.iter()
            .find(|g| g.default)
            .or_else(|| self.config.versions.first());
        let picked = group.and_then(|g| {
            g.releases.first().map(|r| (g.name.clone(), r.version.clone()))
        });
        if let Some((group, version)) = picked {
            self.select_version(&group, &version);
            true
        } else {
            false
        }
    }

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

    /// Select the default model: the model marked `default: true`,
    /// or the first model if none is marked. Returns false if no models
    /// are available.
    pub fn select_default_model(&mut self) -> bool {
        let model_id = self.config.models.iter()
            .find(|m| m.default)
            .or_else(|| self.config.models.first())
            .map(|m| m.id.clone());
        if let Some(id) = model_id {
            self.select_registry_model(&id);
            true
        } else {
            false
        }
    }

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

    /// Add a companion file (e.g. mmproj) to a custom model selection.
    pub fn add_custom_companion_file(&mut self, role: &str, path: &Path) {
        self.state.add_custom_companion_file(
            role.to_string(),
            path.to_string_lossy().to_string(),
            &self.config.models,
        );
    }

    /// Disable a file role (e.g. "mmproj") -- it won't be downloaded or bundled.
    /// Useful for building text-only from a multimodal model.
    pub fn disable_file_role(&mut self, role: &str) {
        self.state.model_file_selections.remove(role);
        self.state.resolved_mmproj_path = None;
        self.state.resolved_mmproj_filename = None;
    }

    // ── Configuration ──

    pub fn set_config_value(&mut self, key: &str, value: serde_json::Value) {
        self.state.config_values.insert(key.to_string(), value);
    }

    pub fn set_args_override(&mut self, raw_args: &str) {
        self.args_override = Some(raw_args.to_string());
    }

    // ── GPU libraries ──

    pub fn add_gpu_lib(&mut self, accel: GpuAccel, os: Os) {
        if let Some(selection) = self.find_gpu_lib(accel, os) {
            self.gpu_libs
                .retain(|g| !(g.accel == accel && g.os == os));
            self.gpu_libs.push(selection);
        }
    }

    pub fn add_custom_gpu_lib(&mut self, path: &Path, accel: GpuAccel, os: Os) {
        self.gpu_libs
            .retain(|g| !(g.accel == accel && g.os == os));
        self.gpu_libs.push(GpuLibSelection {
            accel,
            os,
            source: ComponentSource::Local(path.to_path_buf()),
            expected_size: None,
            expected_sha256: None,
        });
    }

    // ── Output ──

    pub fn set_output_path(&mut self, path: &Path) {
        self.state.output_path = path.to_string_lossy().to_string();
    }

    // ── Execution ──

    /// Use an external cancel flag (shared with the caller).
    /// This allows the caller to cancel the build from outside.
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        self.cancel_flag = flag;
    }

    /// Run the build pipeline. Returns the path to the created llamafile.
    ///
    /// All progress is observable via `tracing` spans and events.
    /// Use a `tracing` subscriber in the calling code to display or log them.
    pub async fn build(&self) -> crate::Result<PathBuf> {
        tracing::info!(output = %self.state.output_path, "build started");
        self.cancel_flag.store(false, Ordering::Relaxed);

        // Phase 0: Disk-space pre-check. Fail fast before any download starts
        // rather than running out mid-transfer.
        self.preflight_disk_space()?;
        self.check_cancelled()?;

        // Phase 1: Ensure zipalign (prerequisite for everything)
        let zipalign_path = {
            tracing::info!(phase = %BuildPhase::PreparingZipalign, "preparing zipalign");
            self.ensure_zipalign().await?
        };
        self.check_cancelled()?;

        // Phase 2: Ensure runtime
        let runtime_path = {
            tracing::info!(phase = %BuildPhase::PreparingRuntime, "preparing runtime binary");
            self.ensure_runtime().await?
        };
        self.check_cancelled()?;

        // Phase 3: Ensure model files
        let (weights_path, mmproj_path) = {
            tracing::info!(phase = %BuildPhase::PreparingModel, "preparing model files");
            self.ensure_model_files().await?
        };
        self.check_cancelled()?;

        // Phase 4: Write .args
        let args_path = {
            let args_content = self.resolved_args();
            tracing::info!(
                phase = %BuildPhase::WritingArgs,
                content_length = args_content.len(),
                "writing .args configuration"
            );
            tracing::debug!(content = %args_content, ".args content");
            crate::file_ops::write_temp_file(&args_content, ".args")
                .await
                .map_err(|e| crate::Error::Build(format!("Failed to write .args: {e}")))?
        };
        self.check_cancelled()?;

        // Phase 5: Ensure GPU libraries
        let gpu_lib_paths = {
            self.ensure_gpu_libs().await
        };
        self.check_cancelled()?;

        // Phase 6: Create executable
        let output_path = {
            self.create_executable(
                &zipalign_path,
                &runtime_path,
                &weights_path,
                mmproj_path.as_deref(),
                &args_path,
                &gpu_lib_paths,
            )
            .await?
        };

        let final_size = tokio::fs::metadata(&output_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        tracing::debug!(
            path = %output_path.display(),
            size = final_size,
            size_human = %format_size(final_size),
            "build complete"
        );

        Ok(output_path)
    }

    // ── Internal helpers ──

    /// Estimate per-destination disk requirements for the current state. The
    /// caller can preview this before building. Same logic [`build`] runs at
    /// Phase 0.
    pub fn estimate_disk_requirements(&self) -> Vec<crate::disk_space::DiskRequirement> {
        let components = self.resolved_components();
        let output_path = std::path::PathBuf::from(&self.state.output_path);
        crate::disk_space::estimate(&components, &self.cache, &output_path)
    }

    fn preflight_disk_space(&self) -> crate::Result<()> {
        let requirements = self.estimate_disk_requirements();
        if requirements.is_empty() {
            tracing::debug!("disk-space pre-check skipped: nothing to download or write");
            return Ok(());
        }

        for req in &requirements {
            tracing::debug!(
                destination = %req.destination.display(),
                bytes_required = req.bytes,
                purpose = req.purpose.label(),
                "disk-space requirement"
            );
        }

        match crate::disk_space::check(&requirements) {
            Ok(reports) => {
                for r in &reports {
                    tracing::info!(
                        path = %r.checked_path.display(),
                        bytes_required = r.bytes_required,
                        bytes_available = r.bytes_available,
                        "disk-space ok"
                    );
                }
                Ok(())
            }
            Err(crate::disk_space::DiskSpaceError::Query { path, source }) => {
                // Failing to query free space (e.g. unsupported filesystem) is
                // non-fatal — log and continue rather than blocking the build.
                tracing::warn!(
                    path = %path.display(),
                    error = %source,
                    "skipping disk-space check (could not query free space)"
                );
                Ok(())
            }
            Err(e) => {
                // Only `Insufficient` reaches here; `Query` is handled above.
                tracing::error!(error = %e, "disk-space pre-check failed");
                Err(crate::Error::DiskSpace(e))
            }
        }
    }

    fn check_cancelled(&self) -> crate::Result<()> {
        if self.cancel_flag.load(Ordering::Relaxed) {
            tracing::warn!("build cancelled");
            Err(crate::Error::Cancelled)
        } else {
            Ok(())
        }
    }

    async fn ensure_zipalign(&self) -> crate::Result<PathBuf> {
        // If user provided a custom zipalign, use it
        if let Some(ref custom) = self.custom_zipalign {
            if custom.exists() {
                tracing::info!(path = %custom.display(), "using custom zipalign");
                return Ok(custom.clone());
            } else {
                return Err(crate::Error::NotFound(format!(
                    "Custom zipalign not found: {}", custom.display()
                )));
            }
        }

        // Try to download from the selected release
        let release = self.find_selected_release();
        if let Some(release) = release {
            if let Some(ref url) = release.zipalign_url {
                tracing::info!(url, "downloading zipalign");
                let component = Component {
                    role: ComponentRole::Zipalign,
                    source: ComponentSource::Url(url.clone()),
                    zip_path: String::new(),
                    expected_size: release.zipalign_size_bytes,
                    expected_sha256: release.zipalign_sha256.clone(),
                };

                let path = self.cache
                    .ensure(&component, self.cancel_flag.clone())
                    .await
                    .map_err(crate::Error::Download)?;

                // Make it executable
                crate::zipalign::make_executable(&path).await?;
                return Ok(path);
            }
        }

        // Fallback: find zipalign in PATH or next to binary
        let found = crate::zipalign::find_zipalign();
        if found.to_str() == Some("zipalign") {
            // find_zipalign fell through to PATH name only -- check it exists
            if std::process::Command::new("zipalign")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_err()
            {
                return Err(crate::Error::NotFound(
                    "zipalign not found. Select a llamafile version that includes \
                     zipalign, provide one with --zipalign, or install it in PATH."
                        .to_string(),
                ));
            }
        }
        tracing::info!(path = %found.display(), "using zipalign from PATH");
        Ok(found)
    }

    async fn ensure_runtime(&self) -> crate::Result<PathBuf> {
        let path = self.state.resolved_llamafile_path.as_ref().ok_or_else(|| {
            crate::Error::Build("No llamafile version selected".to_string())
        })?;

        if let crate::state::LlamafileSource::Custom = self.state.llamafile_source {
            let p = PathBuf::from(path);
            if p.exists() {
                tracing::info!(path = %p.display(), "using custom runtime");
                return Ok(p);
            } else {
                return Err(crate::Error::NotFound(format!(
                    "Custom llamafile not found: {}", path
                )));
            }
        }

        let release = self.find_selected_release().ok_or_else(|| {
            crate::Error::Build("Selected release not found in config".to_string())
        })?;

        tracing::info!(
            version = %release.version,
            url = %release.download_url,
            size = release.size_bytes,
            "downloading runtime"
        );

        let component = Component {
            role: ComponentRole::Runtime,
            source: ComponentSource::Url(release.download_url.clone()),
            zip_path: String::new(),
            expected_size: Some(release.size_bytes),
            expected_sha256: Some(release.sha256.clone()),
        };

        self.cache
            .ensure(&component, self.cancel_flag.clone())
            .await
            .map_err(crate::Error::Download)
    }

    async fn ensure_model_files(&self) -> crate::Result<(PathBuf, Option<PathBuf>)> {
        // Weights
        let weights_path = match &self.state.resolved_weights_path {
            Some(p) if !p.is_empty() => {
                tracing::info!(path = %p, "using local model weights");
                PathBuf::from(p)
            }
            _ => {
                tracing::info!("downloading model weights from registry");
                self.download_registry_file("weights").await?
            }
        };

        // Mmproj (optional)
        let mmproj_path = match &self.state.resolved_mmproj_path {
            Some(p) if !p.is_empty() => {
                tracing::info!(path = %p, "using local mmproj");
                Some(PathBuf::from(p))
            }
            _ => {
                if self.state.resolved_mmproj_filename.is_some() {
                    tracing::info!("downloading mmproj from registry");
                    Some(self.download_registry_file("mmproj").await?)
                } else {
                    tracing::debug!("no mmproj needed");
                    None
                }
            }
        };

        Ok((weights_path, mmproj_path))
    }

    async fn download_registry_file(&self, role: &str) -> crate::Result<PathBuf> {
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

        let component = self.make_model_component(model, quant, role);

        tracing::info!(
            role,
            quantization = %quant.name,
            source = %component.source_display(),
            size = quant.size_bytes,
            "downloading registry file"
        );

        self.cache
            .ensure(&component, self.cancel_flag.clone())
            .await
            .map_err(crate::Error::Download)
    }

    /// Create a Component for a model file, using the model's source to
    /// determine whether to use HF hub or a direct URL.
    fn make_model_component(
        &self,
        model: &Model,
        quant: &ModelFileQuantization,
        role: &str,
    ) -> Component {
        let source = if model.source == "hf" {
            ComponentSource::HuggingFace {
                repo_id: model.id.clone(),
                filename: quant.filename.clone(),
            }
        } else if let Some(ref url) = quant.download_url {
            ComponentSource::Url(url.clone())
        } else {
            // Fallback: construct HF URL from id + filename
            ComponentSource::HuggingFace {
                repo_id: model.id.clone(),
                filename: quant.filename.clone(),
            }
        };

        Component {
            role: if role == "weights" {
                ComponentRole::Weights
            } else {
                ComponentRole::MmprojWeights
            },
            source,
            zip_path: format!("/zip/{}", quant.filename),
            expected_size: Some(quant.size_bytes),
            expected_sha256: quant.sha256.clone(),
        }
    }

    async fn ensure_gpu_libs(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if self.gpu_libs.is_empty() {
            tracing::debug!("no GPU libraries to bundle");
            return paths;
        }

        tracing::info!(
            phase = %BuildPhase::PreparingGpuLibs,
            count = self.gpu_libs.len(),
            "preparing GPU libraries"
        );

        for lib in &self.gpu_libs {
            let accel_name = gpu_accel_name(lib.accel);
            tracing::info!(accel = accel_name, os = ?lib.os, "preparing GPU library");

            let component = gpu_lib_component(lib);
            match self.cache.ensure(&component, self.cancel_flag.clone()).await {
                Ok(path) => {
                    tracing::info!(path = %path.display(), "GPU library ready");
                    paths.push(path);
                }
                Err(e) => {
                    // GPU lib failures are non-fatal
                    tracing::warn!(
                        accel = accel_name,
                        os = ?lib.os,
                        error = %e,
                        "GPU library download failed, continuing without it"
                    );
                }
            }
        }

        paths
    }

    async fn create_executable(
        &self,
        zipalign_path: &Path,
        runtime_path: &Path,
        weights_path: &Path,
        mmproj_path: Option<&Path>,
        args_path: &str,
        gpu_lib_paths: &[PathBuf],
    ) -> crate::Result<PathBuf> {
        let output_path = PathBuf::from(&self.state.output_path);

        tracing::info!(
            phase = %BuildPhase::Assembling,
            output = %output_path.display(),
            "assembling executable"
        );

        // Copy runtime to output
        tracing::debug!(
            src = %runtime_path.display(),
            dest = %output_path.display(),
            "copying runtime to output"
        );
        crate::file_ops::copy_file(
            &runtime_path.to_string_lossy(),
            &output_path.to_string_lossy(),
        )
        .await
        .map_err(|e| crate::Error::Build(format!("Failed to copy runtime: {e}")))?;

        // Make executable
        crate::zipalign::make_executable(&output_path).await?;

        // Collect files to bundle
        let mut files_to_bundle: Vec<PathBuf> = Vec::new();

        tracing::debug!(path = args_path, "bundling .args");
        files_to_bundle.push(PathBuf::from(args_path));

        tracing::debug!(path = %weights_path.display(), "bundling model weights");
        files_to_bundle.push(weights_path.to_path_buf());

        if let Some(mmproj) = mmproj_path {
            tracing::debug!(path = %mmproj.display(), "bundling mmproj");
            files_to_bundle.push(mmproj.to_path_buf());
        }

        for gpu_path in gpu_lib_paths {
            tracing::debug!(path = %gpu_path.display(), "bundling GPU library");
            files_to_bundle.push(gpu_path.clone());
        }

        // Write agent prompt if enabled
        if self.agent_config.enabled {
            let prompt = args::generate_system_prompt(&self.agent_config);
            let prompt_path = crate::file_ops::write_temp_file(&prompt, ".args.prompt")
                .await
                .map_err(|e| crate::Error::Build(format!("Failed to write .args.prompt: {e}")))?;
            tracing::debug!("bundling agent prompt");
            files_to_bundle.push(PathBuf::from(prompt_path));
        }

        // Run zipalign
        let file_refs: Vec<&Path> = files_to_bundle.iter().map(|p| p.as_path()).collect();
        tracing::debug!(file_count = file_refs.len(), "running zipalign");

        let log_lines = crate::zipalign::zipalign_bundle(
            &output_path,
            &file_refs,
            Some(zipalign_path),
        )
        .await?;

        for line in &log_lines {
            tracing::debug!(zipalign = %line);
        }

        Ok(output_path)
    }

    fn registry_file_component(
        &self,
        model_id: &str,
        role: &str,
        _component_role: ComponentRole,
    ) -> Option<Component> {
        let model = self.config.models.iter().find(|m| m.id == model_id)?;
        let file = model.files.iter().find(|f| f.role == role)?;
        let sel = self.state.model_file_selections.get(role);
        let quant_idx = sel.map(|s| s.quantization_idx).unwrap_or(0);
        let quant = file.quantizations.get(quant_idx)?;
        Some(self.make_model_component(model, quant, role))
    }

    fn find_selected_release(&self) -> Option<LlamafileRelease> {
        let group_name = self.state.selected_version_group.as_ref()?;
        let version = self.state.selected_release_version.as_ref()?;
        let group = self.config.versions.iter().find(|g| &g.name == group_name)?;
        group.releases.iter().find(|r| &r.version == version).cloned()
    }

    fn find_gpu_lib(&self, accel: GpuAccel, os: Os) -> Option<GpuLibSelection> {
        let release = self.find_selected_release()?;
        let gpu_libs = release.gpu_libs.as_ref()?;
        let by_platform = match accel {
            GpuAccel::Cuda => gpu_libs.cuda.as_ref()?,
            GpuAccel::Rocm => gpu_libs.rocm.as_ref()?,
            GpuAccel::Vulkan => gpu_libs.vulkan.as_ref()?,
        };
        let lib = match os {
            Os::Linux => by_platform.linux.as_ref()?,
            Os::Windows => by_platform.windows.as_ref()?,
        };
        Some(GpuLibSelection {
            accel,
            os,
            source: ComponentSource::Url(lib.download_url.clone()),
            expected_size: Some(lib.size_bytes),
            expected_sha256: lib.sha256.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_utils::test_config;

    fn test_cache() -> Cache {
        Cache::new(PathBuf::from("/tmp/llamafile-test-cache"))
    }

    fn configured_builder() -> Builder {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();
        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);
        builder.select_registry_model("Qwen/Qwen3-0.6B-GGUF");
        builder
    }

    // ── resolved_args ──

    #[test]
    fn resolved_args_contains_model_path_and_defaults() {
        let builder = configured_builder();
        let args = builder.resolved_args();
        let lines: Vec<&str> = args.lines().collect();

        // Model path
        assert!(lines.contains(&"-m"));
        assert!(lines.iter().any(|l| l.contains("/zip/Qwen3-0.6B-Q8_0.gguf")));
        // Model path always present
        assert!(lines.contains(&"-m"));
        // Default values NOT emitted
        assert!(!lines.contains(&"--host"));
        // Always ends with ...
        assert!(lines.ends_with(&["..."]));
    }

    #[test]
    fn args_override_bypasses_schema_generation() {
        let mut builder = configured_builder();
        builder.set_args_override("-m\n/zip/custom.gguf\n...");
        assert_eq!(builder.resolved_args(), "-m\n/zip/custom.gguf\n...");
    }

    // ── resolved_components ──

    #[test]
    fn resolved_components_simple_model_has_runtime_weights_args() {
        let builder = configured_builder();
        let components = builder.resolved_components();

        let roles: Vec<_> = components.iter().map(|c| &c.role).collect();
        assert!(
            roles.iter().any(|r| matches!(r, ComponentRole::Runtime)),
            "missing Runtime"
        );
        assert!(
            roles.iter().any(|r| matches!(r, ComponentRole::Weights)),
            "missing Weights"
        );
        assert!(
            roles.iter().any(|r| matches!(r, ComponentRole::ArgsFile)),
            "missing ArgsFile"
        );
        // qwen3-0.6b has no mmproj
        assert!(
            !roles
                .iter()
                .any(|r| matches!(r, ComponentRole::MmprojWeights)),
            "unexpected MmprojWeights"
        );
    }

    #[test]
    fn resolved_components_multimodal_includes_mmproj() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();
        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);
        builder.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF");

        let components = builder.resolved_components();
        let roles: Vec<_> = components.iter().map(|c| &c.role).collect();
        assert!(roles.iter().any(|r| matches!(r, ComponentRole::Weights)));
        assert!(
            roles
                .iter()
                .any(|r| matches!(r, ComponentRole::MmprojWeights)),
            "multimodal model should include MmprojWeights"
        );
    }

    #[test]
    fn resolved_components_gpu_libs_appear() {
        let mut builder = configured_builder();
        builder.add_gpu_lib(GpuAccel::Cuda, Os::Linux);
        builder.add_custom_gpu_lib(Path::new("/my/ggml-rocm.so"), GpuAccel::Rocm, Os::Linux);

        let components = builder.resolved_components();
        let gpu_components: Vec<_> = components
            .iter()
            .filter(|c| matches!(c.role, ComponentRole::GpuLib { .. }))
            .collect();
        assert_eq!(gpu_components.len(), 2);

        // CUDA should be a URL source (from registry)
        assert!(matches!(
            &gpu_components.iter().find(|c| matches!(c.role, ComponentRole::GpuLib { accel: GpuAccel::Cuda, .. })).unwrap().source,
            ComponentSource::Url(_)
        ));
        // ROCm should be a local source
        assert!(matches!(
            &gpu_components.iter().find(|c| matches!(c.role, ComponentRole::GpuLib { accel: GpuAccel::Rocm, .. })).unwrap().source,
            ComponentSource::Local(_)
        ));
    }

    #[test]
    fn resolved_components_no_agent_prompt_by_default() {
        let builder = configured_builder();
        let components = builder.resolved_components();
        assert!(
            !components
                .iter()
                .any(|c| matches!(c.role, ComponentRole::ArgsPrompt)),
            "ArgsPrompt should not appear when agent disabled"
        );
    }

    #[test]
    fn resolved_components_custom_model_uses_local_source() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();
        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);
        builder.select_custom_model(Path::new("/my/weights.gguf"));

        let components = builder.resolved_components();
        let weights = components
            .iter()
            .find(|c| matches!(c.role, ComponentRole::Weights))
            .expect("should have Weights");
        assert_eq!(
            weights.source,
            ComponentSource::Local(PathBuf::from("/my/weights.gguf"))
        );
    }

    #[test]
    fn resolved_components_quantization_change_updates_url() {
        let config = test_config();
        let group_name = config.versions[0].name.clone();
        let release_version = config.versions[0].releases[0].version.clone();
        let mut builder = Builder::new(config, test_cache());
        builder.select_version(&group_name, &release_version);
        builder.select_registry_model("mistralai/Ministral-3-3B-Instruct-2512-GGUF");

        // Default (index 0) is Q4_K_M
        let c1 = builder.resolved_components();
        let w1 = c1.iter().find(|c| matches!(c.role, ComponentRole::Weights)).unwrap();
        assert!(w1.zip_path.contains("Q4_K_M"));

        // Switch to BF16 (index 1)
        builder.select_quantization("weights", 1);
        let c2 = builder.resolved_components();
        let w2 = c2.iter().find(|c| matches!(c.role, ComponentRole::Weights)).unwrap();
        assert!(w2.zip_path.contains("BF16"));
    }

    // ── GPU lib management ──

    #[test]
    fn gpu_lib_deduplication() {
        let mut builder = configured_builder();

        builder.add_gpu_lib(GpuAccel::Cuda, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 1);

        // Same (accel, os) replaces, not duplicates
        builder.add_gpu_lib(GpuAccel::Cuda, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 1);

        // Different accel adds (ROCm Linux is in our test data)
        builder.add_gpu_lib(GpuAccel::Rocm, Os::Linux);
        assert_eq!(builder.gpu_libs.len(), 2);

        // Custom libs always add regardless of registry data
        builder.add_custom_gpu_lib(Path::new("/x.dll"), GpuAccel::Cuda, Os::Windows);
        assert_eq!(builder.gpu_libs.len(), 3);
    }

    #[test]
    fn gpu_lib_vulkan_resolves_to_registry_url() {
        // 0.10.1_alpha (first release of the default group) ships vulkan libs for
        // both Linux and Windows. Regression guard for the original bug where
        // GpuLibs was missing its vulkan field and serde silently dropped the section.
        let mut builder = configured_builder();

        builder.add_gpu_lib(GpuAccel::Vulkan, Os::Linux);
        builder.add_gpu_lib(GpuAccel::Vulkan, Os::Windows);
        assert_eq!(builder.gpu_libs.len(), 2);

        let components: Vec<_> = builder
            .resolved_components()
            .into_iter()
            .filter(|c| matches!(c.role, ComponentRole::GpuLib { accel: GpuAccel::Vulkan, .. }))
            .collect();
        assert_eq!(components.len(), 2);

        // Both must resolve to a URL from the catalog (not None, not Local).
        for c in &components {
            match &c.source {
                ComponentSource::Url(u) => assert!(u.contains("vulkan"), "url is {u}"),
                other => panic!("expected Url source, got {other:?}"),
            }
            // Size + sha256 must propagate from versions.json so the cache layer
            // can short-circuit on a full match and the download layer can verify.
            assert!(c.expected_size.is_some(), "expected_size should be populated");
            assert!(c.expected_sha256.is_some(), "expected_sha256 should be populated");
        }

        // Filenames should map to the OS-appropriate extension.
        let zip_paths: Vec<_> = components.iter().map(|c| c.zip_path.clone()).collect();
        assert!(zip_paths.contains(&"ggml-vulkan.so".to_string()));
        assert!(zip_paths.contains(&"ggml-vulkan.dll".to_string()));
    }

    // ── Validation ──

    #[test]
    fn validate_catches_missing_model_and_version() {
        let builder = Builder::new(test_config(), test_cache());
        let msgs = builder.validate();
        let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
        assert!(errors.len() >= 2, "should flag missing model and version");
        assert!(!builder.can_build());
    }

    #[test]
    fn validate_passes_with_full_config() {
        let builder = configured_builder();
        let msgs = builder.validate();
        let errors: Vec<_> = msgs.iter().filter(|m| m.is_error()).collect();
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert!(builder.can_build());
    }

    #[test]
    fn validate_warns_on_gpu_on_macos() {
        let mut builder = configured_builder();
        builder.add_custom_gpu_lib(Path::new("/x.so"), GpuAccel::Cuda, Os::Linux);
        let msgs = builder.validate();

        if cfg!(target_os = "macos") {
            assert!(
                msgs.iter().any(|m| matches!(m, ValidationMessage::Warning(s) if s.contains("macOS"))),
                "should warn about GPU libs on macOS"
            );
        }
    }

    // ── Disk-space pre-check ──

    #[test]
    fn estimate_disk_requirements_for_default_qwen_build() {
        // The default Qwen build pulls runtime + zipalign over HTTP and
        // weights from HuggingFace. The pre-check should report space at
        // three destinations: cache, hf cache, and output. Isolate the HF
        // cache so the test doesn't false-pass when the developer's machine
        // already has the model cached locally.
        let hf_tmp = tempfile::TempDir::new().unwrap();
        temp_env::with_vars(
            [
                ("HF_HOME", Some(hf_tmp.path().to_string_lossy().to_string())),
                ("HUGGINGFACE_HUB_CACHE", None),
            ],
            || {
                // Same isolation for the URL cache so existing cached
                // runtime/zipalign don't make the Cache bucket disappear.
                let cache_tmp = tempfile::TempDir::new().unwrap();
                let config = test_config();
                let group_name = config.versions[0].name.clone();
                let release_version = config.versions[0].releases[0].version.clone();
                let mut builder =
                    Builder::new(config, Cache::new(cache_tmp.path().to_path_buf()));
                builder.select_version(&group_name, &release_version);
                builder.select_registry_model("Qwen/Qwen3-0.6B-GGUF");
                builder.set_output_path(Path::new("/tmp/test-out.llamafile"));

                let reqs = builder.estimate_disk_requirements();
                let purposes: Vec<_> = reqs.iter().map(|r| r.purpose).collect();
                assert!(
                    purposes.contains(&crate::disk_space::Purpose::Cache),
                    "expected Cache requirement, got {:?}",
                    purposes
                );
                assert!(
                    purposes.contains(&crate::disk_space::Purpose::HuggingFaceCache),
                    "expected HuggingFaceCache requirement, got {:?}",
                    purposes
                );
                assert!(
                    purposes.contains(&crate::disk_space::Purpose::Output),
                    "expected Output requirement, got {:?}",
                    purposes
                );
                for r in &reqs {
                    assert!(r.bytes > 0, "requirement {:?} had zero bytes", r);
                }
            },
        );
    }

    #[test]
    fn estimate_disk_requirements_empty_for_unconfigured_builder() {
        // Builder with no model and no version selected resolves no
        // downloadable components beyond .args (Generated). Nothing to
        // require, nothing to fail on.
        let builder = Builder::new(test_config(), test_cache());
        let reqs = builder.estimate_disk_requirements();
        assert!(
            reqs.iter().all(|r| r.bytes == 0)
                || reqs.is_empty(),
            "got non-trivial requirements from unconfigured builder: {:?}",
            reqs
        );
    }

    // ── Defaults ──

    #[test]
    fn select_default_version_picks_marked_default() {
        let mut builder = Builder::new(test_config(), test_cache());
        assert!(builder.select_default_version());
        // versions.json has llamafile 0.10 marked default
        assert!(builder
            .state()
            .selected_version_group
            .as_deref()
            .unwrap()
            .contains("0.10"));
    }

    #[test]
    fn select_default_model_picks_marked_default() {
        let mut builder = Builder::new(test_config(), test_cache());
        assert!(builder.select_default_model());
        // models.json has qwen3-0.6b marked default
        assert_eq!(
            builder.state().selected_model_id.as_deref(),
            Some("Qwen/Qwen3-0.6B-GGUF")
        );
    }

    // ── HuggingFace source ──

    #[test]
    fn resolved_components_hf_model_uses_huggingface_source() {
        let builder = configured_builder();
        let components = builder.resolved_components();
        let weights = components
            .iter()
            .find(|c| matches!(c.role, ComponentRole::Weights))
            .expect("should have Weights");
        assert!(
            matches!(&weights.source, ComponentSource::HuggingFace { repo_id, filename }
                if repo_id == "Qwen/Qwen3-0.6B-GGUF" && filename == "Qwen3-0.6B-Q8_0.gguf"),
            "weights should use HuggingFace source, got {:?}",
            weights.source
        );
    }

    // ── Zipalign ──

    #[test]
    fn resolved_components_includes_zipalign_from_release() {
        let builder = configured_builder();
        let components = builder.resolved_components();
        let zipalign = components
            .iter()
            .find(|c| matches!(c.role, ComponentRole::Zipalign));
        assert!(zipalign.is_some(), "should include Zipalign component");
        let zipalign = zipalign.unwrap();
        assert!(
            matches!(&zipalign.source, ComponentSource::Url(url) if url.contains("zipalign")),
            "zipalign source should be a URL containing 'zipalign', got {:?}",
            zipalign.source
        );
    }

    #[test]
    fn resolved_components_uses_custom_zipalign_when_set() {
        let mut builder = configured_builder();
        builder.use_custom_zipalign(Path::new("/my/custom/zipalign"));
        let components = builder.resolved_components();
        let zipalign = components
            .iter()
            .find(|c| matches!(c.role, ComponentRole::Zipalign))
            .expect("should include Zipalign component");
        assert_eq!(
            zipalign.source,
            ComponentSource::Local(PathBuf::from("/my/custom/zipalign"))
        );
    }

    // ── End-to-end build pipeline ──

    /// Run the full 5-phase pipeline against an isolated tempdir, with all
    /// remote dependencies (zipalign, runtime, model weights) substituted by
    /// local stub files. This is the only test that calls `build()` itself
    /// rather than its individual helpers; it's the regression guard for the
    /// pipeline's wiring (phase ordering, output assembly, error
    /// classification).
    ///
    /// The stub `zipalign` is a shell script that exits 0. That's enough
    /// because `create_executable` copies the runtime to the output path
    /// *before* invoking zipalign, so the output file already exists when
    /// zipalign runs. The contents of the produced llamafile are not
    /// asserted — those are the responsibility of the real zipalign and
    /// llamafile binaries, both of which are out of scope here.
    #[tokio::test]
    #[cfg(unix)]
    async fn build_pipeline_runs_end_to_end_with_stubbed_artefacts() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        let runtime_path = root.join("fake-runtime");
        tokio::fs::write(&runtime_path, b"#!/bin/sh\nexit 0\n").await.unwrap();

        let model_path = root.join("fake-weights.gguf");
        // Plausible-looking GGUF magic bytes + some padding so file size is non-trivial.
        let mut gguf = b"GGUF".to_vec();
        gguf.extend_from_slice(&[0u8; 1024]);
        tokio::fs::write(&model_path, &gguf).await.unwrap();

        let zipalign_path = root.join("fake-zipalign");
        tokio::fs::write(
            &zipalign_path,
            b"#!/bin/sh\n# stub zipalign for tests: succeed without modifying $2\nexit 0\n",
        ).await.unwrap();
        crate::zipalign::make_executable(&zipalign_path).await.unwrap();

        let output_path = root.join("out.llamafile");
        let cache = Cache::new(root.join("cache"));
        let mut builder = Builder::new(test_config(), cache);
        builder.use_custom_zipalign(&zipalign_path);
        builder.use_custom_runtime(&runtime_path);
        builder.select_custom_model(&model_path);
        builder.set_output_path(&output_path);

        // Sanity check: the configuration we just built must be buildable.
        let validation_errors: Vec<_> = builder.validate().iter().filter_map(|m| match m {
            crate::ValidationMessage::Error(s) => Some(s.clone()),
            _ => None,
        }).collect();
        assert!(
            validation_errors.is_empty(),
            "test setup produced validation errors: {validation_errors:?}",
        );

        let result = builder.build().await.expect("build should succeed");

        assert_eq!(result, output_path, "build should return the configured output path");
        assert!(output_path.exists(), "output file should have been written");
        // The runtime was copied to the output path before zipalign ran;
        // its contents should still be readable from the result.
        let written = tokio::fs::read(&output_path).await.unwrap();
        assert_eq!(written, b"#!/bin/sh\nexit 0\n", "output should contain the runtime bytes");
    }

    /// If the stubbed zipalign fails, `build()` must surface a `Build` error
    /// rather than reporting success. Guards against the regression of
    /// silently swallowing a non-zero exit code.
    #[tokio::test]
    #[cfg(unix)]
    async fn build_pipeline_propagates_zipalign_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        let runtime_path = root.join("fake-runtime");
        tokio::fs::write(&runtime_path, b"runtime").await.unwrap();
        let model_path = root.join("fake-weights.gguf");
        tokio::fs::write(&model_path, b"GGUF\0\0\0\0").await.unwrap();

        let zipalign_path = root.join("failing-zipalign");
        tokio::fs::write(
            &zipalign_path,
            b"#!/bin/sh\necho 'simulated zipalign failure' >&2\nexit 7\n",
        ).await.unwrap();
        crate::zipalign::make_executable(&zipalign_path).await.unwrap();

        let cache = Cache::new(root.join("cache"));
        let mut builder = Builder::new(test_config(), cache);
        builder.use_custom_zipalign(&zipalign_path);
        builder.use_custom_runtime(&runtime_path);
        builder.select_custom_model(&model_path);
        builder.set_output_path(&root.join("out.llamafile"));

        let err = builder.build().await.expect_err("build should fail when zipalign exits non-zero");
        match err {
            crate::Error::Build(msg) => {
                assert!(msg.contains("zipalign") && msg.contains('7'),
                    "error should mention zipalign and the exit code, got: {msg}");
            }
            other => panic!("expected Error::Build, got {other:?}"),
        }
    }

    /// Cancellation flipped between phases must short-circuit the pipeline
    /// with `Error::Cancelled` rather than running to completion.
    #[tokio::test]
    #[cfg(unix)]
    async fn build_pipeline_honours_cancel_flag() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();

        let runtime_path = root.join("fake-runtime");
        tokio::fs::write(&runtime_path, b"runtime").await.unwrap();
        let model_path = root.join("fake-weights.gguf");
        tokio::fs::write(&model_path, b"GGUF\0\0\0\0").await.unwrap();
        let zipalign_path = root.join("fake-zipalign");
        tokio::fs::write(&zipalign_path, b"#!/bin/sh\nexit 0\n").await.unwrap();
        crate::zipalign::make_executable(&zipalign_path).await.unwrap();

        let cache = Cache::new(root.join("cache"));
        let mut builder = Builder::new(test_config(), cache);
        builder.use_custom_zipalign(&zipalign_path);
        builder.use_custom_runtime(&runtime_path);
        builder.select_custom_model(&model_path);
        builder.set_output_path(&root.join("out.llamafile"));

        let cancel = Arc::new(AtomicBool::new(false));
        builder.set_cancel_flag(cancel.clone());
        // Pre-flip the flag. `build()` resets it, then re-checks between
        // phases — so we need to flip it again after the reset by spawning
        // a flipper. The simplest approach: just flip *during* the build via
        // a spawned task with a tiny delay. The pipeline's `check_cancelled`
        // calls between phases will then observe it.
        let cancel_flipper = cancel.clone();
        tokio::spawn(async move {
            // Yield a few times so the build progresses past phase 0/1.
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }
            cancel_flipper.store(true, Ordering::Relaxed);
        });

        let err = builder.build().await.expect_err("cancelled build should error");
        // Cancellation may surface as Error::Cancelled directly, or as a
        // download error wrapping cancellation if it lands inside ensure_*.
        assert!(
            matches!(err, crate::Error::Cancelled | crate::Error::Download(crate::DownloadError::Cancelled)),
            "expected Cancelled, got {err:?}"
        );
    }
}

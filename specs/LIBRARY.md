# llamafile-core Library API

This document describes the public API of `llamafile-core`, the library crate that all interface layers (CLI, JSON-RPC, HTTP server, web UI) call.

**Zero UI dependencies.** The library depends only on: `tokio`, `reqwest`, `serde`, `serde_json`, `sha2`, `hex`, `thiserror`, `tracing`, `directories`, `dirs`, `futures-util`, `url`, `hf-hub`.

**Observability via tracing.** All I/O operations emit structured `tracing` spans and events. The library never prints to stdout/stderr or owns any progress display.


## Quick start

```rust
use llamafile_core::{Builder, BuilderConfig, Cache, ConfigManager};

// 1. Load configuration
let cm = ConfigManager::new(config_dir);
let config = cm.load()?;

// 2. Create cache and builder
let cache = Cache::new(cache_dir);
let mut builder = Builder::new(config, cache);

// 3. Configure the build
builder.select_default_version();  // or select_version("llamafile 0.10", "0.10.0")
builder.select_registry_model("Qwen/Qwen3-0.6B-GGUF");

// 4. Build
let output = builder.build().await?;
println!("Built: {}", output.display());
```

Note: model IDs are HuggingFace repo IDs (e.g. `"Qwen/Qwen3-0.6B-GGUF"`) for models with `source: "hf"`.


## ConfigManager

Loads, caches, and updates the JSON configuration files (versions, models, config schema).

```rust
ConfigManager::new(config_dir: PathBuf) -> Self
    .with_remote_url(url: &str) -> Self       // default: GitHub raw content
    .with_update_mode(mode: UpdateMode) -> Self // default: Ask

    .update_mode() -> UpdateMode
    .load() -> Result<BuilderConfig>            // from disk, fallback to bundled
    .check_for_updates() -> Result<UpdateStatus> // async, fetches from remote
    .apply_update(update: &ConfigUpdate) -> Result<()>
```

| Type | Variants |
|------|----------|
| `UpdateMode` | `Always`, `Never`, `Ask` |
| `UpdateStatus` | `UpToDate`, `ConfigUpdateAvailable(ConfigUpdate)`, `BuilderUpdateRequired { min_version }` |

### BuilderConfig

Returned by `ConfigManager::load()`. Contains all data needed by the Builder.

| Field | Type |
|-------|------|
| `versions` | `Vec<LlamafileVersionGroup>` |
| `models` | `Vec<Model>` |
| `config_schema` | `Vec<ConfigSchemaItem>` |
| `config_version` | `u32` |
| `min_builder_version` | `Option<String>` |


## Cache

Manages downloaded files. HuggingFace models go to `~/.cache/huggingface/hub/` (shared with llama.cpp). Non-HF files (runtimes, GPU libs) go to `base_dir/`.

```rust
Cache::new(base_dir: PathBuf) -> Self
    .with_hf_token(token: String) -> Self

    .runtime_dir() -> PathBuf              // base_dir/bin/
    .gpu_lib_dir(version: &str) -> PathBuf // base_dir/v/<version>/
    .config_dir() -> PathBuf               // base_dir/config/
    .path_for(component: &Component) -> Option<PathBuf>
    .check(component: &Component) -> CacheStatus
    .ensure(component: &Component, cancel_flag: Arc<AtomicBool>) -> Result<PathBuf> // async
```

| Type | Variants |
|------|----------|
| `CacheStatus` | `Valid(PathBuf)`, `Partial { path, bytes_downloaded }`, `Missing` |


## Builder

The main entry point. Stateful: call selection methods to configure, then `build()` to execute.

### Construction

```rust
Builder::new(config: BuilderConfig, cache: Cache) -> Self
```

### State queries

```rust
.state() -> &BuildState                     // current internal state
.config() -> &BuilderConfig                 // loaded config data
.resolved_components() -> Vec<Component>    // what build() would assemble
.resolved_args() -> String                  // .args content preview
.validate() -> Vec<ValidationMessage>       // warnings and errors
.can_build() -> bool                        // no blocking errors?
```

### Version / binaries

```rust
.select_default_version() -> bool              // picks version group with default=true
.select_version(group: &str, version: &str)
.use_custom_runtime(path: &Path)
.use_custom_zipalign(path: &Path)
```

### Model selection

```rust
.select_default_model() -> bool                // picks model with default=true
.select_registry_model(model_id: &str)
.select_custom_model(path: &Path)
.select_ollama_model(model: OllamaModel)
.select_lmstudio_model(model: LMStudioModel)
.select_quantization(role: &str, index: usize)         // change quantization for a role
.use_custom_file_for_role(role: &str, path: &Path)     // override a registry file with local
.add_custom_companion_file(role: &str, path: &Path)    // add mmproj to custom model
.remove_custom_companion_file(role: &str)               // remove companion file
.select_lmstudio_file(role: &str, index: usize)        // select within LM Studio model
```

**How registry model selection works:**

A registry model (from `models.json`) has one or more **files**, each with a **role** (`"weights"`, `"mmproj"`) and multiple **quantization** variants (e.g. Q4_K_M, Q8_0, BF16). For example, Ministral 3 has:

- `weights` role: Q4_K_M (index 0), BF16 (index 1)
- `mmproj` role: BF16 (index 0)

When you call `select_registry_model("ministral-3-3b-instruct-2512")`:
1. The model is looked up by ID in the loaded config
2. A `FileSelection` is created for **each role** with `quantization_idx: 0` (first available)
3. Output path and resolved filenames are computed from the default quantizations

To change the quantization for a specific role:
```rust
builder.select_quantization("weights", 1);  // switch weights to BF16
builder.select_quantization("mmproj", 0);   // mmproj stays at BF16 (only option)
```

To override a specific role with a custom local file:
```rust
// Use your own weights file instead of the registry one
builder.use_custom_file_for_role("weights", Path::new("/path/to/custom.gguf"));
```

For custom models (not from registry), add companion files:
```rust
builder.select_custom_model(Path::new("/path/to/weights.gguf"));
builder.add_custom_companion_file("mmproj", Path::new("/path/to/mmproj.gguf"));
```

The `resolved_components()` output reflects all these per-role selections.

### Configuration

```rust
.set_config_value(key: &str, value: serde_json::Value)
.set_args_override(raw_args: &str)   // bypass schema generation
.clear_args_override()
```

### GPU libraries

```rust
.add_gpu_lib(accel: GpuAccel, os: Os)                         // from registry
.add_custom_gpu_lib(path: &Path, accel: GpuAccel, os: Os)     // from disk
.remove_gpu_lib(accel: GpuAccel, os: Os)
.clear_gpu_libs()
```

### Agent

```rust
.set_agent_config(config: AgentConfig)   // placeholder, FR-7
```

### Output

```rust
.set_output_path(path: &Path)
```

### Execution

```rust
.cancel()                          // sets cancel flag
.build() -> Result<PathBuf>       // async, runs the 5-phase pipeline
```

Build phases (all observable via tracing):
1. `ensure_zipalign` -- download or locate zipalign binary
2. `ensure_runtime` -- download or locate the llamafile binary
3. `ensure_model` -- download or locate model weights (and mmproj)
4. `write_config` -- generate .args from schema or override
5. `ensure_gpu_libs` -- download GPU libraries (non-fatal on failure)
6. `create_executable` -- copy runtime, run zipalign to bundle everything


## Component model

Every selection resolves to a list of `Component`s via `resolved_components()`.

```rust
struct Component {
    role: ComponentRole,
    source: ComponentSource,
    zip_path: String,             // path inside the llamafile, e.g. "/zip/model.gguf"
    expected_size: Option<u64>,
    expected_sha256: Option<String>,
}
```

| `ComponentRole` | Description |
|-----------------|-------------|
| `Runtime` | llamafile APE binary (base of output, not zipped) |
| `Zipalign` | zipalign tool (used during build, not bundled) |
| `Weights` | primary model weights (.gguf) |
| `MmprojWeights` | multimodal projector weights (.gguf) |
| `GpuLib { accel, os }` | GPU acceleration library |
| `ArgsFile` | .args configuration |
| `ArgsPrompt` | .args.prompt for agent mode |

| `ComponentSource` | Description |
|-------------------|-------------|
| `HuggingFace { repo_id, filename }` | download via hf-hub shared cache |
| `Url(String)` | download via HTTP (runtimes, GPU libs) |
| `Local(PathBuf)` | file already on disk |
| `Generated` | created at build time (.args, .args.prompt) |

| `GpuAccel` | `Os` |
|------------|------|
| `Cuda`, `Rocm`, `Vulkan` | `Linux`, `Windows` |

Metal is not in `GpuAccel` -- it is compiled on-the-fly from sources embedded in the llamafile binary.


## Validation

```rust
validation::validate(state: &BuildState, config: &BuilderConfig) -> Vec<ValidationMessage>
```

`Builder::validate()` calls this and adds GPU-on-wrong-platform and agent shell tool checks.

| `ValidationMessage` | Meaning |
|--------------------|---------|
| `Warning(String)` | build can proceed |
| `Error(String)` | build cannot proceed |

Checks performed:
- Model selected
- Version selected
- Output path set
- Context length vs model training length
- Large context (32K+) on CPU warning
- Custom binary exists and is executable
- GPU libs on macOS warning (Metal is built-in)
- Agent shell tool enabled warning

Note: zipalign availability is not checked at validation time -- the builder downloads it automatically during the build (Phase 5c).


## Model discovery

Standalone functions, no Builder needed:

```rust
search_models(models: &[Model], query: Option<&str>, tag: Option<&str>) -> Vec<&Model>
system_info() -> SystemInfo
ollama::get_default_ollama_cache_dir() -> Option<String>
ollama::check_ollama_available(custom_dir: Option<&str>) -> bool
ollama::get_ollama_models(custom_dir: Option<&str>) -> Vec<OllamaModel>
lmstudio::get_default_lmstudio_cache_dir() -> Option<String>
lmstudio::check_lmstudio_available(custom_dir: Option<&str>) -> bool
lmstudio::get_lmstudio_models(custom_dir: Option<&str>) -> Vec<LMStudioModel>
```


## Args generation

```rust
args::generate_args_from_schema(
    schema: &[ConfigSchemaItem],
    config_values: &HashMap<String, Value>,
    model_filename: &str,
    mmproj_filename: Option<&str>,
) -> String

args::generate_system_prompt(agent: &AgentConfig) -> String
args::estimate_size(model_size: u64, compute: &ComputeConfig, agent: &AgentConfig) -> u64
args::default_config_values(schema: &[ConfigSchemaItem]) -> HashMap<String, Value>
```


## Error types

```rust
enum Error {
    Io(std::io::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Download(DownloadError),
    Build(String),
    Config(String),
    Validation(String),
    NotFound(String),
    Cancelled,
}

enum DownloadError {
    Http(reqwest::Error),
    Io(std::io::Error),
    ChecksumMismatch { expected, actual },
    SizeMismatch { expected, actual },
    Cancelled,
    AuthRequired { status: u16 },
    ResumeNotSupported,
}
```


## Data types

### Versions

| Type | Key fields |
|------|-----------|
| `LlamafileVersionGroup` | `name`, `tag` (latest/stable), `default`, `description`, `releases: Vec<LlamafileRelease>` |
| `LlamafileRelease` | `version`, `release_date`, `download_url`, `size_bytes`, `sha256`, `zipalign_url`, `zipalign_size_bytes`, `zipalign_sha256`, `gpu_libs: Option<GpuLibs>` |
| `GpuLibs` | `cuda: Option<GpuLibsByPlatform>`, `rocm: Option<GpuLibsByPlatform>` |
| `GpuLibsByPlatform` | `linux: Option<GpuLibrary>`, `windows: Option<GpuLibrary>` |
| `GpuLibrary` | `download_url`, `size_bytes`, `sha256` |

### Models

| Type | Key fields |
|------|-----------|
| `Model` | `id` (HF repo ID for HF sources), `source` ("hf" for HuggingFace), `name`, `organization`, `parameters`, `param_count`, `description`, `license`, `default`, `tags`, `context_length`, `files: Vec<ModelFile>` |
| `ModelFile` | `role` (weights/mmproj), `label`, `quantizations: Vec<ModelFileQuantization>` |
| `ModelFileQuantization` | `name` (Q4_K_M, Q8_0, etc.), `filename`, `size_bytes`, `download_url: Option` (fallback for non-HF), `sha256` |

### Configuration schema

`ConfigSchemaItem` is a tagged enum:

| Variant | Key fields |
|---------|-----------|
| `Section` | `label`, `key` (toggle), `default`, `flag`, `items: Vec<ConfigSchemaItem>` |
| `Text` | `key`, `label`, `description`, `default`, `value_type`, `flag`, `conditional` |
| `Dropdown` | `key`, `label`, `description`, `default`, `options`, `flag`, `conditional` |
| `Bool` | `key`, `label`, `description`, `default`, `flag`, `conditional` |
| `Slider` | `key`, `label`, `description`, `default`, `min`, `max`, `step`, `flag`, `conditional` |

### Compute & Agent

| Type | Key fields |
|------|-----------|
| `ComputeConfig` | `mode` (Cpu/Gpu), `cuda`, `rocm`, `metal` (bools) |
| `AgentConfig` | `enabled`, `tools: Vec<AgentTool>`, `system_prompt` |
| `AgentTool` | `id`, `name`, `description`, `icon`, `enabled` |

### System info

| Type | Key fields |
|------|-----------|
| `SystemInfo` | `os`, `arch`, `cpu_cores`, `memory_gb`, `has_nvidia_gpu`, `has_amd_gpu`, `has_metal` |

### Local model sources

| Type | Key fields |
|------|-----------|
| `OllamaModel` | `model_name`, `tag`, `display_name`, `weights_path`, `size_bytes` |
| `LMStudioModel` | `dir_name`, `display_name`, `dir_path`, `files: Vec<LMStudioFile>`, `total_size_bytes` |
| `LMStudioFile` | `filename`, `path`, `size_bytes`, `role` |

### Build state

| Type | Key fields |
|------|-----------|
| `BuildState` | `model_source`, `selected_model_id`, `model_file_selections`, `custom_model_files`, `selected_ollama_model`, `selected_lmstudio_model`, `llamafile_source`, `selected_version_group`, `selected_release_version`, `custom_llamafile_path`, `config_values`, `output_path`, resolved paths |
| `ModelSource` | `Registry`, `Custom`, `Ollama`, `LMStudio` |
| `FileSource` | `Registry`, `Custom` |
| `LlamafileSource` | `Registry`, `Custom` |


## Test coverage

103 tests across core library (100) and CLI (3):

| Module | Tests | What's covered |
|--------|-------|----------------|
| `state` | 20 | All model sources, file selection, output path, resolved paths |
| `args` | 16 | Schema-driven .args generation, system prompt, size estimation, defaults |
| `download` | 15 | HF URL detection, token resolution, file info, checksums |
| `builder` | 17 | resolved_components (simple, multimodal, GPU, agent, custom, quantization), HF source, zipalign, defaults, validation |
| `cache` | 5 | Cache status: local existing/missing, generated, URL partial/complete/missing |
| `config_manager` | 6 | Load, fallback, invalid JSON, bare array rejection, apply update, version comparison |
| `validation` | 5 | Required components, context length, custom binary, large context |
| `ollama` | 6 | Availability, scanning, missing blobs, empty dirs |
| `lmstudio` | 7 | Availability, scanning, role inference, non-gguf filtering |
| CLI `state_file` | 3 | SavedState round-trip, build_from_state with explicit config, build_from_state with defaults |

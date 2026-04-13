# Architecture

This document describes how the llamafile-builder will be structured to satisfy the requirements in [README.md](README.md). It builds on the original architecture proposal and fills gaps identified during review.


## Core decision

Split the builder into a **core library** (pure Rust, no UI dependencies) and **thin interface layers** that expose the library's capabilities through different channels.

This directly addresses the central problem: the builder's logic is currently trapped inside a renderer-dependent binary. By extracting it into a library, any interface -- CLI, HTTP server, TUI, or a future native GUI -- can use it without pulling in each other's dependencies.

```
llamafile-core (library crate, zero UI deps)
       |
       +-- llamafile-cli (single binary)
       |       |
       |       +-- CLI mode (default)
       |       |     $ llamafile-builder build --model llama3 -o out.llamafile
       |       |
       |       +-- Server mode (--serve)
       |       |     $ llamafile-builder --serve --port 8765
       |       |     HTTP + SSE on localhost, serves web UI
       |       |
       |       +-- Interactive mode (--interactive)
       |             $ llamafile-builder --interactive
       |             JSON-RPC over stdio
       |
       +-- llamafile-tui (optional separate binary)
               ratatui, links directly to core
```


## Requirements traceability

Before detailing the design, here is how each requirement maps to an architectural element.

| Requirement | Addressed by |
|-------------|-------------|
| FR-1 Component selection | Core library: `Builder` API (version, model, config, compute, agent methods) |
| FR-2 Model discovery | Core library: registry queries, Ollama/LM Studio scanners |
| FR-3 Download management | Core library: `hf-hub` crate for HF models (shared cache with llama.cpp), custom `Downloader` for non-HF files |
| FR-4 Configuration generation | Core library: schema-driven `.args` generation + raw `.args` override |
| FR-5 Build assembly | Core library: `Builder::build()` orchestrates zipalign |
| FR-6 Build validation | Core library: `Builder::validate()` returns warnings/errors |
| FR-7 Agent configuration | Core library: `AgentConfig` on `Builder` |
| NFR-1 Platform independence | Core library has zero UI/renderer dependencies |
| NFR-2 Headless operation | CLI mode, no display server needed |
| NFR-3 Scriptability | CLI (exit codes, stdout) + JSON-RPC (stdio) + HTTP (REST + SSE) |
| NFR-4 Graphical access | Server mode serves web UI to any browser |
| NFR-5 Progressive complexity | CLI defaults; wizard flow in web UI |
| NFR-6 Platform coverage | Core is pure Rust; interfaces are lightweight |
| NFR-7 Offline capability | `hf-hub` cache for models; `Cache` for runtimes, GPU libs, configs |
| NFR-8 Interoperability | `hf-hub` crate uses standard HF cache layout (`~/.cache/huggingface/hub/`), shared with llama.cpp and Python tools |
| NFR-9 Remote configuration | Core library: `ConfigManager` fetches, versions, and caches config files |
| NFR-10 Update checking | Core library: `ConfigManager` handles update check with three modes |
| NFR-11 Web UI as separate assets | Server mode serves static files from a known directory on disk |
| NFR-12 Llamafile modification | `Builder` operates on components, not monolithic state; future `open()` feasible |


## Core library (`llamafile-core`)

The library is the single source of truth for all builder logic. It has no UI, no CLI parsing, no HTTP handling. Its public API is what every interface layer calls.

### Dependencies

Only what the core logic needs:

```toml
[dependencies]
tokio = { version = "1", features = ["fs", "sync", "rt-multi-thread"] }
hf-hub = { version = "0.4", features = ["tokio"] }  # HF downloads + shared cache
reqwest = { version = "0.12", features = ["stream"] } # non-HF downloads (runtimes, GPU libs)
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
hex = "0.4"
thiserror = "2"
directories = "6"
```

No `clap`, no `axum`, no `ratatui`, no rendering libraries of any kind.

### Configuration management (NFR-9, NFR-10)

The current codebase embeds `versions.json`, `models.json`, and `config.json` at compile time. The new design replaces this with a `ConfigManager` that:

All JSON config files use a **versioned envelope format**:
```json
{
  "config_version": 1,
  "min_builder_version": "0.1.0",
  "data": [ ... ]
}
```
`config_version` is a monotonically increasing integer. `min_builder_version` declares the minimum builder version required to interpret this config. `data` contains the actual array of versions, models, or config schema items. The ConfigManager also accepts bare arrays for backward compatibility with user-edited configs.

1. **On first run**: uses bundled fallback configs (compiled in as defaults) and writes them to a local config directory.
2. **On startup** (if update checking is enabled): fetches configs from a remote URL, compares version fields, and replaces local copies if newer.
3. **On version mismatch**: if the fetched config declares a minimum builder version higher than the running binary, warns the user instead of applying it.
4. **Default URL is GitHub**: the remote URL points to the config files in the project's GitHub repository (raw content), so there is no extra infrastructure to maintain. A `--config-url` flag (or env var) overrides this for testing, custom registries, or air-gapped mirrors.

```rust
pub struct ConfigManager {
    config_dir: PathBuf,       // e.g. ~/.llamafile-builder/config/
    remote_url: String,        // default or overridden
    update_mode: UpdateMode,   // Always, Never, Ask
}

pub enum UpdateMode {
    Always,
    Never,
    Ask,
}

impl ConfigManager {
    pub fn new(config_dir: PathBuf) -> Self;
    pub fn with_remote_url(self, url: &str) -> Self;
    pub fn with_update_mode(self, mode: UpdateMode) -> Self;

    /// Load configs from disk, falling back to bundled defaults.
    pub fn load(&self) -> Result<BuilderConfig>;

    /// Check remote for newer configs. Returns what changed, if anything.
    pub async fn check_for_updates(&self) -> Result<UpdateStatus>;

    /// Apply a fetched update to the local config directory.
    pub fn apply_update(&self, update: &ConfigUpdate) -> Result<()>;
}

pub struct BuilderConfig {
    pub versions: Vec<VersionGroup>,
    pub models: Vec<Model>,
    pub config_schema: Vec<ConfigSchemaItem>,
    pub config_version: u32,
    pub min_builder_version: Option<String>,
}

pub enum UpdateStatus {
    UpToDate,
    ConfigUpdateAvailable(ConfigUpdate),
    BuilderUpdateRequired { min_version: String },
}
```

The interface layers decide how to handle `UpdateStatus::Ask` -- the CLI prompts on stderr, the web UI shows a dialog, JSON-RPC emits a notification.

### Component model

A llamafile is built from components. Every selection the user makes -- choosing a version, picking a model, adding GPU libraries -- ultimately resolves to a set of **components** that will be bundled. Each component has a known type, a source (registry URL, HuggingFace ID, or local path), and metadata for validation.

```rust
/// A resolved file that will be included in the final llamafile.
pub struct Component {
    pub role: ComponentRole,
    pub source: ComponentSource,
    pub zip_path: String,           // path inside the llamafile (e.g. "/zip/model.gguf")
    pub expected_size: Option<u64>,
    pub expected_sha256: Option<String>,
}

pub enum ComponentRole {
    Runtime,          // the llamafile APE binary (base of the output file)
    Zipalign,         // the zipalign tool (used during build, not bundled)
    Weights,          // primary model weights (.gguf)
    MmprojWeights,    // multimodal projector weights (.gguf)
    GpuLib {          // GPU acceleration library
        accel: GpuAccel,
        os: Os,
    },
    ArgsFile,         // .args configuration (generated or overridden)
    ArgsPrompt,       // .args.prompt for agent mode
}

pub enum ComponentSource {
    /// Download from HuggingFace using the hf-hub shared cache.
    HuggingFace { repo_id: String, filename: String },
    /// Download from a direct URL (runtimes, GPU libs).
    Url(String),
    /// Use a file already on disk.
    Local(PathBuf),
    /// Generated at build time (e.g. .args from schema).
    Generated,
}

pub enum GpuAccel { Cuda, Rocm, Vulkan }
// Note: Metal is not included — it is compiled on-the-fly from sources
// embedded in the llamafile binary itself, not a downloadable library.
pub enum Os { Linux, Windows }
```

GPU libraries have two orthogonal dimensions: acceleration type and OS. For example, a build might include `GpuLib { accel: Cuda, os: Linux }` and `GpuLib { accel: Cuda, os: Windows }` to produce a llamafile that works with CUDA on both platforms. Any combination is valid, and libraries can be mixed between downloaded and local sources.

### Builder API (FR-1 through FR-7)

The `Builder` is the main entry point for constructing a llamafile. All selection methods modify internal state, which ultimately resolves to a set of `Component`s that `build()` will assemble.

```rust
pub struct Builder {
    config: BuilderConfig,
    state: BuildState,
    cancel_flag: Arc<AtomicBool>,
}

impl Builder {
    pub fn new(config: BuilderConfig, cache: Cache) -> Self;

    // --- State queries ---
    pub fn state(&self) -> &BuildState;
    pub fn resolved_components(&self) -> Vec<Component>;
    pub fn resolved_args(&self) -> Result<String>;  // preview the .args that would be generated
    pub fn validate(&self) -> Vec<ValidationMessage>;  // FR-6

    // --- Version / binaries (FR-1) ---
    pub fn select_version(&mut self, group: &str, version: &str) -> Result<()>;
    pub fn use_custom_runtime(&mut self, path: &Path) -> Result<()>;
    pub fn use_custom_zipalign(&mut self, path: &Path) -> Result<()>;

    // --- Model (FR-1, FR-2) ---
    pub fn select_registry_model(&mut self, model_id: &str) -> Result<()>;
    pub fn select_custom_model(&mut self, path: &Path) -> Result<()>;
    pub fn select_ollama_model(&mut self, name: &str, tag: &str, dir: Option<&Path>) -> Result<()>;
    pub fn select_lmstudio_model(&mut self, dir_name: &str, dir: Option<&Path>) -> Result<()>;
    pub fn select_quantization(&mut self, role: &str, index: usize) -> Result<()>;

    // --- Configuration (FR-4) ---
    pub fn set_config_value(&mut self, key: &str, value: serde_json::Value) -> Result<()>;
    pub fn set_args_override(&mut self, raw_args: &str);  // bypass schema, use raw .args content

    // --- GPU libraries (FR-1) ---
    pub fn add_gpu_lib(&mut self, accel: GpuAccel, os: Os) -> Result<()>;
    pub fn add_custom_gpu_lib(&mut self, path: &Path, accel: GpuAccel, os: Os) -> Result<()>;
    pub fn remove_gpu_lib(&mut self, accel: GpuAccel, os: Os);
    pub fn clear_gpu_libs(&mut self);

    // --- Agent (FR-7, placeholder) ---
    pub fn set_agent_config(&mut self, config: AgentConfig) -> Result<()>;

    // --- Output ---
    pub fn set_output_path(&mut self, path: &Path) -> Result<()>;

    // --- Execution ---
    // All progress is observable via tracing spans and events.
    pub async fn build(&self) -> Result<PathBuf>;  // FR-3, FR-5

    pub fn cancel(&self);
}
```

Design notes:

- **Component-centric**: every selection method modifies internal state that `resolved_components()` materializes into a flat list of components to assemble. This keeps the builder's internals declarative -- callers describe *what* they want, and the builder figures out the concrete files, URLs, and build steps.
- **`resolved_args()`** lets the UI or CLI show the `.args` content that *would* be generated, before the user commits to a build. Combined with `set_args_override()`, this supports the workflow: generate from schema, review, optionally edit, then build.
- **GPU libs are additive**: `add_gpu_lib()` adds one library at a time, allowing any combination of acceleration type and OS. `add_custom_gpu_lib()` adds a local file for a specific (accel, os) pair. This avoids a single `ComputeTarget` enum that would need to encode all possible combinations.
- **`set_agent_config()`** is a placeholder. Agent configuration is not yet defined, so the method accepts an `AgentConfig` struct that will be fleshed out later. For now it stores the data; `build()` generates `.args.prompt` from it if enabled.

### Model discovery (FR-2)

Stateless query functions that don't need a `Builder` instance:

```rust
// Registry queries (from loaded config)
pub fn search_models(config: &BuilderConfig, query: &str, tag: Option<&str>) -> Vec<&Model>;

// Local source scanning
pub fn scan_ollama_models(custom_dir: Option<&Path>) -> Result<Vec<OllamaModel>>;
pub fn scan_lmstudio_models(custom_dir: Option<&Path>) -> Result<Vec<LMStudioModel>>;

// System capabilities
pub fn system_info() -> SystemInfo;
```

### Downloads and caching (FR-3, NFR-7, NFR-8)

Downloads use two systems depending on the component source:

**HuggingFace models** (`ComponentSource::HuggingFace`) use the `hf-hub` crate. This is the official Rust client for the HuggingFace Hub API. It:

- Downloads files by repo ID + filename (e.g. `api.model("Qwen/Qwen3-0.6B-GGUF").get("qwen3-0.6b-q8_0.gguf")`)
- Uses the **standard HuggingFace cache layout** at `~/.cache/huggingface/hub/` -- the same layout used by Python's `huggingface_hub`, `llama.cpp`, `ollama`, and other tools. Files downloaded by the builder are immediately available to these tools and vice versa, avoiding duplicate multi-GB downloads on disk.
- Handles auth via `HF_TOKEN` env var and `~/.cache/huggingface/token`
- Supports async downloads (tokio feature)

**HF download strategy**: the `Cache::ensure()` method for HuggingFace sources follows this sequence:

1. **Local cache check first**: `hf_hub::Cache::from_env().model(repo_id).get(filename)` performs a pure filesystem check against hf-hub's cache. No network access. If the file is found, return immediately.
2. **Download on cache miss**: `download_with_progress()` downloads via hf-hub with our custom `TracingProgress` reporter (shared `Arc<AtomicU64>` counter for parallel chunks, `compare_exchange` for deduplicating milestone logs).
3. **Lock file handling**: if hf-hub fails with "Lock acquisition failed" (stale `.lock` file from an interrupted download or a concurrent process), the error message is surfaced to the user with the lock file path so they can decide whether to remove it. We do not auto-remove lock files — they may belong to a concurrent download.
4. **No fallback downloader**: HuggingFace sources are never downloaded with our own HTTP downloader. Interoperability with llama.cpp and other tools that share the HF cache is the priority (NFR-8).

Note: hf-hub's `download_with_progress()` does not check its own cache — it always downloads. This is why step 1 (our manual cache check) is essential to avoid re-downloading files on every build.

**Non-HF files** (`ComponentSource::Url`) -- runtimes, GPU libraries, zipalign -- use our own `Downloader` backed by `reqwest`, with resume support (HTTP Range), SHA256 verification, and progress reporting via `tracing`.

**Zipalign as a managed dependency**: zipalign is not assumed to be pre-installed. It comes from the same release as the llamafile runtime and is downloaded and cached alongside it. The `versions.json` config includes zipalign download URLs per release. Users may override with `use_custom_zipalign()` if they have a local copy.

**Local files** (`ComponentSource::Local`) need no downloading. The builder verifies they exist and optionally checks SHA256 if provided.

```rust
pub struct Cache {
    base_dir: PathBuf,          // e.g. ~/.llamafile-builder/
    hf_api: hf_hub::api::tokio::Api,  // shared HF client
}

impl Cache {
    pub fn new(base_dir: PathBuf) -> Self;
    pub fn with_hf_token(self, token: &str) -> Self;

    pub fn runtime_dir(&self) -> PathBuf;
    pub fn gpu_lib_dir(&self, version: &str) -> PathBuf;

    /// Ensure a component is available locally. Downloads if needed.
    /// For HF sources, delegates to hf-hub (shared cache).
    /// For URL sources, downloads to our cache dir with resume support.
    /// For Local sources, verifies the file exists.
    /// Progress is reported via tracing events.
    pub async fn ensure(
        &self,
        component: &Component,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<PathBuf>;

    /// Check status without downloading.
    pub fn check(&self, component: &Component) -> CacheStatus;
}

pub enum CacheStatus {
    Valid(PathBuf),
    Partial { path: PathBuf, bytes_downloaded: u64 },
    Missing,
}
```

`Builder::build()` calls `Cache::ensure()` for each resolved component before assembly.

### Observability

**Tracing is the sole observability mechanism.** The core library has no UI-facing progress types, no callback channels, no event emitters. All operations are instrumented with `tracing` events. Each interface layer subscribes to these events and presents them however it chooses:

- **CLI**: standard `tracing_subscriber` writing to stderr at INFO level.
- **TUI**: custom `tracing::Layer` forwarding events to an `mpsc` channel, drained by the render loop.
- **HTTP server** (planned): tracing events bridged to SSE stream.

Key events emitted during a build (all at INFO level unless noted):
- `build started` with `output` field
- `preparing zipalign` / `preparing runtime` / `preparing model files`
- `already cached` (cache hit) or `download started` / `downloading` (with `bytes_downloaded`, `bytes_total`, `percentage`, `filename` fields)
- `download complete` or `already in HF cache`
- `writing .args configuration` (DEBUG)
- `bundling` / `copying runtime` / `running zipalign` (DEBUG)
- `build complete` with `path` and `size_human` fields (DEBUG)

Download progress events are emitted at DEBUG level (~2/sec) with INFO at 10% milestones. This keeps INFO-level output clean while giving frontends fine-grained progress when they subscribe to DEBUG.

Important constraint: async functions in the core library must not use `#[tracing::instrument]` or hold `EnteredSpan` across `.await` points, as this makes futures `!Send` and prevents `tokio::spawn`.

### Validation (FR-6)

```rust
pub enum ValidationMessage {
    Warning(String),  // e.g. "Context size exceeds model training length"
    Error(String),    // e.g. "No model selected"
}
```

`Builder::validate()` checks:
- Required components are selected (model, version, output path)
- Context size vs. model training length
- Compute target compatibility with host platform (e.g. CUDA on macOS)
- Agent shell tool warning
- Custom binary existence and permissions


## Interface layers

Each interface is a thin translation layer between user interaction and the core library. None contains builder logic.

### CLI mode (`llamafile-builder [subcommand]`)

The default when no mode flag is given. Uses `clap` for argument parsing.

```
llamafile-builder versions                  # list available versions
llamafile-builder models [--search X] [--tag Y]  # search model registry
llamafile-builder models show <id>          # model details
llamafile-builder ollama list [--dir PATH]  # scan Ollama cache
llamafile-builder lmstudio list [--dir PATH]  # scan LM Studio cache
llamafile-builder system-info               # host capabilities

llamafile-builder build \
    --model <id|path> \
    [--quantization Q4_K_M] \
    [--output path.llamafile] \
    [--gpu cuda|rocm|metal|none] \
    [--context-length 8192] \
    [--args-file custom.args] \
    [--config-url https://...] \
    ...
```

**Exit codes** (NFR-3):

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Download failed |
| 4 | Build failed |
| 5 | Cancelled |

Progress is reported to stderr via `tracing` at INFO level. Structured output (JSON) available with `--json` flag for machine consumption.

**Progressive complexity** (NFR-5): `llamafile-builder build --model llama3-8b-instruct` is sufficient for a basic build. The CLI fills in defaults (latest stable version, default config, CPU-only, auto-generated output path).

**Three modes of operation:**

1. **One-shot mode**: all options specified on the command line, build runs immediately. Good for scripts and CI.
2. **Multi-step mode**: `config set-*` commands persist state between invocations. `build` (no `--model`) uses saved state. Defaults applied for anything not configured.
3. **JSON-RPC mode** (`--interactive`): stateful session over stdin/stdout. Same methods as the `config` subcommands but as a protocol. See [JSON-RPC.md](../specs/JSON-RPC.md). This is the universal interface that the HTTP server and future GUIs will also use.

### Server mode (`llamafile-builder --serve`)

Starts an HTTP server on localhost using `axum`. Serves two things:

1. **REST API + SSE** for programmatic access
2. **Web UI static files** from a directory on disk (NFR-11)

```
# Static data
GET  /api/versions
GET  /api/models[?search=X&tag=Y]
GET  /api/models/:id
GET  /api/config-schema
GET  /api/system-info
GET  /api/ollama/models[?dir=PATH]
GET  /api/lmstudio/models[?dir=PATH]

# Build state
GET  /api/state
POST /api/state/version          { group, version }
POST /api/state/model            { id }
POST /api/state/model-file       { path }
POST /api/state/config           { key, value }
POST /api/state/args-override    { content }
POST /api/state/compute          { target, custom_libs? }
POST /api/state/agent            { enabled, system_prompt, tools }
POST /api/state/output           { path }
POST /api/state/reset
GET  /api/state/validate

# Build execution
POST /api/build                  -> SSE stream
POST /api/cancel

# Cache status
GET  /api/cache/status           -> what's already downloaded
```

**Web UI location**: The server looks for static files in a configurable directory, defaulting to `$INSTALL_PREFIX/share/llamafile-builder/web/` or a `web/` directory next to the binary. The web UI is a set of HTML/CSS/JS files (ported from the current Tauri frontend) that uses `fetch()` for API calls and `EventSource` for build progress.

The server binds to `127.0.0.1` only by default and opens the user's browser automatically (suppressible with `--no-open`).

### Interactive mode (`llamafile-builder --interactive`)

JSON-RPC 2.0 over stdin/stdout. For scripting scenarios where multiple operations need shared state within a session.

```jsonc
// Request
{"jsonrpc":"2.0","id":1,"method":"list_models","params":{}}

// Response
{"jsonrpc":"2.0","id":1,"result":[...]}

// Progress notification (no id, pushed during build)
{"jsonrpc":"2.0","method":"progress","params":{"stage":"download_model","bytes":1234,"total":5678}}

// Error
{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"No model selected"}}
```

Methods mirror the core library API one-to-one. This mode is useful for integration with editors, external GUIs, or complex automation that the CLI's one-shot model doesn't suit.

Note: the current implementation is missing `cancel`, `list_ollama_models`, and `list_lmstudio_models` methods. These are planned for a future phase.

### TUI (`llamafile-tui`)

A separate binary using `ratatui` + `crossterm`. Links directly to `llamafile-core` (no IPC). The TUI provides an interactive wizard in the terminal without requiring a browser or display server.

The TUI is fully functional: version/model/quantization selection via popup menus, schema-driven configuration editing, multi-select GPU library picker, `.args` editor with `tui-textarea`, real-time log pane with level filtering, and non-blocking builds via `tokio::spawn`.

See [TUI.md](TUI.md) for full reference.


## Project structure

```
llamafile-builder/
+-- Cargo.toml                        # Workspace root
+-- crates/
|   +-- llamafile-core/               # Library, zero UI deps
|   |   +-- Cargo.toml
|   |   +-- src/
|   |   |   +-- lib.rs                # Public API re-exports
|   |   |   +-- builder.rs            # Builder struct, build orchestration
|   |   |   +-- cache.rs              # Cache management, download coordination
|   |   |   +-- config_manager.rs     # Remote config fetch, versioning, update checks
|   |   |   +-- download.rs           # HTTP downloads, resume, SHA256, HF auth
|   |   |   +-- state.rs              # BuildState, component selections
|   |   |   +-- models.rs             # Data types (Model, Version, ConfigSchema, etc.)
|   |   |   +-- args.rs               # Schema-driven .args generation
|   |   |   +-- agent.rs              # AgentConfig, system prompt, tool definitions
|   |   |   +-- validation.rs         # Build validation rules
|   |   |   +-- ollama.rs             # Ollama cache scanning
|   |   |   +-- lmstudio.rs           # LM Studio cache scanning
|   |   |   +-- system_info.rs        # Host platform detection
|   |   |   +-- error.rs              # Error types (thiserror)
|   |   +-- data/                     # Bundled fallback configs
|   |       +-- versions.json
|   |       +-- models.json
|   |       +-- config.json
|   |
|   +-- llamafile-cli/                # Binary: CLI + server + interactive
|   |   +-- Cargo.toml
|   |   +-- src/
|   |       +-- main.rs               # Entry point, mode dispatch
|   |       +-- cli.rs                # Clap definitions
|   |       +-- commands/             # CLI subcommand handlers
|   |       |   +-- mod.rs
|   |       |   +-- versions.rs
|   |       |   +-- models.rs
|   |       |   +-- build.rs
|   |       |   +-- scan.rs
|   |       +-- server.rs             # Axum HTTP server
|   |       +-- sse.rs                # SSE bridge (BuildProgress -> SSE events)
|   |       +-- interactive.rs        # JSON-RPC over stdio
|   |       +-- progress.rs           # Terminal progress display (indicatif)
|   |
|   +-- llamafile-tui/                # Terminal UI wizard
|       +-- Cargo.toml
|       +-- src/
|           +-- main.rs              # Entry point, tokio runtime, build task spawning
|           +-- app.rs               # App state, Focus/Popup enums, Builder bridge
|           +-- ui.rs                # Dashboard and popup rendering
|           +-- events.rs            # Key handling for all popups
|           +-- config_ui.rs         # Schema flattening for config popup
|           +-- log_layer.rs         # Custom tracing Layer -> mpsc channel
|           +-- app.rs
|           +-- ui.rs
|           +-- events.rs
|
+-- web/                              # Web UI static assets (served from disk)
|   +-- index.html
|   +-- app.js
|   +-- styles.css
|
+-- data/                             # Canonical config files (source of truth for releases)
    +-- versions.json
    +-- models.json
    +-- config.json
```

Note: `data/` at workspace root is the single source of truth for config files. The core library references them via `include_str!("../../../data/...")` for bundled fallbacks.


## Layered testability (NFR-12)

Each interface layer serves as an integration test for the layer below it. This maximizes automated test coverage before moving to manual UI testing.

```
Unit tests (103)         -> core library functions + CLI state management
CLI (one-shot + config)  -> integration test for Builder API, downloads, assembly
JSON-RPC (piped stdin)   -> integration test for protocol and stateful sessions
HTTP server (curl)       -> integration test for REST API before adding a browser UI
Web UI (browser)         -> the only layer requiring manual testing
```

Implemented layers: unit tests, CLI (one-shot + multi-step), JSON-RPC. Remaining: HTTP server, Web UI.

The order of implementation follows this layering: each phase adds a new test surface on top of the previous one.


## What stays in interface layers

These concerns are NOT part of the core library. Each interface implements them using its own mechanisms:

| Concern | CLI | Server/Web UI | TUI |
|---------|-----|---------------|-----|
| File/folder picker | N/A (paths as args) | Browser `<input type="file">` or path text input | N/A (paths as args) |
| Browser launch | `open` crate | Auto-open on `--serve` | N/A |
| Progress display | `tracing` subscriber on stderr | SSE events rendered by JS | `ratatui` widgets |
| Update prompt (NFR-10) | stderr log | Browser dialog | TUI dialog |
| Cancellation | Ctrl+C / signal handler | POST /api/cancel | Ctrl+C / key binding |


## Implementation plan

See [PLAN.md](PLAN.md) for the phased implementation plan with current status.

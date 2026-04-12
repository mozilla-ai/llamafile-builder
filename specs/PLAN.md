# Implementation Plan: New llamafile-builder Architecture

## Context

The llamafile-builder is being rewritten from a monolithic Tauri desktop app into a Rust workspace with a core library (zero UI dependencies) and thin interface layers (CLI, HTTP server, JSON-RPC, web UI). This solves the fundamental problem that the builder's logic (downloading, configuring, assembling) is trapped inside renderer-dependent binaries that break on various platforms (WebKit in Snap, WGPU on Intel Macs).

The specs, architecture, and code review are at:
- `specs/README.md` -- requirements (FR-1..7, NFR-1..12)
- `specs/ARCHITECTURE.md` -- architecture decisions
- `specs/CODE_REVIEW.md` -- which existing code to reuse

**Primary source code**: dioxus branch at `~/workspace/llamafile-builder-worktrees/add-dioxus/`
**Target directory**: `~/workspace/llamafile-builder-worktrees/new_architecture/`

---

## Phase 0: Workspace Scaffolding

**Goal**: Cargo workspace compiles with stub crates.

### Create:
- `Cargo.toml` (workspace root, replaces old Tauri project)
  - Members: `crates/llamafile-core`, `crates/llamafile-cli`
  - Workspace dependencies: `tokio`, `serde`, `serde_json`, `reqwest` (rustls-tls), `hf-hub` (tokio), `sha2`, `hex`, `thiserror`, `tracing`, `tracing-subscriber`, `directories`, `dirs`, `futures-util`, `url`
- `crates/llamafile-core/Cargo.toml` -- library crate, workspace deps only
- `crates/llamafile-core/src/lib.rs` -- stub: `pub mod error;`
- `crates/llamafile-core/src/error.rs` -- stub error type
- `crates/llamafile-cli/Cargo.toml` -- binary crate, depends on `llamafile-core`
- `crates/llamafile-cli/src/main.rs` -- stub main
- `crates/llamafile-core/data/` -- copy `config.json`, `models.json`, `versions.json` from `add-dioxus/src-tauri/`
- `data/` at workspace root -- same files (canonical source for releases)

### Modify:
- `.gitignore` -- add `/target/`, keep `Cargo.lock` tracked

### Note:
- Old `src-tauri/` and `src/` stay untouched until Phase 7

### Verify:
- `cargo check --workspace` compiles
- `cargo run -p llamafile-cli` prints stub message

---

## Phase 1: Data Types and State Management

**Goal**: Core library exports all data structures. Build state tests pass (~60 tests).

### Files to create in `crates/llamafile-core/src/`:

**`error.rs`** (write from scratch)
- Unified `Error` enum via `thiserror`: `Io`, `Http`, `Json`, `Download(DownloadError)`, `Build`, `Config`, `Validation`, `NotFound`, `Cancelled`
- `DownloadError` enum: `Http`, `Io`, `ChecksumMismatch`, `SizeMismatch`, `Cancelled`, `AuthRequired`, `ResumeNotSupported`
- `pub type Result<T> = std::result::Result<T, Error>;`

**`models.rs`** (copy + adapt from `add-dioxus/crates/models/src/lib.rs`, 544 lines)
- Keep all existing types as-is: `GpuLibrary`, `GpuLibsByPlatform`, `GpuLibs`, `LlamafileRelease`, `LlamafileVersionGroup`, `ModelFileQuantization`, `ModelFile`, `Model`, `ConfigSchemaItem`, `ComputeMode`, `ComputeConfig`, `AgentTool`, `AgentConfig`, `BuildStage`, `BuildProgress`, `SystemInfo`, `OllamaModel`, `LMStudioFile`, `LMStudioModel`, `FileExistsVariant`, `FileExistsChoice`
- Keep helpers: `format_size`, `get_filename_from_path`, `get_filename_from_url`, `get_default_output_dir`
- Remove: `get_llamafile_cache_dir`, `get_model_cache_dir`, `get_gpu_lib_cache_dir` (move to Cache in Phase 3)
- Add new Component types: `Component`, `ComponentRole`, `ComponentSource`, `GpuAccel`, `Os`
  - `GpuAccel`: `Cuda`, `Rocm`, `Vulkan` (no Metal -- Metal is compiled on-the-fly, not a downloadable lib)
  - `Os`: `Linux`, `Windows`

**`state.rs`** (copy + adapt from `add-dioxus/crates/models/src/build_state.rs`, ~920 lines with ~450 lines of tests)
- Fix imports: `use crate::models::*`
- Fix test `include_str!` paths: `include_str!("../data/models.json")` etc. (relative to `crates/llamafile-core/src/`)
- Keep all 60+ tests verbatim (they load real shipped JSON data)
- Leave `resolved_llamafile_path` logic as-is for now; Phase 4 will rework it with Cache

**Update `lib.rs`**: export `error`, `models`, `state`

### Verify:
- `cargo test -p llamafile-core` -- 60+ state tests pass
- `cargo check --workspace`

---

## Phase 2: Args, File Ops, Scanners, System Info

**Goal**: Add pure logic modules (no network). ~50 more tests.

### Files to create in `crates/llamafile-core/src/`:

**`args.rs`** (copy from `add-dioxus/crates/services/src/builder.rs`, lines 0-229 + tests 356-647)
- Keep: `generate_args_from_schema`, `generate_system_prompt`, `estimate_size`, `ArgsContext`, `process_schema_items`
- Add: `pub fn default_config_values(schema) -> HashMap` (extract `collect_defaults` from tests, make public)
- Fix imports, fix test `include_str!` paths
- ~30 tests

**`file_ops.rs`** (copy from `add-dioxus/crates/services/src/file_ops.rs`, 86 lines)
- Verbatim copy, no changes needed

**`zipalign.rs`** (extract from `add-dioxus/crates/services/src/builder.rs`, lines 231-354)
- Keep: `zipalign_bundle` (with ENOEXEC fallback), `make_executable`
- Rewrite `find_zipalign`: check adjacent to binary -> PATH -> configurable custom path
- Remove the `CARGO_MANIFEST_DIR` / `src-tauri/bin` dev-build fallback

**`ollama.rs`** (copy from `add-dioxus/crates/services/src/ollama.rs`, 301 lines)
- Fix import: `use crate::models::OllamaModel`
- 8 tests, copy as-is

**`lmstudio.rs`** (copy from `add-dioxus/crates/services/src/lmstudio.rs`, 276 lines)
- Fix import: `use crate::models::LMStudioModel`
- 7 tests, copy as-is

**`system_info.rs`** (write from scratch, ~40 lines)
- Real detection using `sysinfo` crate (add dep) for memory
- GPU heuristics: NVIDIA (`nvidia-smi` in PATH), AMD (`/opt/rocm` or `rocminfo`), Metal (`cfg!(target_os = "macos")`)

**Update `lib.rs`**: export all new modules

### Verify:
- `cargo test -p llamafile-core` -- ~110 tests pass
- No network deps touched yet

---

## Phase 3: Downloads and Cache

**Goal**: Core library can download files from URLs and HuggingFace.

### Files:

**`download.rs`** (copy + adapt from `add-dioxus/crates/services/src/download.rs`, 544 lines)
- Move `DownloadError` to `error.rs` (already defined in Phase 1)
- Move `DownloadProgress`, `FileInfo` to `models.rs`
- Keep `is_huggingface_url`, `resolve_hf_token` as `pub(crate)`
- Refactor `download_file` callback to use `mpsc::Sender<DownloadProgress>` instead of generic closure
- 17 tests

**`cache.rs`** (write from scratch, ~200 lines)
- `Cache` struct: `base_dir: PathBuf`, `hf_api: hf_hub::api::tokio::Api`
- Methods: `new()`, `with_hf_token()`, `runtime_dir()`, `gpu_lib_dir()`, `config_dir()`
- `ensure(component, progress_tx, cancel_flag) -> Result<PathBuf>` -- the key method:
  - `ComponentSource::HuggingFace`: use `hf_hub::api::tokio::ApiRepo::url()` to resolve the download URL with auth, then download with our custom downloader to the hf-hub cache path. This gives us full progress while maintaining HF cache compatibility with llama.cpp.
  - `ComponentSource::Url`: download with our custom downloader to `base_dir/bin/` or `base_dir/v/<version>/`
  - `ComponentSource::Local`: verify file exists, optionally check SHA256
  - `ComponentSource::Generated`: no-op
- `check(component) -> CacheStatus` (Valid/Partial/Missing)

### Add deps:
- `hf-hub = { workspace = true }` to llamafile-core
- Dev deps for download tests: `tempfile`, `hyper`, `hyper-util`, `http-body-util`, `bytes`

### Verify:
- All download tests pass
- New cache unit tests (local files, cache status)

---

## Phase 4: ConfigManager, Validation, and Builder API

**Goal**: The public `Builder` API works. This is the most critical phase.

### Files:

**`config_manager.rs`** (write from scratch, ~150 lines)
- `ConfigManager` struct: `config_dir`, `remote_url` (default: GitHub raw), `update_mode` (Always/Never/Ask)
- `BuilderConfig` struct: `versions`, `models`, `config_schema`, `config_version`, `min_builder_version`
- `load()`: read from `config_dir`, fallback to bundled (`include_str!("../data/...")`)
- `check_for_updates()`: fetch from remote URL, compare versions -> `UpdateStatus`
- `apply_update()`: write new JSON to `config_dir`
- Handle both bare arrays (existing format) and future versioned wrappers

**`validation.rs`** (write from scratch, ~80 lines)
- `ValidationMessage` enum: `Warning(String)`, `Error(String)`
- `validate(state, config) -> Vec<ValidationMessage>`: required components selected, context size vs training length, GPU compat, agent shell warning, file existence

**`agent.rs`** (write from scratch, ~20 lines)
- Placeholder module. Re-exports `AgentConfig` from models. Stub for future agent-specific logic.

**`builder.rs`** (write from scratch, ~400 lines -- most important new file)
- `Builder` struct: `config`, `state`, `cache`, `cancel_flag`, custom overrides, gpu_libs, args_override, agent_config, output_path
- Selection methods (all modify internal state): `select_version`, `use_custom_runtime`, `use_custom_zipalign`, `select_registry_model`, `select_custom_model`, `select_ollama_model`, `select_lmstudio_model`, `select_quantization`, `set_config_value`, `set_args_override`, `add_gpu_lib`, `add_custom_gpu_lib`, `remove_gpu_lib`, `clear_gpu_libs`, `set_agent_config`, `set_output_path`
- Query methods: `state()`, `resolved_components()`, `resolved_args()`, `validate()`
- Execution: `build(event_tx, decision_rx, policy)`, `cancel()`
- Two-channel design for interactive builds:
  - `event_tx: Sender<BuildEvent>` -- progress, logs, decisions needed, complete/failed
  - `decision_rx: Receiver<Decision>` -- file-exists choices, HF token
  - `FileExistsPolicy`: `SkipExisting` (CLI default), `RedownloadAll`, `Interactive` (server/TUI)
- Build pipeline (adapted from `add-dioxus/crates/services/src/build_runner.rs`):
  1. Ensure runtime (Cache::ensure for Runtime component)
  2. Ensure model files (Cache::ensure for Weights/MmprojWeights components)
  3. Write .args config (from resolved_args or args_override)
  4. Ensure GPU libs (Cache::ensure for each GpuLib component, non-fatal on failure)
  5. Create executable (copy runtime, make executable, zipalign_bundle with all files)

**Update `lib.rs`**: export `builder`, `config_manager`, `validation`, `agent`; re-export `Builder`, `ConfigManager`, `BuilderConfig`

### Verify:
- Unit tests for `ConfigManager::load()` with temp directories
- Unit tests for `validate()` with various states
- Unit tests for `Builder::resolved_components()` with different model/version/GPU combinations
- `cargo test -p llamafile-core` -- all tests pass (~130+)

---

## Phase 5: CLI Interface

**Goal**: `llamafile-builder build --model <id>` works end-to-end.

### Add deps to `llamafile-cli/Cargo.toml`:
- `clap = { version = "4", features = ["derive"] }`
- `indicatif = "0.17"` (progress bars)
- `console = "0.15"` (terminal colors)
- Dev: `assert_cmd = "2"`, `predicates = "3"`

### Files in `crates/llamafile-cli/src/`:

**`cli.rs`** (write from scratch)
- Clap derive structs: `Cli`, `Commands` (Versions, Models, Ollama, LmStudio, SystemInfo, Build), `BuildArgs`
- Mode flags: `--serve`, `--interactive`, `--config-url`, `--update-mode`, `--json`
- Build args: `--model`, `--quantization`, `--output`, `--gpu` (repeatable), `--args-file`, `--context-length`, `--version`, `--skip-existing`

**`commands/mod.rs`**, **`commands/versions.rs`**, **`commands/models.rs`**, **`commands/build.rs`**, **`commands/scan.rs`** (write from scratch)
- Each is a thin wrapper around core library calls
- `build.rs`: create Builder, apply CLI args, call build(), drain event channel with progress display

**`progress.rs`** (write from scratch)
- Bridge `BuildEvent` channel -> `indicatif` progress bars on stderr
- Handle `NeedDecision` events (shouldn't happen with SkipExisting policy)

**`main.rs`** (write from scratch)
- Dispatch: `--serve` -> server, `--interactive` -> interactive, subcommand -> commands
- Exit codes: 0=success, 1=general, 2=args, 3=download, 4=build, 5=cancelled

### Verify:
- `llamafile-builder versions` prints list
- `llamafile-builder models --search llama` searches registry
- `llamafile-builder system-info` shows real system info
- `llamafile-builder build --model <id> --output /tmp/test.llamafile` performs full build
- Integration tests with `assert_cmd`

---

## Phase 6: HTTP Server, SSE, and Interactive Mode

**Goal**: `--serve` mode exposes REST API + SSE. `--interactive` mode does JSON-RPC over stdio.

### Add deps to `llamafile-cli/Cargo.toml`:
- `axum = "0.8"`
- `tower-http = { version = "0.6", features = ["fs", "cors"] }`
- `async-stream = "0.3"`

### Files in `crates/llamafile-cli/src/`:

**`server.rs`** (write from scratch)
- Axum router with all `/api/...` endpoints from ARCHITECTURE.md
- Shared state: `Arc<Mutex<Builder>>`, `Arc<BuilderConfig>`
- Static file serving from configurable web dir
- Bind to `127.0.0.1`, auto-open browser (suppressible with `--no-open`)

**`sse.rs`** (write from scratch)
- Bridge `BuildEvent` channel -> Axum SSE stream
- Interactive decisions via separate response endpoint

**`interactive.rs`** (write from scratch)
- JSON-RPC 2.0 over stdin/stdout
- Methods mirror core library API
- Progress notifications pushed during builds

### Verify:
- `curl localhost:8765/api/versions` returns JSON
- `curl -N -X POST localhost:8765/api/build` returns SSE stream
- `echo '{"jsonrpc":"2.0","id":1,"method":"list_versions"}' | llamafile-builder --interactive`

---

## Phase 7: Web UI Port and Cleanup

**Goal**: Browser-based wizard works. Old code removed.

### Web UI:
- Copy `src/` (current Tauri frontend) to `web/`
- Systematic replacement in JS:
  - `invoke('command_name', {args})` -> `fetch('/api/endpoint', {method, body})`
  - `listen('event-name', callback)` -> `new EventSource('/api/build')`
- Test full wizard flow in browser

### Cleanup:
- Remove `src-tauri/` directory
- Remove `src/` directory (old frontend location)
- Update `CLAUDE.md` for new architecture
- Update `README.md`

### Verify:
- `llamafile-builder --serve` opens browser, wizard completes a build
- All `cargo test --workspace` passes

---

## Key Source File Mappings

| New location | Source | Action |
|-------------|--------|--------|
| `core/src/models.rs` | `add-dioxus/crates/models/src/lib.rs` | Copy + add Component types |
| `core/src/state.rs` | `add-dioxus/crates/models/src/build_state.rs` | Copy + fix imports |
| `core/src/args.rs` | `add-dioxus/crates/services/src/builder.rs` (lines 0-229) | Extract args generation |
| `core/src/zipalign.rs` | `add-dioxus/crates/services/src/builder.rs` (lines 231-354) | Extract zipalign + ENOEXEC |
| `core/src/download.rs` | `add-dioxus/crates/services/src/download.rs` | Copy + channel-based progress |
| `core/src/ollama.rs` | `add-dioxus/crates/services/src/ollama.rs` | Copy verbatim |
| `core/src/lmstudio.rs` | `add-dioxus/crates/services/src/lmstudio.rs` | Copy verbatim |
| `core/src/file_ops.rs` | `add-dioxus/crates/services/src/file_ops.rs` | Copy verbatim |
| `core/src/error.rs` | -- | Write from scratch |
| `core/src/builder.rs` | Adapted from `add-dioxus/crates/services/src/build_runner.rs` logic | Write from scratch using build_runner as reference |
| `core/src/cache.rs` | -- | Write from scratch (hf-hub + custom downloader) |
| `core/src/config_manager.rs` | -- | Write from scratch |
| `core/src/validation.rs` | -- | Write from scratch |
| `core/src/system_info.rs` | -- | Write from scratch (real detection) |
| `core/src/agent.rs` | -- | Placeholder |
| CLI, server, interactive | -- | All written from scratch |

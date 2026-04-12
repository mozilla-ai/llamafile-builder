# Code Review: Existing Implementations

Comparison of backend code across branches to determine what to reuse, adapt, or rewrite for the new architecture.

**Branches compared:**
- **tauri** (`main`, `new_architecture`, `rewrite`) -- essentially the same monolithic codebase (6 files in `src-tauri/src/`)
- **dioxus** (`add-dioxus`) -- refactored into a workspace with 4 crates (`models`, `services`, `state`, `ui`)

The dioxus branch is the evolved version of the tauri code. Since main/new_architecture/rewrite are nearly identical, this is effectively a two-way comparison.


## Component-by-component verdict

### Data types (`models.rs`)

| | Tauri | Dioxus |
|---|---|---|
| Location | `src-tauri/src/models.rs` (378 lines) | `crates/models/src/lib.rs` (544 lines) |
| Structures | All core types (Model, Version, ConfigSchema, Build, Agent, Ollama, LMStudio) | Same types plus `FileExistsVariant`/`FileExistsChoice` enums, helper functions (`format_size`, `get_filename_from_path`, `get_filename_from_url`, cache dir helpers) |
| Tests | None | None in lib.rs (tests are in build_state.rs) |

**Verdict: Use dioxus.** Already a separate crate. Helper functions extracted from commands.rs. `FileExistsVariant`/`FileExistsChoice` are useful for the build runner's file-exists prompting logic.

**Adaptation needed:** Remove `get_llamafile_cache_dir` / `get_model_cache_dir` helpers (these move to the `Cache` struct). Add `Component`, `ComponentRole`, `ComponentSource`, `GpuAccel`, `Os` types from our architecture.


### Build state (`build_state.rs`)

| | Tauri | Dioxus |
|---|---|---|
| Location | `src-tauri/src/build_state.rs` (632 lines) | `crates/models/src/build_state.rs` (920 lines, ~450 lines are tests) |
| API style | `BuildStateManager` wraps `Mutex<BuildState>`, all mutations go through the manager | Methods directly on `BuildState`, no Mutex wrapper -- the consumer adds thread-safety |
| Tests | None | 60+ tests covering all model sources, file selection, output paths, edge cases |

**Verdict: Use dioxus.** Dramatically better test coverage. Putting methods directly on `BuildState` (not behind a Mutex wrapper) makes the code testable as a plain struct, which is exactly what a library crate needs. The Mutex/Arc wrapper belongs in the interface layer, not the core.

**Adaptation needed:** Refactor toward the component model (selections resolve to `Vec<Component>` instead of scattered resolved paths). Remove Tauri-specific path assumptions. The existing tests are a valuable safety net during this refactoring.


### Download (`download.rs`)

| | Tauri | Dioxus |
|---|---|---|
| Location | `src-tauri/src/download.rs` (339 lines) | `crates/services/src/download.rs` (544 lines) |
| Progress | Callback with no throttling | Callback throttled to ~2/sec (500ms minimum interval) to avoid UI spam |
| SHA256 | 64KB buffer, async | 256KB buffer, spawns blocking task (correct for heavy I/O) |
| Tests | 1 test (HF URL detection) | 17 tests (URL detection, token resolution, file info, checksums) |

**Verdict: Use dioxus.** Progress throttling and blocking-task SHA256 are real improvements. Much better test coverage.

**Adaptation needed:** HuggingFace model downloads will be replaced by `hf-hub` crate (per NFR-8). This module becomes the downloader for non-HF files only (runtimes, GPU libs, config updates). Strip out HF URL detection and token resolution. Keep: resume support, SHA256 verification, progress callback, cancellation.


### Build orchestration

| | Tauri | Dioxus |
|---|---|---|
| Args generation | `builder.rs` (544 lines), schema-driven, 2 tests | `crates/services/src/builder.rs` (647 lines), same logic plus `zipalign_bundle` with ENOEXEC fallback, `find_zipalign`, 30+ tests |
| Build pipeline | **Stubbed** -- `build_llamafile()` is pseudocode | **Complete** -- `crates/services/src/build_runner.rs` (811 lines), 5-phase pipeline with `BuildReporter` trait |

**Verdict: Use dioxus, strongly.** The build runner is the single most important piece of code in the project, and only the dioxus branch has a working implementation.

The `BuildReporter` trait is particularly good design:

```rust
pub trait BuildReporter {
    async fn log(&mut self, msg: &str);
    async fn set_progress(&mut self, stage: BuildStage, pct: f32, msg: &str);
    async fn set_download_progress(&mut self, dp: Option<DownloadProgress>);
    async fn prompt_file_exists(&mut self, variant: FileExistsVariant) -> FileExistsChoice;
    fn is_cancelled(&self) -> bool;
    async fn set_hf_token_needed(&mut self);
}
```

This cleanly separates orchestration logic from UI concerns -- exactly the pattern our architecture needs. The CLI implements `BuildReporter` with terminal output, the server implements it with SSE events, the TUI implements it with ratatui widgets.

The dioxus `builder.rs` also has the ENOEXEC fallback for zipalign (APE binaries on Linux need `sh` wrapper), which the tauri version lacks.

**Adaptation needed:**
- Replace `BuildReporter` trait methods with progress channels (`mpsc::Sender<BuildProgress>`) for non-interactive operations, keeping the trait for interactive prompting (file-exists decisions).
- Integrate `hf-hub` for model downloads.
- Add the `Cache::ensure()` abstraction around download logic.
- Rework to use the component model (`Vec<Component>` drives the pipeline).


### Ollama scanning

| | Tauri | Dioxus |
|---|---|---|
| Location | Embedded in `commands.rs` (~100 lines) | `crates/services/src/ollama.rs` (301 lines) |
| Tests | None | 8 tests with realistic temp dir setup |

**Verdict: Use dioxus.** Already a standalone module with proper tests. Same logic, better packaging.

**Adaptation needed:** Minimal. Move into core library as-is.


### LM Studio scanning

| | Tauri | Dioxus |
|---|---|---|
| Location | Embedded in `commands.rs` (~80 lines) | `crates/services/src/lmstudio.rs` (276 lines) |
| Tests | None | 7 tests |

**Verdict: Use dioxus.** Same reasoning as Ollama.

**Adaptation needed:** Minimal. The filename-based role inference (`infer_gguf_role`) is heuristic but reasonable for now.


### HuggingFace token management

| | Tauri | Dioxus |
|---|---|---|
| Location | Split between `download.rs` (resolve) and `commands.rs` (get/set/clear) | `crates/services/src/hf_token.rs` (111 lines, 5 tests) |

**Verdict: Dump both.** Per specs (FR-3), we're simplifying to environment variable only. The `hf-hub` crate handles HF_TOKEN and `~/.cache/huggingface/token` natively. We don't need our own token management code.


### Validation

| | Tauri | Dioxus |
|---|---|---|
| Location | `validation.rs` (75 lines) | `crates/services/src/validation.rs` (95 lines) |
| Difference | Panics only | Also has `validate_all() -> Result<(), String>` (non-panicking) |

**Verdict: Dump both.** The current validators check embedded JSON at compile time. With `ConfigManager` (NFR-9), config files are loaded from disk/remote at runtime. Validation will be part of `ConfigManager::load()` and needs to be rewritten to handle versioning, schema evolution, and graceful error reporting instead of panicking.


### System info

| | Tauri | Dioxus |
|---|---|---|
| Both | Hardcoded 16GB memory, GPU detection always false, Metal = cfg!(target_os = "macos") |

**Verdict: Dump both.** Both are stubs. Rewrite with actual detection (e.g. `sysinfo` crate for memory, probe for GPU capabilities).


### File operations

| | Tauri | Dioxus |
|---|---|---|
| Location | Scattered in `commands.rs` | `crates/services/src/file_ops.rs` (86 lines) |

**Verdict: Use dioxus** as a starting point, but these are trivial wrappers (`file_exists`, `copy_file`, `make_executable`, `calculate_sha256`). Could also just inline them where needed.


### Tauri commands layer (`commands.rs`)

**Verdict: Dump entirely.** This is the Tauri IPC glue (1226 lines). It gets replaced by our CLI, HTTP server, and JSON-RPC interfaces. The logic it contains (Ollama scanning, LM Studio scanning, file ops, download coordination) has already been extracted into proper modules in the dioxus branch.


### Dioxus UI crate and state crate

**Verdict: Dump both.** The UI crate is Dioxus-specific (RSX components, Blitz workarounds). The state crate uses Dioxus signals. Neither is reusable in the new architecture. However, `build_orchestrator.rs` (90 lines) is a good reference for how to implement `BuildReporter` for a specific UI -- the pattern will be replicated for CLI/server/TUI.


## Summary: what we take forward

### From dioxus branch (primary source)

| Module | Source | Lines | Tests | Adaptation |
|--------|--------|-------|-------|------------|
| Data types | `crates/models/src/lib.rs` | 544 | -- | Add Component types |
| Build state | `crates/models/src/build_state.rs` | 920 | 60+ | Refactor toward component model |
| Build runner | `crates/services/src/build_runner.rs` | 811 | -- | Channel-based progress, hf-hub, Cache integration |
| Args generation | `crates/services/src/builder.rs` | 647 | 30+ | Minor: remove dead code |
| Download | `crates/services/src/download.rs` | 544 | 17 | Strip HF logic (hf-hub replaces it) |
| Ollama | `crates/services/src/ollama.rs` | 301 | 8 | Minimal |
| LM Studio | `crates/services/src/lmstudio.rs` | 276 | 7 | Minimal |
| File ops | `crates/services/src/file_ops.rs` | 86 | -- | Minimal |

### Written from scratch

| Module | Reason |
|--------|--------|
| `ConfigManager` | New concept (NFR-9, NFR-10), no prior implementation |
| `Cache` | New concept (NFR-7), integrates hf-hub + custom downloader |
| `system_info` | Both versions are stubs; needs real implementation |
| `validation` | Rewritten for runtime config loading, not embedded JSON |
| `error.rs` | Unified error types for the library |
| `lib.rs` (public API) | New: `Builder` struct with component-centric API |
| CLI, server, interactive, TUI | New interface layers |
| Web UI | Port from Tauri frontend (invoke -> fetch, listen -> EventSource) |


### Dumped

| Code | Reason |
|------|--------|
| `commands.rs` (all branches) | Tauri IPC glue, replaced by new interfaces |
| `hf_token.rs` | Simplified to env var; hf-hub handles it natively |
| `validation.rs` (all branches) | Rewritten for ConfigManager |
| `system_info.rs` (all branches) | Stubs, need real implementation |
| `main.rs` (all branches) | Framework-specific entry points |
| Dioxus UI + state crates | Framework-specific, not reusable |
| Dioxus `build_orchestrator.rs` | Framework-specific adapter (but good reference for pattern) |

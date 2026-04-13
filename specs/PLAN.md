# Implementation Plan: New llamafile-builder Architecture

## Context

The llamafile-builder is being rewritten from a monolithic Tauri desktop app into a Rust workspace with a core library (zero UI dependencies) and thin interface layers (CLI, HTTP server, JSON-RPC, web UI). This solves the fundamental problem that the builder's logic (downloading, configuring, assembling) is trapped inside renderer-dependent binaries that break on various platforms (WebKit in Snap, WGPU on Intel Macs).

Related docs:
- `specs/README.md` -- requirements (FR-1..7, NFR-1..13)
- `specs/ARCHITECTURE.md` -- architecture decisions
- `specs/CODE_REVIEW.md` -- which existing code was reused

**Primary source code**: dioxus branch at `~/workspace/llamafile-builder-worktrees/add-dioxus/`
**Target directory**: `~/workspace/llamafile-builder-worktrees/new_architecture/`

---

## Phase 0: Workspace scaffolding -- DONE

Cargo workspace with `llamafile-core` (library) and `llamafile-cli` (binary) crates. JSON config files in `data/` at workspace root. Old `src-tauri/` untouched.

---

## Phase 1: Data types and state management -- DONE

`error.rs`, `models.rs` (all data types + Component model), `state.rs` (BuildState with 20 tests). Types adapted from dioxus branch `crates/models/`.

---

## Phase 2: Args, file ops, scanners, system info -- DONE

`args.rs` (schema-driven .args generation, 16 tests), `file_ops.rs`, `zipalign.rs` (with ENOEXEC fallback), `ollama.rs` (6 tests), `lmstudio.rs` (7 tests), `system_info.rs` (real memory/GPU detection, 2 tests). 53 tests total at this point.

---

## Phase 3: Downloads and cache -- DONE

`download.rs` (HTTP downloads with resume, SHA256, tracing, 17 tests), `cache.rs` (Cache with `check`/`ensure`, hf-hub integration for HF sources, 10 tests). 81 tests total.

---

## Phase 4: ConfigManager, validation, and Builder API -- DONE

`config_manager.rs` (load from disk with bundled fallbacks, remote update checking, 6 tests), `validation.rs` (required components, context length, GPU compat, zipalign availability, 5 tests), `agent.rs` (placeholder), `builder.rs` (full Builder API with `resolved_components()`, `resolved_args()`, `validate()`, `build()`, 13 tests). 105 tests total.

---

## Phase 5: CLI interface (one-shot mode) -- DONE

`cli.rs` (clap), `commands/` (versions, models, build, scan), `main.rs`. Tracing-based observability (no UI-facing progress types in core). Exit codes 0/1/2/3/4/5. Config update checking wired in for Always/Ask modes. `search_models()` in core library. All core types re-exported from `lib.rs`.

---

## Phase 5a: Config versioning -- DONE

**Goal**: JSON config files carry version metadata so the builder can detect newer configs and reject incompatible ones (NFR-9).

### Changes:
- **`data/versions.json`, `data/models.json`, `data/config.json`**: Wrap each bare array in an object with metadata:
  ```json
  {
    "config_version": 1,
    "min_builder_version": "0.1.0",
    "data": [ ... ]
  }
  ```
- **`models.rs`**: No change to the inner types. The wrapper is parsed by ConfigManager only.
- **`config_manager.rs`**: `load()` parses the wrapper, extracts `config_version` and `min_builder_version`, passes the inner `data` array to the existing deserializers. Must also handle bare arrays (for backward compat with user-edited configs that lack the wrapper).
- **`check_for_updates()`**: Compare fetched `config_version` against local. Only apply if newer. If `min_builder_version` exceeds running version, return `BuilderUpdateRequired`.
- **`Cargo.toml`**: The builder's own version is available via `env!("CARGO_PKG_VERSION")` at runtime.
- **JSON schemas**: Update to reflect the wrapper object.

### Verify:
- `ConfigManager::load()` reads both wrapped and bare-array formats.
- `check_for_updates()` returns `UpToDate` when remote version equals local.
- `check_for_updates()` returns `BuilderUpdateRequired` when `min_builder_version` is higher.
- Hardcoded `config_version: 1` is removed.

---

## Phase 5b: HuggingFace downloads via hf-hub -- DONE

**Goal**: Model downloads use the `hf-hub` crate and the shared HuggingFace cache at `~/.cache/huggingface/hub/`, achieving interoperability with llama.cpp and other tools (NFR-8).

### Changes:
- **`data/models.json`**: Add `huggingface_repo_id` field to each model (e.g. `"Qwen/Qwen3-0.6B-GGUF"`). The `download_url` field stays as a fallback for non-HF sources.
- **`models.rs`**: Add `huggingface_repo_id: Option<String>` to `ModelFileQuantization` or `Model`.
- **`builder.rs` / `download_registry_file()`**: When `huggingface_repo_id` is present, produce `ComponentSource::HuggingFace { repo_id, filename }` instead of `ComponentSource::Url`.
- **`cache.rs`**: The `HuggingFace` branch of `ensure()` already delegates to `hf-hub`. Verify it works end-to-end with progress via tracing.

### Verify:
- Build with a registry model. Model file appears in `~/.cache/huggingface/hub/models--Qwen--Qwen3-0.6B-GGUF/`.
- Second build reuses the cached file without re-downloading.
- `llama.cpp` or `huggingface-cli` can see the same cached file.

---

## Phase 5c: Zipalign as a managed dependency -- DONE

**Goal**: zipalign is downloaded and cached automatically, not assumed to be pre-installed (FR-5).

### Changes:
- **`data/versions.json`**: Add `zipalign_url`, `zipalign_size_bytes`, `zipalign_sha256` fields to each release.
- **`models.rs`**: Add zipalign fields to `LlamafileRelease`.
- **`builder.rs`**: In the build pipeline, add a phase before `create_executable` that ensures zipalign is available via `Cache::ensure()`, using the URL from the selected release. Store the resolved path.
- **`zipalign.rs`**: `find_zipalign()` becomes a fallback only; the builder passes the cached zipalign path explicitly.
- **`validation.rs`**: Remove the PATH-based zipalign check (no longer needed since builder downloads it).

### Verify:
- Build succeeds without zipalign in PATH.
- zipalign binary appears in `~/.llamafile-builder/cache/bin/`.
- `use_custom_zipalign()` still works for override.

---

## Phase 5d: Multi-step CLI -- DONE

**Goal**: The CLI supports incremental configuration across multiple invocations, persisting state between them (NFR-5).

### Changes:
- **State persistence**: Builder state is serialized to `~/.llamafile-builder/build-state.json` between CLI invocations.
- **New subcommands**:
  - `config set-version <group> <version>` -- select version
  - `config set-model <id>` -- select registry model by HF repo ID
  - `config set-model-file <path>` -- select custom local .gguf
  - `config set-quantization <role> <name>` -- select quantization for a role
  - `config set-output <path>` -- set output path
  - `config set <key> <value>` -- set a config value (context_length, temperature, etc.)
  - `config set-args-file <path>` -- override .args with custom file
  - `config add-gpu <accel>[:<os>]` -- add GPU library
  - `config remove-gpu <accel>[:<os>]` -- remove GPU library
  - `config set-runtime <path>` -- use custom llamafile binary
  - `config set-zipalign <path>` -- use custom zipalign binary
  - `config show` -- display current state and resolved components
  - `config show-args` -- preview the .args content that would be generated
  - `config reset` -- clear all state
  - `build` (no `--model` required) -- build from saved state
- The existing one-shot `build --model <id>` continues to work unchanged.

### Verify:
- Multi-step: set-version, set-model, show, show-args, build -- each persists and the next reads it
- config reset clears state
- One-shot build still works independently of saved state
- This is the integration test for the Builder's stateful API (NFR-12).

---

## Phase 5e: JSON-RPC interactive mode -- DONE

**Goal**: `--interactive` mode exposes the Builder API over JSON-RPC 2.0 on stdin/stdout (NFR-3). This is the universal protocol that all frontends can use to drive the builder.

### Changes:
- **`interactive.rs`** (new): JSON-RPC 2.0 server over stdin/stdout. Methods mirror the Builder API: `list_versions`, `list_models`, `search_models`, `select_version`, `select_model`, `set_config`, `get_state`, `resolved_components`, `resolved_args`, `validate`, `build`, `cancel`. Progress notifications pushed as JSON-RPC notifications during builds.
- **CLI `--interactive` flag**: Launches the JSON-RPC server.

### Verify:
- `echo '{"jsonrpc":"2.0","id":1,"method":"list_versions"}' | llamafile-builder --interactive` returns JSON response.
- Multi-step session via piped JSON-RPC: select version, select model, validate, build.
- Progress notifications appear as JSON-RPC notifications during download/build.
- This is the integration test for the protocol layer before adding HTTP (NFR-12).

---

## Phase 6: HTTP server and SSE

**Goal**: `--serve` mode exposes REST API + SSE for the web UI.

### Changes:
- **`server.rs`** (new): Axum router with `/api/...` endpoints from ARCHITECTURE.md. Static file serving from configurable web dir. Binds to `127.0.0.1`.
- **`sse.rs`** (new): Bridge tracing events -> SSE stream for build progress.

### Verify:
- `curl localhost:8765/api/versions` returns JSON
- `curl -N -X POST localhost:8765/api/build` returns SSE stream
- This is testable entirely via curl before building any web UI (NFR-12).

---

## Phase 7: Web UI port and cleanup

**Goal**: Browser-based wizard works. Old code removed.

### Changes:
- Copy `src/` (current Tauri frontend) to `web/`
- Replace `invoke()` -> `fetch()`, `listen()` -> `EventSource`
- Remove `src-tauri/`, `src/`
- Update `CLAUDE.md`, `README.md`

### Verify:
- `llamafile-builder --serve` opens browser, wizard completes a build
- All `cargo test --workspace` passes

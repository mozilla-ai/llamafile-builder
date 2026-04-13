# Implementation Plan: New llamafile-builder Architecture

## Context

The llamafile-builder is being rewritten from a monolithic Tauri desktop app into a Rust workspace with a core library (zero UI dependencies) and thin interface layers (CLI, TUI, HTTP server, JSON-RPC, web UI). This solves the fundamental problem that the builder's logic (downloading, configuring, assembling) is trapped inside renderer-dependent binaries that break on various platforms (WebKit in Snap, WGPU on Intel Macs).

Related docs:
- `specs/README.md` -- requirements (FR-1..7, NFR-1..13)
- `specs/ARCHITECTURE.md` -- architecture decisions
- `specs/LIBRARY.md` -- core library API
- `specs/CLI.md` -- CLI usage
- `specs/TUI.md` -- TUI reference
- `specs/JSON-RPC.md` -- JSON-RPC protocol
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

JSON config files carry version metadata in an envelope format:
```json
{ "config_version": 1, "min_builder_version": "0.1.0", "data": [ ... ] }
```
`ConfigManager::load()` parses both envelope and bare-array formats. `check_for_updates()` compares versions and detects when a builder update is required.

---

## Phase 5b: HuggingFace downloads via hf-hub -- DONE

Model downloads use the `hf-hub` crate and the shared HuggingFace cache at `~/.cache/huggingface/hub/`. Models in `models.json` use `"source": "hf"` with `filename` fields; the builder produces `ComponentSource::HuggingFace` which is resolved by the Cache via hf-hub's API.

Key design decisions:
- `hf_hub::Cache::from_env()` is used for local cache checks (pure filesystem, no network).
- `download_with_progress()` is called only on cache miss (it doesn't check its own cache).
- Lock file errors are surfaced to the user (not auto-removed — could be a concurrent download).
- No fallback to our own HTTP downloader for HF sources (interoperability with llama.cpp is the priority).

---

## Phase 5c: Zipalign as a managed dependency -- DONE

zipalign is downloaded and cached automatically from `zipalign_url` in each release entry in `versions.json`. The builder tries: custom zipalign → cached download → PATH lookup. The builder downloads it as phase 1 of the build pipeline, before any other component.

---

## Phase 5d: Multi-step CLI -- DONE

State is persisted to `~/.llamafile-builder/build-state.json` between invocations. `SavedState` is the universal intermediate representation: both one-shot mode and multi-step mode go through `build_from_state()` to create a configured Builder. Subcommands: `config set-*`, `config show`, `config show-args`, `config reset`.

---

## Phase 5e: JSON-RPC interactive mode -- DONE

`--interactive` mode exposes the Builder API over JSON-RPC 2.0 on stdin/stdout. 20 methods mirroring the Builder API. Stateful session with in-memory SavedState. Progress notifications pushed during builds.

Missing (deferred): `cancel`, `list_ollama_models`, `list_lmstudio_models` methods.

---

## Phase 5f: TUI with ratatui -- DONE

Full terminal UI wizard using `ratatui` and `crossterm`. Dashboard form with popup editors for each parameter. Multi-select GPU library picker driven by `versions.json` data. Schema-driven configuration editor. `.args` and output path editing via `tui-textarea`. Real-time log pane with level filtering. Non-blocking builds via `tokio::spawn` with shared cancel flag. Elapsed time display.

See `specs/TUI.md` for full reference.

Key design decisions:
- Tracing is the sole observability mechanism — the TUI has a custom `tracing::Layer` that captures events via `mpsc` channel.
- Build phase and download progress are extracted from structured tracing fields in log messages.
- The build task receives cloned state and reconstructs the Builder in the spawned task (must be `Send`-compatible).
- No `#[tracing::instrument]` on async functions (creates `!Send` spans).

---

## Phase 5g: Spec alignment -- IN PROGRESS

Audit and update all spec documents to match the implementation. Document the TUI, HF download strategy, tracing-only observability, config envelope format, and zip_path semantics. Add plan items for missing features (Ollama/LM Studio in TUI + JSON-RPC).

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

---

## Future work

### Ollama and LM Studio integration in TUI + JSON-RPC

The core library already has `ollama.rs` and `lmstudio.rs` for scanning local models. The CLI exposes them via `ollama list` and `lm-studio list`. These need to be added to:
- **TUI**: A model source selector (registry / Ollama / LM Studio / custom file) in the Model popup.
- **JSON-RPC**: `list_ollama_models` and `list_lmstudio_models` methods.
- **HTTP server**: `/api/ollama/models` and `/api/lmstudio/models` endpoints.

### JSON-RPC cancellation

Add a `cancel` method to the JSON-RPC protocol so long-running builds/downloads can be interrupted by the client.

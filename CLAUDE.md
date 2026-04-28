# CLAUDE.md

Guide for Claude Code sessions working in this repository. For the user-facing overview see [`README.md`](README.md); for design intent see the documents under [`specs/`](specs/).

## What this repo is

`llamafile-builder` assembles [llamafiles](https://github.com/Mozilla-Ocho/llamafile) — single-file APE executables that bundle an inference runtime, model weights, a `.args` configuration, and optional GPU acceleration libraries. The tool selects components, downloads them (or finds them in a shared cache), generates configuration, and invokes `zipalign` to produce the final executable.

The project is split into a **core library** (pure Rust, zero UI dependencies) and **thin interface layers** (CLI, TUI, HTTP server + web UI, JSON-RPC). All interfaces drive the same core; this is the central architectural principle (see [`specs/ARCHITECTURE.md`](specs/ARCHITECTURE.md)).

## Repository layout

```
.
├── crates/
│   ├── llamafile-builder-core/   library — state, downloads, assembly, validation
│   ├── llamafile-builder/        CLI binary + --serve (HTTP) + --interactive (JSON-RPC)
│   └── llamafile-builder-tui/    TUI binary (ratatui)
├── data/                         default versions.json, models.json, config.json
├── web/                          static assets served by `--serve` (incl. favicon)
├── assets/icons/                 icon masters (PNG/ICNS/ICO) for future packaging
├── specs/                        design documents — authoritative source of truth
├── Cargo.toml                    workspace manifest (members = ["crates/*"])
├── Makefile                      native + cross-compile build targets
├── README.md                     user-facing overview
└── CLAUDE.md                     this file
```

Produced binaries: `target/{debug,release}/llamafile-builder` and `llamafile-builder-tui`.

## Build and test

```bash
make setup        # install zig + cargo-zigbuild + cross-compile rust targets (macOS/Linux)
make              # debug build of both binaries
make test         # cargo test --workspace
make release      # release build
make all          # cross-compile all 5 targets (needs zig + cargo-zigbuild)
make target T=<triple>   # single cross-compile target
make dist         # cross-compile all targets and stage binaries under dist/<target>/
```

`make setup` is also what the release CI workflow runs before `make dist`; the GitHub Actions release job (`.github/workflows/release.yml`) attaches per-target archives plus `SHA256SUMS` to a published GitHub release.

When iterating, prefer `cargo check --workspace --all-targets` for fast validation and `cargo test --workspace` for correctness. The only pre-existing warnings are two `tempfile::TempDir::into_path` deprecations in test code — ignore them unless you're fixing that specifically.

## Architecture at a glance

- **`llamafile-builder-core`** owns all state, validation, downloads, config generation, and the build pipeline. Zero UI/renderer dependencies. Public API documented in [`specs/LIBRARY.md`](specs/LIBRARY.md).
- **`llamafile-builder`** (the CLI binary) provides three modes from a single binary: default CLI (`build`, `recipe`, `models`, …), `--serve` (HTTP + SSE + web UI), and `--interactive` (JSON-RPC 2.0 over stdin/stdout).
- **`llamafile-builder-tui`** is a separate ratatui binary. It links the core directly and uses its own tracing `Layer` to pipe events into an in-UI log pane.

Key shared abstractions (all in core):

- `Builder` — the main stateful builder. Selection methods configure it; `build()` runs the 5-phase pipeline (zipalign → runtime → model → .args → assemble). See [`specs/LIBRARY.md`](specs/LIBRARY.md#builder).
- `BuildState` / `SavedState` — internal state vs. the serialized form persisted in recipes. The CLI/JSON-RPC/server rebuild a fresh `Builder` from `SavedState` per invocation rather than mutating a long-lived one.
- `ConfigStore` — recipe storage (named, UUID-identified build configs persisted under `~/.llamafile-builder/recipes/`). Shared across all interfaces.
- `ConfigManager` — loads `versions.json`, `models.json`, `config.json` from disk with a bundled fallback, and optionally checks a remote URL for updates at startup.
- `Cache` — non-HF downloads (runtimes, zipalign, GPU libs). HF models go through `hf-hub` into the shared `~/.cache/huggingface/hub/`.

## Conventions and patterns

- **Observability is tracing-only.** The core library never writes to stdout/stderr; it emits structured `tracing` spans and events. The CLI formats them for stderr, the TUI captures them into its log pane. Don't add `println!`/`eprintln!` to core code.
- **`.args` generation is schema-driven.** `data/config.json` describes inputs (text, dropdown, slider, bool, section) with flags, conditionals, and defaults. Never hardcode flag emission in code — edit the schema.
- **Recipes, not session state.** Every build configuration is a recipe (a `BuildRecipe` holding a `SavedState`). When a user says "configure then build," they're mutating the current recipe, which auto-persists. Rebuilds always construct a fresh `Builder` from `SavedState` — removal/clear methods live on `SavedState`, not on `Builder`.
- **Remote-loaded config.** `versions.json`/`models.json`/`config.json` are **not** embedded at compile time (despite the bundled fallback). Adding a model or a llamafile release is a data change, not a code change. Respect this — don't hard-code entries in Rust.
- **HuggingFace cache interoperability.** Model downloads go through `hf-hub` and land in `~/.cache/huggingface/hub/` so they're shared with `llama.cpp`, the `huggingface_hub` Python library, and any other HF-aware tool. Don't reinvent this with custom HTTP — use the core `Cache::ensure` path.
- **Cancellation via shared atomic.** Long-running builds accept an `Arc<AtomicBool>` via `Builder::set_cancel_flag`; the caller flips it from a different task. There's no `Builder::cancel()` convenience method, because the `Builder` reference is typically moved into a background task and isn't reachable from where cancel is triggered.
- **No agent mode yet.** `AgentConfig` exists as a placeholder (FR-7) but no interface exposes a setter. Don't add agent wiring speculatively.
- **Tests span layers.** Unit tests in `llamafile-builder-core` exercise the library; tests in the CLI crate exercise the HTTP server and state-file layer; the TUI has its own unit tests for app state. Per NFR-12, every interface must be automatedly testable without the layers above it. Add tests at the lowest layer that still covers the behaviour.

## Specs — the source of truth

When in doubt, read the spec. When the code contradicts the spec, flag the drift and decide deliberately which side to update — don't silently paper over it.

| Document | Topic |
|---|---|
| [`specs/README.md`](specs/README.md) | What a llamafile is; what the builder does; FR-1…FR-7 and NFR-1…NFR-13 |
| [`specs/ARCHITECTURE.md`](specs/ARCHITECTURE.md) | Core/interface split; `Builder` / `Cache` / `ConfigManager` data types; build pipeline |
| [`specs/LIBRARY.md`](specs/LIBRARY.md) | Public API of `llamafile-builder-core` |
| [`specs/CLI.md`](specs/CLI.md) | CLI command reference (one-shot and recipe flows) |
| [`specs/TUI.md`](specs/TUI.md) | TUI layout, navigation, popups |
| [`specs/JSON-RPC.md`](specs/JSON-RPC.md) | Method reference for `--interactive` and the HTTP server |
| [`specs/PLAN.md`](specs/PLAN.md) | Implementation phases; what's done and what's "future work" |
| [`specs/CODE_REVIEW.md`](specs/CODE_REVIEW.md) | Historical: what was reused from the old Tauri codebase |

## Standard post-iteration checks

After any non-trivial code change, run through this sequence before declaring the iteration done:

1. **Spec alignment.** Are the claims in `specs/` still true? If code and spec disagree, decide which is correct and update the other. Don't let drift accumulate. When renaming types, methods, flags, or crate-level items, grep `specs/` and `README.md` for stale references before moving on.
2. **Test coverage.** Is the new feature (or bug fix) exercised by a test? If the change is a bug fix, add a regression guard that would have failed before the fix. Run `cargo test --workspace` and confirm no regressions.
3. **Integration tests with real artefacts.** For anything that touches the build pipeline, exercise it through the CLI with the smallest real model in the registry (`Qwen/Qwen3-0.6B-GGUF`). A few useful probes, all from [`specs/CLI.md`](specs/CLI.md):
   - `llamafile-builder versions` / `llamafile-builder models` / `llamafile-builder system-info`
   - `llamafile-builder --json <command>` — machine-readable variants
   - `llamafile-builder recipe new; recipe set-model …; recipe add-gpu …; recipe show-args; build` — the multi-step flow
   - `llamafile-builder build --model Qwen/Qwen3-0.6B-GGUF -o /tmp/smoke.llamafile` — full build; smoke-test the output with `--help`
   - For protocol changes, a `llamafile-builder --interactive` session with a few JSON-RPC requests piped in

Report what was checked and what wasn't, clearly. If an integration test reveals a separate bug, surface it explicitly rather than silently working around it.

## When making changes

- **Prefer editing existing files.** Don't add new `.md` files unless asked.
- **Match the cleanup ethos.** Before adding new API surface, check that there's a real consumer. The core library has already been trimmed of speculative methods — don't re-introduce them "for symmetry" without a caller.
- **Don't commit automatically.** The user drives commits and reviews diffs themselves. Make the edits, run the checks, and report; wait to be asked before `git commit`.
- **Legacy directories are gone.** `src-tauri/` and the old `src/` Tauri frontend were removed. Don't re-create them or reference them in new code.

## Known follow-ups

- **GPU lib cache collision** — `Cache::path_for` stores GPU libs at `<cache>/gpu/<filename>` with no version discriminator; different llamafile releases share filenames (`ggml-cuda.so`, etc.) so one would overwrite the other. Fix requires threading a version into `Component::GpuLib` and updating `path_for`.
- **Ollama / LM Studio model selection in TUI** — the browsing tabs exist but selecting and building from those sources needs end-to-end testing and polish.
- **JSON-RPC `cancel` method** — currently deferred (see [`specs/PLAN.md`](specs/PLAN.md) Phase 5e); long-running builds can't be cancelled over the protocol.

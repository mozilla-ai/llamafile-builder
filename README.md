# llamafile Builder

A tool for assembling [llamafiles](https://github.com/Mozilla-Ocho/llamafile) — single-file executables that bundle an inference runtime together with model weights, a configuration, and optional GPU acceleration libraries.

A llamafile itself is an [APE](https://justine.lol/ape.html) (Actually Portable Executable) that uses its ZIP container for extra data: the `llamafile` binary, one or more GGUF weight files, a `.args` configuration file, and optional `ggml-*.so`/`ggml-*.dll` GPU libraries. These components are combined using [`zipalign`](https://github.com/jart/zipalign). llamafile-builder is the "pick and mix" front-end for that workflow: it selects the components, downloads the ones that aren't already on disk, generates the configuration file, and invokes `zipalign` to produce the final executable.

## What it does

- **Runtime selection** — picks a llamafile runtime and matching `zipalign` binary from a versioned catalog. Downloads are cached; people who are building from source may also point at a freshly compiled binary.
- **Model selection** — browses a curated registry holding a HuggingFace subset of tested models, scans local Ollama and LM Studio caches, or accepts an arbitrary local `.gguf` file. Per-role quantization choice (weights, mmproj) where applicable.
- **Configuration** — allows for a custom `.args` file or one generated from user choices in the UI. The UI itself is configurable to provide different levels of detail.
- **GPU libraries** — optionally bundles pre-compiled CUDA, ROCm, or Vulkan libraries for Linux/Windows. Metal on macOS is compiled into the llamafile binary itself and needs nothing extra.
- **Downloads** — HTTP with resume, SHA256 verification, progress reporting. Relies on HuggingFace Hub for interoperability.
- **Recipes** — every build configuration is a JSON recipe you can save, reuse, and share with others.

## Interfaces

All interfaces drive the same core library, so they share behaviour and the recipe store.

### CLI

```bash
# One-shot build with defaults
llamafile-builder build --model Qwen/Qwen3-0.6B-GGUF

# Incremental: configure, inspect, build
llamafile-builder recipe set-model Qwen/Qwen3-0.6B-GGUF
llamafile-builder recipe set context_length 8192
llamafile-builder recipe add-gpu cuda
llamafile-builder recipe show
llamafile-builder build
```

Add `--json` for machine-readable output, e.g.:

```bash
llamafile-builder --json recipe show
```


See [`specs/CLI.md`](specs/CLI.md) for the full reference.

### TUI

```bash
llamafile-builder-tui
```

A [ratatui](https://ratatui.rs/) terminal wizard: dashboard form on top, live log pane below. See [`specs/TUI.md`](specs/TUI.md).

### Web UI

```bash
llamafile-builder --serve
# opens http://localhost:8765 in a browser
```

The CLI binary serves a browser-based wizard from the `web/` directory (static HTML/CSS/JS). 
Static assets are served from disk, so the UI can be modified or replaced without rebuilding.

### JSON-RPC

```bash
llamafile-builder --interactive
```

JSON-RPC 2.0 over stdin/stdout — one request per line, one response per line, progress events as notifications. Used for tests, scripting, and as the integration point for external GUIs. See [`specs/JSON-RPC.md`](specs/JSON-RPC.md).

## Architecture

The builder is split into a **core library** (pure Rust, no UI dependencies) and **thin interface layers** that call into it:

```
llamafile-builder-core (library)
       |
       +-- llamafile-builder  (one binary: CLI + --serve + --interactive)
       |
       +-- llamafile-builder-tui  (one binary: ratatui)
```

The core owns all state, validation, downloads, and assembly. Interface layers only handle argument parsing, rendering, and protocol translation. This keeps the logic portable to headless environments (CI, containers, SSH) and makes each layer independently testable. See [`specs/ARCHITECTURE.md`](specs/ARCHITECTURE.md) for the details.

The model registry, llamafile versions, and configuration schema are **not** embedded at compile time — they are fetched from a remote URL at startup (with a bundled fallback for first-run and offline use), so the catalog can be updated without shipping a new binary. See `NFR-9` and `NFR-10` in [`specs/README.md`](specs/README.md).

## Building

### Native (current platform)

```bash
make          # debug build
make test     # run the test suite
make release  # optimized build
```

Output binaries: `target/debug/llamafile-builder` and `target/debug/llamafile-builder-tui` (or `target/release/…`).

Only Rust (stable, 1.70+) is required for native builds.

### Cross-compilation

All five supported targets are cross-compilable from a single host using [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) and [Zig](https://ziglang.org/) as a drop-in C cross-compiler. No Docker, no VMs.

```bash
make setup    # one-time: installs zig, cargo-zigbuild, and required Rust targets
make all      # build all 5 targets (release)
make dist     # same, plus copy binaries to dist/
make target T=x86_64-unknown-linux-gnu   # single target
```

Supported targets:

| Target | Platform |
|--------|----------|
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-apple-darwin` | macOS Intel |
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 (e.g. Raspberry Pi) |
| `x86_64-pc-windows-gnu` | Windows x86_64 |

Binaries are 2.5–4 MB, statically linked for TLS (rustls).

## Project layout

```
.
├── crates/
│   ├── llamafile-builder-core/   pure library — state, downloads, assembly, validation
│   ├── llamafile-builder/    CLI + --serve (HTTP) + --interactive (JSON-RPC)
│   └── llamafile-builder-tui/    ratatui wizard
├── data/                 default versions.json, models.json, config.json (bundled fallback)
├── web/                  static assets served by --serve
├── assets/icons/         icon masters (icon.png 512×512, icon.icns, icon.ico) for future packaging
├── specs/                design documents — see below
├── Cargo.toml            workspace manifest
└── Makefile              native + cross-compile build targets
```

## Further reading

The `specs/` directory is the authoritative source of truth.

| Document | Topic |
|----------|-------|
| [`specs/README.md`](specs/README.md) | What a llamafile is, what the builder does, requirements (FR-1…, NFR-1…) |
| [`specs/ARCHITECTURE.md`](specs/ARCHITECTURE.md) | Core library / interface-layer split, data types, build pipeline |
| [`specs/LIBRARY.md`](specs/LIBRARY.md) | Public API reference for `llamafile-builder-core` |
| [`specs/CLI.md`](specs/CLI.md) | CLI command reference and examples |
| [`specs/TUI.md`](specs/TUI.md) | TUI layout, navigation, popups |
| [`specs/JSON-RPC.md`](specs/JSON-RPC.md) | JSON-RPC method reference (interactive mode + HTTP server) |

## License

Apache 2.0. See [`LICENSE`](LICENSE).

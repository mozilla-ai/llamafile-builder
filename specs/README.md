# llamafile Builder Specifications

## What is a llamafile

A llamafile is a single self-contained executable that bundles everything needed to run an LLM locally. It uses the [APE](https://justine.lol/ape.html) (Actually Portable Executable) format, which supports ZIP as a container for extra data. Its contents can be inspected with `unzip -vl <filename.llamafile>`.

A llamafile contains:

- **Inference code** -- the `llamafile` binary itself, compiled as an APE containing both ARM and x86 builds with architecture auto-detection at runtime. It relies on llama.cpp and custom code to provide an inference server, TUI chat, and CLI capabilities.

- **Model weights** -- one or more GGUF files. A single file for text-only LLMs; two files (LLM weights + mmproj weights) for multimodal models. Weights are typically downloaded from HuggingFace, where some repositories are gated and require an access token, but could in theory be downloaded from any URL (provided it is stored in the corresponding json configuration file) or from disk.

- **Configuration (.args)** -- a text file listing one argument per line, passed to the llamafile executable at startup. The model weights path is required; everything else (temperature, context length, server port, etc.) is optional. Files inside the bundle are referenced via `/zip/` paths. A trailing `...` token allows users to append additional CLI arguments at runtime.

- **GPU libraries** -- optional pre-compiled acceleration libraries (`ggml-cuda.so`, `ggml-rocm.so`, `ggml-vulkan.so`, or their `.dll` equivalents). Metal support on macOS is compiled on-the-fly from sources embedded in the llamafile binary itself.

- **Agentic code** -- (work in progress) extra code and data for agentic capabilities bundled alongside the other components.

These components are assembled using [zipalign](https://github.com/jart/zipalign), which appends ZIP entries to the APE executable. Files in the ZIP are accessible to llamafile's code under `/zip/` (e.g. `/zip/.args`, `/zip/model.gguf`). The zipalign binaries is an APE itself which should be available at bundling time.


## What llamafile-builder does

Due to its composable nature, llamafile lends itself well to a "pick and mix" workflow where users select components and bundle them into a portable executable.

llamafile-builder is a tool that makes this process accessible. Inspired by the [Raspberry Pi Imager](https://github.com/raspberrypi/rpi-imager), it guides users through a multi-step process that automatically downloads and bundles all (and only) the necessary components:

1. **Version** -- select a llamafile runtime / zipalign release (stable, latest, or legacy)
2. **Model** -- choose model weights and quantization level
3. **Configuration** -- set runtime parameters that become the `.args` file
4. **Compute** -- choose CPU-only or GPU-accelerated (CUDA, ROCm, Metal)
5. **Agent** -- optionally enable agent mode with tools and a system prompt
6. **Build** -- review selections, then download and assemble everything

The builder handles:

- **Binary discovery** -- access to both alpha and stable versions of llamafile and zipalign, from URLs (provided in config) or disk
- **Model discovery** -- a curated registry of models with quantization variants, plus scanning of local Ollama and LM Studio caches for models already on disk, and allowing users to manually specify local weights
- **Downloads** -- HTTP downloads with resume support, SHA256 verification, HuggingFace token resolution (environment variable, CLI cache, or stored token), progress reporting, and cancellation
- **Configuration generation** -- schema-driven `.args` file generation from a structured config definition. The configuration itself can be customized and supports sections, toggles, conditionals, and multiple input types (text, dropdown, slider, boolean)
- **GPU library bundling** -- platform-aware selection and download of the correct GPU acceleration libraries for the target compute configuration (same as with models, there should be an option to load libraries from disk)
- **Assembly** -- invoking zipalign to combine runtime binary + model weights + config + GPU libs into the final executable


## Who uses it

### Desktop users

People who want to run LLMs locally but are not comfortable with command-line tools. They expect a graphical interface that walks them through the process, handles downloads, and produces a ready-to-run file, ideally with as few choices to make as possible. This is the primary audience the tool was designed for.

### Power users and tinkerers

People comfortable with terminals who want fine-grained control: custom binaries, libraries, and model files from disk, specific quantizations, non-default server configurations, automation of repeated builds. They may prefer a CLI or scriptable interface over a GUI.

### CI/CD and automation

Build pipelines that produce llamafiles as artifacts -- for example, an organization that maintains a set of pre-configured llamafiles for internal use and rebuilds them when new model versions or llamafile releases come out. These workflows need non-interactive, scriptable operation with machine-readable output.


## Where it must run

llamafile itself is designed to run everywhere -- that is its core value proposition. The builder should not be the bottleneck that limits where llamafiles can be created.

### Target platforms

| Platform | Architecture | Priority | Notes |
|----------|-------------|----------|-------|
| macOS | ARM64 (Apple Silicon) | High | Primary development platform |
| macOS | x86_64 (Intel) | High | Still widely used |
| Linux | x86_64 | High | Servers, desktops, CI |
| Linux | ARM64 | Medium | Raspberry Pi 4/5, ARM servers |
| Linux | armhf | Low | Older Raspberry Pi models |
| Windows | x86_64 | Medium | Desktop users |

### Distribution channels

| Channel | Constraints |
|---------|------------|
| Direct download | No special constraints; user runs the binary |
| Homebrew / APT / AUR | Standard packaging; binary + optional data files |
| Snap (strict confinement) | No access to system libraries outside the snap; filesystem restrictions; GPU library paths vary by architecture |
| Flatpak | Similar sandboxing constraints to Snap |
| Container images | Headless; no display server available |
| Source build | User compiles from source; must work with standard Rust toolchain |


## Building

### Quick start

```bash
make          # debug build for current platform
make test     # run all 121 tests
make release  # release build for current platform
```

### Cross-compilation

All 5 supported targets can be cross-compiled from a Mac using `cargo-zigbuild` and Zig as a drop-in C cross-compiler. No Docker needed.

**One-time setup:**

```bash
make setup    # installs zig, cargo-zigbuild, and all Rust targets
```

Or manually:

```bash
brew install zig
cargo install cargo-zigbuild
rustup target add x86_64-apple-darwin x86_64-unknown-linux-gnu \
  aarch64-unknown-linux-gnu x86_64-pc-windows-gnu
```

**Build all targets:**

```bash
make all      # cross-compile CLI + TUI for all 5 targets (release)
make dist     # same, plus copy binaries to dist/
make sizes    # show binary sizes
```

**Build a single target:**

```bash
make target T=x86_64-unknown-linux-gnu
```

### Supported build targets

| Target | Platform |
|--------|----------|
| `aarch64-apple-darwin` | Mac Silicon |
| `x86_64-apple-darwin` | Mac Intel |
| `x86_64-unknown-linux-gnu` | Linux x86 |
| `aarch64-unknown-linux-gnu` | Linux ARM (Raspberry Pi) |
| `x86_64-pc-windows-gnu` | Windows x86 |

All binaries are 2.5–4 MB, statically linked for TLS (rustls, no OpenSSL dependency).

### Dependencies

| Dependency | What it's for | How to install |
|------------|---------------|----------------|
| Rust (stable) | Compiler | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Zig | C cross-compiler for `ring` (TLS crypto) | `brew install zig` |
| cargo-zigbuild | Bridges cargo and zig | `cargo install cargo-zigbuild` |

Zig and cargo-zigbuild are only needed for cross-compilation. Native builds (`make` / `make release`) require only Rust.


## The problem

The builder's functionality is pure computation: fetch data over HTTP, verify checksums, generate text files, invoke a command-line tool (zipalign). None of this inherently requires a graphical toolkit, a display server, or GPU-capable rendering.

Yet every approach tried so far has coupled this logic to a UI renderer that introduces platform-specific dependencies and compatibility walls:

### Tauri (current)

Tauri uses the system WebView (WebKitGTK on Linux, WebKit on macOS, WebView2/Edge on Windows). This works well in most desktop environments but breaks down at the edges:

- **Snap strict confinement**: WebKitGTK strips inline event handlers in sandboxed environments, breaking all UI interactions. EGL initialization fails due to library path restrictions. Architecture-specific WebKit library paths (`x86_64-linux-gnu`, `aarch64-linux-gnu`, `arm-linux-gnueabihf`) must be explicitly mapped in the snap layout. Working around these issues required rewriting 75+ event handlers, migrating GPU interfaces, and adding complex environment variable overrides -- fragile fixes that may break with future WebKit or snap updates.

- **Headless environments**: Tauri requires a display server. The builder cannot run in CI pipelines, containers, or SSH sessions without a virtual framebuffer (Xvfb), which is an unnecessary complication for a tool that fundamentally just downloads files and runs zipalign.

- **Linux dependency surface**: `libwebkit2gtk-4.0-dev`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev` must all be present. On minimal Linux installations (servers, containers, some ARM boards), these are heavy or unavailable.

### Dioxus with Blitz/WGPU (attempted)

Dioxus 0.7 with its native Blitz renderer avoids the WebView dependency but introduces its own:

- **Intel Mac incompatibility**: Blitz relies on WGPU for rendering, which has severe rendering issues on Intel Mac GPUs. The build script had to be restricted to ARM64 only, completely excluding Intel Mac users.

- **GPU requirement for rendering**: A tool that builds llamafiles (a CPU-bound packaging task) should not require a GPU-capable graphics stack just to display its UI.

### The pattern

Both approaches share the same structural problem: **the builder's core logic (downloading, verifying, configuring, assembling) is trapped inside a binary that can only run where its UI renderer works.** Every new platform or distribution channel requires fighting a new set of renderer-specific compatibility issues, and those fights produce fragile workarounds rather than stable solutions.


## Requirements

### Functional requirements

**FR-1: Component selection**
The builder must allow users to select each component of a llamafile: binaries (zipalign + llamafile runtime) version, model weights (with quantization choice), configuration parameters, compute target (CPU/GPU), and optional agent configuration.

**FR-2: Model discovery**
The builder must provide access to a curated model registry and support scanning of local model sources (Ollama cache, LM Studio cache, arbitrary GGUF files on disk).

**FR-3: Download management**
The builder must download components over HTTP with: progress reporting, resumption of interrupted downloads, SHA256 integrity verification, and cancellation. HuggingFace model downloads must use the standard HuggingFace Hub API so that downloaded files are cached in the shared HuggingFace cache at `~/.cache/huggingface/hub/` and are immediately available to other tools (llama.cpp, Python's `huggingface_hub`, etc.). Authentication for gated repositories is resolved from environment variables only (few GGUF repositories are gated, so a simple resolution strategy is sufficient).

**FR-4: Configuration generation**
The builder must generate valid `.args` files. Arguments change frequently (depending on new or changed llama.cpp / llamafile features), so we need a flexible configuration schema that describes more the parameter types and how they are presented in the UI (correctly handling sections, conditionals, toggleable groups, and placeholder substitution) rather than the parameter themselves. Users should also be able to customize the `.args` file before it is bundled with the llamafile.

**FR-5: Build assembly**
The builder must assemble the final llamafile by invoking zipalign to combine the runtime binary, model weights, configuration, and optional GPU libraries into a single executable. The zipalign binary itself must be treated as a managed dependency: it is downloaded and cached alongside the llamafile runtime (they come from the same release), not assumed to be pre-installed. Users may override with a local zipalign binary if preferred.

**FR-6: Build validation**
The builder must validate the build configuration before starting assembly (e.g. warn about context sizes that exceed model training length, flag missing dependencies, detect incompatible compute targets for the host platform).

**FR-7: Agent configuration**
The builder must support optional agent mode: setting a system prompt, selecting from available tools, and generating the corresponding `.args.prompt` file.

### Non-functional requirements

**NFR-1: Platform independence of core logic**
The core logic (everything listed in FR-1 through FR-7) must not depend on any UI toolkit, display server, GPU renderer, or platform-specific windowing library. It must be usable as a library, independent of any particular interface layer.

**NFR-2: Headless operation**
It must be possible to perform a complete build (from component selection through final assembly) without a display server, in environments like CI runners, containers, and SSH sessions.

**NFR-3: Scriptability**
All operations must be drivable from scripts and automation tools -- through command-line arguments, standard I/O, or a network API -- with machine-readable output and meaningful exit codes. The JSON-RPC interactive mode serves as the universal protocol: external GUIs, the web UI server, and eventually the CLI itself can all drive the builder through the same interface, ensuring consistent behavior across all frontends.

**NFR-4: Graphical access**
Desktop users must still have access to a graphical interface for the guided wizard workflow. This interface should not require installing additional system dependencies beyond what the builder binary itself provides.

**NFR-5: Progressive complexity**
Simple tasks (build a llamafile from a known model) should require minimal input. Advanced options (custom files, non-default config, GPU target selection) should be available but not mandatory. The builder's internal state should be manipulable in multiple steps (select version, then select model, then configure, then build) rather than requiring all choices to be specified at once. The CLI should support both one-shot mode (all options on the command line) and a future multi-step interactive mode where the user builds up the configuration incrementally.

**NFR-6: Platform coverage**
The builder must work on the platforms listed in the target platforms table. Adding support for a new platform or distribution channel should not require changes to the core logic.

**NFR-7: Offline capability**
Once components have been downloaded, the builder should be able to assemble a llamafile without network access. Cached downloads should be reusable across builds.

**NFR-8: Interoperability**
Our solutions should be as interoperable as possible with other open AI tools, so to limit the fingerprint of our own system. For instance, llama.cpp downloads models from HuggingFace by providing their HF id, and we should do the same to avoid downloading the same models in different files.

**NFR-9: Remote configuration**
The model registry, version list, and config schema must not be embedded at compile time. Instead, the builder fetches the most up-to-date configuration files from a default URL at startup. Configuration files are versioned: if a downloaded configuration requires a newer version of the builder than the one running, the user is notified to update the application. It must also be possible to override the configuration URL (for testing, custom registries, or air-gapped environments with a local mirror). A bundled fallback configuration ships with the binary for first-run and offline scenarios.

**NFR-10: Update checking**
At startup the builder checks for updates to its configuration files (and optionally to itself). The check behavior is user-configurable with three modes: always check, never check, or ask before checking (the default). A configuration-only update (new models, new llamafile versions) does not require updating the builder binary itself, unless the configuration version exceeds what the running binary supports.

**NFR-11: Web UI as separate assets**
If the builder serves a browser-based UI via a local HTTP server, the static assets (HTML, CSS, JS) should be served from disk rather than embedded in the binary. This allows modifying or replacing the UI without rebuilding the builder. The builder should track where assets are located and support cache headers so the browser can cache them efficiently.

**NFR-12: Layered testability**
Each interface layer must be automatically testable without the layers above it. The core library is tested via unit tests. The CLI (calling the library directly) serves as an integration test for the library. The JSON-RPC interactive mode is testable via piped stdin/stdout, serving as an integration test for the protocol and state management. The HTTP server is testable via HTTP requests. Each layer maximizes the automated test surface before moving to the next, so that bugs are caught at the lowest possible level rather than discovered only through manual UI testing.

**NFR-13: Llamafile modification (future)**
The builder's scope is assembling new llamafiles from components. However, the architecture should not preclude a future capability to open an existing llamafile and selectively replace components (e.g. update the runtime while preserving the model weights, or swap GPU libraries without re-downloading the model). This is not a launch requirement but should be kept in mind during design.

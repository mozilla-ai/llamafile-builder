# TUI Reference

The TUI provides an interactive terminal wizard for configuring and building llamafiles, using [ratatui](https://ratatui.rs/) and [crossterm](https://docs.rs/crossterm/).

```bash
cargo run -p llamafile-builder-tui    # debug
make release                  # or build release, then run:
./target/release/llamafile-builder-tui
```

See [README.md](README.md#building) for cross-compilation to other platforms.

## Layout

The screen is split into two regions:

- **Dashboard** (top): a single-column form showing all build parameters, a status bar, and action buttons.
- **Log pane** (bottom): a scrollable view of tracing events emitted by the core library during builds.

The status bar at the very bottom shows the description of the focused row and key hints.

## Dashboard rows

| Row | Focus key | Description |
|-----|-----------|-------------|
| Version | `Version` | Selected llamafile runtime version (group / release) |
| Model | `Model` | Selected model from the registry or a custom file |
| Weights | `Weights` | Quantization level for model weights |
| Mmproj | `Mmproj` | Quantization for the vision adapter (only shown for multimodal models) |
| Output | `Output` | Output path for the built `.llamafile` |
| Config | `Config` | Runtime configuration summary (context length, temperature, server mode) |
| GPU | `Gpu` | GPU acceleration libraries to bundle |
| Build / Show .args / Quit | `Build`, `ShowArgs`, `Quit` | Action buttons |

## Navigation

| Key | Action |
|-----|--------|
| `↑` / `↓` | Move focus between dashboard rows |
| `Enter` | Open popup editor for the focused row, or trigger Build |
| `Space` | Toggle mmproj on/off (when Mmproj row is focused) |
| `<` / `>` | Lower / raise log level filter (DEBUG → INFO → WARN → ERROR) |
| `[` / `]` | Scroll log pane up / down |
| `Ctrl+S` | Save current configuration as a named recipe |
| `Ctrl+L` | Load a previously saved recipe |
| `Ctrl+C` / `Q` | Quit (with confirmation if a build is running) |

## Popups

Each dashboard row opens a popup when Enter is pressed:

### Version Select
List of all versions from `versions.json`, grouped by version group. The currently active version is marked with `*`. Arrow keys navigate, Enter selects.

### Model Select
Searchable list of all registry models. Type to filter by name/org/tag. The currently active model is marked with `*`. Arrow keys navigate, Enter selects.

### Quantization Select
List of available quantizations for a file role (weights or mmproj). The currently active quantization is marked with `*`.

### Output Path
A `tui-textarea` single-line editor. Enter is blocked (path must stay one line). Esc saves and closes. Supports cursor movement, Home/End, and standard text editing.

### Configuration
Schema-driven editor generated from `config.json`. Shows all configuration items with current values. Sections can be collapsed by toggling the section header. Supports:
- Bool items: Space toggles
- Text/number items: Enter opens inline editor
- Dropdown items: Enter opens dropdown overlay
- Slider items: Left/Right arrows adjust value, Enter opens inline editor

Description of the focused item is shown at the top.

### GPU Libraries
Multi-select checklist of GPU libraries available in the selected release. Each line shows the acceleration type (CUDA/ROCm), OS, and download size. Space toggles selection. Multiple libraries can be selected simultaneously.

### .args Editor
A `tui-textarea` multi-line editor showing the generated `.args` content. Editing creates an override — the Config row becomes greyed out and unskippable in navigation. `Ctrl+R` resets to the auto-generated content. Esc saves and closes.

## Build lifecycle

1. User presses Enter on the Build row.
2. `build_status` is set to `Building { phase: "starting" }` and `build_started_at` is recorded.
3. The main loop detects the "starting" phase and spawns a `tokio::spawn` task that reconstructs a `Builder` from cloned app state.
4. The build task runs the full pipeline (zipalign → runtime → model → config → GPU → assemble).
5. Progress is communicated solely through tracing events: the main loop drains a `mpsc` channel of `LogEntry` values captured by a custom `tracing::Layer`.
6. Build phase and download progress are extracted from log message content as they arrive.
7. On completion, a `oneshot` channel delivers the result (path + size, or error).

The build can be cancelled by quitting the app (sets a shared `Arc<AtomicBool>` cancel flag).

## Tracing bridge

The TUI does not use any UI-facing progress types from the core library. Instead:

- A custom `TuiLogLayer` implements `tracing_subscriber::Layer` and forwards events to an `mpsc::Sender<LogEntry>`.
- The main loop drains the receiver on each tick and pushes entries into `app.logs`.
- Download progress is detected by matching structured fields in log messages (`bytes_downloaded`, `bytes_total`, `filename`).
- Build phase transitions are detected by matching INFO-level messages containing keywords (`preparing runtime`, `downloading`, `writing config`, `assembling`, etc.).
- The log pane filters entries by the user-selected log level.
- Noisy targets (`hyper`, `reqwest`, `rustls`) and connection pool messages are filtered out in the layer.

## Dependencies

- `ratatui` — terminal UI framework
- `crossterm` — terminal event handling
- `tui-textarea` — text editor widget (used for .args and output path)
- `llamafile-builder-core` — the core library (no UI dependencies)
- `tokio` — async runtime for non-blocking builds

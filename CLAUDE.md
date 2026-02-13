# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

llamafile Builder is a Tauri-based desktop application for building custom llamafiles with bundled models and configurations. It's a 6-step wizard UI (Version → Model → Configuration → Compute → Agent → Build).

## Development Commands

```bash
# Install Tauri CLI (first time only)
cargo install tauri-cli

# Development with hot-reload
cargo tauri dev

# Production build
cargo tauri build
# Output: src-tauri/target/release/bundle/
```

**Note**: JSON files (`config.json`, `models.json`, `versions.json`) are embedded at compile time. Changes to these require restarting `cargo tauri dev`.

## Architecture

### Frontend (`src/`)
- Pure HTML/CSS/JS with no build step
- `app.js` (2,500+ lines) contains all state management, rendering, and event handlers
- Edit and reload; no compilation needed

### Backend (`src-tauri/src/`)
- **main.rs** — Entry point, command registration
- **commands.rs** — Tauri IPC command handlers (30+ commands)
- **models.rs** — Data structures (serde enums/structs for models, config, versions)
- **builder.rs** — `.args` file generation from config schema
- **download.rs** — HTTP downloads with progress, SHA256 verification, resume support
- **validation.rs** — JSON config validation at startup
- **build_state.rs** — Centralized build state management

### Data Files (`src-tauri/`)
- **config.json** — Schema-driven UI configuration (inference settings, server mode)
- **models.json** — Model catalog with quantization options and HuggingFace URLs
- **versions.json** — llamafile version releases by version group

### Frontend-Backend Communication

```javascript
// Invoke Rust command from JS
const versions = await invoke('get_llamafile_versions');

// Listen for backend events
listen('build-progress', (event) => { /* handle progress */ });
```

```rust
// Define command in Rust
#[tauri::command]
async fn get_llamafile_versions() -> Result<Vec<LlamafileVersion>, String> { ... }

// Emit event to frontend
app_handle.emit_all("build-progress", &progress)?;
```

## Key Patterns

### Schema-Driven UI
Configuration UI is generated from `config.json` with type-tagged items:
- `Section` — Nested groups (can be toggleable)
- `Text` — String/number inputs
- `Dropdown` — Select options
- `Slider` — Range controls
- `Bool` — Checkboxes

### Startup Validation
`validation.rs` validates all JSON at startup. Malformed JSON causes panic with line/column info.

### Download System
`download.rs` handles HTTP range requests (resume), SHA256 checksums, HuggingFace token resolution (env var → ~/.cache → app-stored), and cancellation via atomic bool.

## Prerequisites

- Rust 1.70+
- Platform-specific Tauri dependencies:
  - **macOS**: Xcode Command Line Tools
  - **Linux**: `libwebkit2gtk-4.0-dev`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`
  - **Windows**: Microsoft Visual Studio C++ Build Tools

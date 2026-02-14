# Unused Symbols Analysis Report

Generated from `cargo check` warnings. Each symbol is categorized as:
- **PLACEHOLDER**: Part of unfinished feature, keep for future implementation
- **DEAD CODE**: No longer needed, safe to remove
- **SHOULD FIX**: Not unused, but needs attention (deprecation, etc.)

---

## Summary (Updated)

| Category | Count | Status |
|----------|-------|--------|
| Placeholder (keep) | 9 | Kept intentionally |
| Dead code (remove) | 3 | **REMOVED** |
| Should fix | 1 | **FIXED** |

### Cleanup Applied
- Removed `AsyncSeekExt` unused import
- Removed `get_models()` and `get_versions()` methods (frontend fetches JSON directly)
- Removed `format_bytes()` function (duplicated in JS frontend)
- Migrated `open_url` from deprecated `tauri-plugin-shell` to `tauri-plugin-opener`

---

## Detailed Analysis

### 1. `AsyncSeekExt` import (download.rs:8)

```rust
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
```

**Category:** DEAD CODE

**Why unused:** Was likely imported anticipating seek operations for resume functionality, but the current implementation uses `OpenOptions::append()` instead of seeking.

**Recommendation:** Remove the unused import.

---

### 2. `output_path` variable (builder.rs:303)

```rust
let output_path = Path::new(&config.output_path);
```

**Category:** PLACEHOLDER

**Why unused:** This is inside `build_llamafile()`, which is the real build orchestration function. The function itself is never called because builds currently use the mock implementation in `commands.rs::start_build()`.

**Recommendation:** Keep. Will be used when real build is implemented. Prefix with `_` to silence warning if desired.

---

### 3. `config` parameter (commands.rs:97)

```rust
pub async fn start_build(
    config: BuildConfig,  // unused
    app_handle: tauri::AppHandle,
) -> Result<(), String>
```

**Category:** PLACEHOLDER

**Why unused:** The `start_build` command currently simulates build stages for UI development. The `config` parameter will be used when the real `builder::build_llamafile()` is called.

**Recommendation:** Keep. Prefix with `_config` to silence warning if desired.

---

### 4. `Shell::open` deprecation (commands.rs:702)

```rust
app_handle.shell().open(&url, None)  // deprecated
```

**Category:** SHOULD FIX

**Why flagged:** Tauri has deprecated `tauri-plugin-shell` for URL opening in favor of `tauri-plugin-opener`.

**Recommendation:** Migrate to `tauri-plugin-opener`:
```rust
// Add to Cargo.toml: tauri-plugin-opener
// Then use:
app_handle.opener().open_url(&url, None)
```

---

### 5. `get_models()` and `get_versions()` methods (build_state.rs:352-356)

```rust
pub fn get_models(&self) -> Vec<Model> { ... }
pub fn get_versions(&self) -> Vec<LlamafileVersionGroup> { ... }
```

**Category:** DEAD CODE

**Why unused:** The frontend loads `models.json` and `versions.json` directly via `fetch()` for browser development compatibility. These Rust methods were intended to serve the data but are bypassed.

**Recommendation:** Remove. The frontend approach (fetch JSON directly) is simpler and works in both Tauri and browser environments. If server-side filtering/processing is needed later, these can be re-added.

---

### 6. `BuildError` variants: `Download`, `Process`, `Config`, `Cancelled` (builder.rs:14-23)

```rust
pub enum BuildError {
    Io(#[from] std::io::Error),
    Download(String),      // unused
    Process(String),       // unused
    Config(String),        // unused
    Cancelled,             // unused
}
```

**Category:** PLACEHOLDER

**Why unused:** These error variants are for the real build implementation in `build_llamafile()`. Since that function is never called, neither are these error paths.

**Recommendation:** Keep. They document the expected error cases for the build process.

---

### 7. `generate_system_prompt()` (builder.rs:246)

```rust
pub fn generate_system_prompt(agent: &AgentConfig) -> String
```

**Category:** PLACEHOLDER (Agent Mode)

**Why unused:** Generates the system prompt file for agent mode, appending tool descriptions. Agent mode is a planned feature visible in the UI but not yet wired to the backend.

**Recommendation:** Keep. This is part of the agent mode feature.

---

### 8. `estimate_size()` (builder.rs:263)

```rust
pub fn estimate_size(
    model_size: u64,
    compute: &ComputeConfig,
    agent: &AgentConfig,
) -> u64
```

**Category:** PLACEHOLDER (GPU/Agent Mode)

**Why unused:** Estimates final llamafile size including GPU library overhead (~180MB CUDA, ~60MB ROCm, ~10MB Metal) and agent overhead. Could be used for a "Estimated size" preview in the UI.

**Recommendation:** Keep. Useful for the Build step to show estimated output size.

---

### 9. `build_llamafile()` (builder.rs:292)

```rust
pub async fn build_llamafile(
    config: &BuildConfig,
    cache_dir: &Path,
    progress_callback: impl Fn(BuildProgress),
    cancel_check: impl Fn() -> bool,
) -> Result<(), BuildError>
```

**Category:** PLACEHOLDER (Core Build Logic)

**Why unused:** This is the **main build orchestration function**. It coordinates:
1. Download model weights to cache
2. Download llamafile runtime
3. Generate `.args` configuration (this part works via `generate_args_from_schema`)
4. Bundle GPU libraries if needed
5. Create final executable with zipalign

Currently replaced by mock implementation in `commands.rs::start_build()`.

**Recommendation:** Keep. This is the core of the real build feature.

---

### 10. `make_executable()` (builder.rs:436)

```rust
pub async fn make_executable(path: &Path) -> Result<(), BuildError>
```

**Category:** PLACEHOLDER

**Why unused:** Sets Unix executable permissions (chmod +x) on the output llamafile. Will be called by `build_llamafile()` after creating the executable.

**Recommendation:** Keep. Required for the final build step on Unix systems.

---

### 11. `zipalign_append()` (builder.rs:451)

```rust
pub async fn zipalign_append(
    llamafile_path: &Path,
    file_to_add: &Path,
    internal_name: &str,
) -> Result<(), BuildError>
```

**Category:** PLACEHOLDER

**Why unused:** Runs the `zipalign` tool to append files to a llamafile (model weights, .args, GPU libraries). Will be called by `build_llamafile()`.

**Recommendation:** Keep. Core functionality for building llamafiles.

---

### 12. `ResumeNotSupported` variant (download.rs:33)

```rust
pub enum DownloadError {
    // ...
    ResumeNotSupported,  // unused
}
```

**Category:** PLACEHOLDER

**Why unused:** Error for when a server doesn't support HTTP Range requests for resume. The current download implementation handles this case by falling back to full download without explicitly using this error variant.

**Recommendation:** Keep. May be useful for more explicit error handling or logging.

---

### 13. `format_bytes()` (download.rs:328)

```rust
pub fn format_bytes(bytes: u64) -> String
```

**Category:** DEAD CODE

**Why unused:** Utility to format bytes as "1.5 GB", "256 MB", etc. Has unit tests but is never called - the frontend has its own `formatBytes()` JavaScript function.

**Recommendation:** Remove. Duplication with frontend. If Rust-side logging needs formatted bytes, this can be re-added.

---

## Actions Taken

### Removed (3 items): DONE
- `download.rs:8` - Removed `AsyncSeekExt` from import
- `build_state.rs:352-358` - Removed `get_models()` and `get_versions()` methods
- `download.rs:328-341` - Removed `format_bytes()` function and its test

### Fixed deprecation (1 item): DONE
- Migrated `open_url` from `tauri-plugin-shell` to `tauri-plugin-opener`
- Added `tauri-plugin-opener = "2"` to Cargo.toml
- Added `opener:default` to capabilities/default.json
- Updated main.rs to init the opener plugin

### Kept as-is (9 remaining warnings):
All builder.rs functions and BuildError variants - they form the real build implementation that will replace the current mock. These warnings are expected until the real build is wired up.

### Optional: Silence with underscore prefix
If the placeholder warnings are distracting during development:
```rust
// builder.rs:303
let _output_path = Path::new(&config.output_path);

// commands.rs:97
pub async fn start_build(
    _config: BuildConfig,
    // ...
```

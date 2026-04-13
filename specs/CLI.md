# CLI Usage

Build the CLI:

```bash
cargo build -p llamafile-cli
alias lb=./target/debug/llamafile-builder
```

## Browsing available data

```bash
lb versions                             # available llamafile runtime versions
lb models                               # all models with default quantization and size
lb models show Qwen/Qwen3-0.6B-GGUF    # full model details (files, quantizations)
lb models --search qwen                 # filter by name/org
lb models --tag vision                  # filter by tag
lb system-info                          # OS, arch, memory, GPU detection
lb ollama list                          # local Ollama models
lb lm-studio list                       # local LM Studio models
```

Add `--json` before the subcommand for machine-readable output:

```bash
lb --json models show Qwen/Qwen3-0.6B-GGUF
```


## One-shot builds

All options on the command line. Uses defaults for anything not specified.

### Simplest case (qwen3-0.6b)

```bash
lb build --model Qwen/Qwen3-0.6B-GGUF
```

Uses defaults: latest version (0.10.0), Q8_0 quantization (the only one), server mode on, output to `~/Desktop/Qwen3-0.6B-Q8_0.llamafile`.

### Choosing a quantization (gpt-oss-20b)

```bash
lb build --model unsloth/gpt-oss-20b-GGUF                      # default: Q5_K_S
lb build --model unsloth/gpt-oss-20b-GGUF --quantization Q8_0   # explicit
lb build --model unsloth/gpt-oss-20b-GGUF --quantization MXFP4  # case-insensitive
```

`--quantization` applies to the weights role. Multimodal models automatically include the mmproj at its default quantization.

### Multimodal model (ministral-3-3b)

```bash
lb build --model mistralai/Ministral-3-3B-Instruct-2512-GGUF
```

Automatically includes both weights (Q4_K_M) and mmproj (BF16). The .args file contains `--mmproj /zip/<mmproj-filename>.gguf`.

### Overrides

```bash
lb build --model Qwen/Qwen3-0.6B-GGUF -o /tmp/qwen3.llamafile  # output path
lb build --model Qwen/Qwen3-0.6B-GGUF --version 0.9.3           # specific runtime
lb build --model Qwen/Qwen3-0.6B-GGUF --context-length 8192     # config override
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda                # add GPU library
lb build --model Qwen/Qwen3-0.6B-GGUF --args-file /tmp/my.args  # custom .args file
lb build --model /path/to/my-model.gguf                          # local file
lb build --model Qwen/Qwen3-0.6B-GGUF --runtime /path/to/llamafile  # custom runtime
```


## Multi-step builds

Configure incrementally, then build. State persists between invocations at `~/.llamafile-builder/build-state.json`.

### View default configuration

```bash
lb config show          # shows what would be built with all defaults
lb config show-args     # preview the .args file content
```

### Configure step by step

```bash
lb config set-model Qwen/Qwen3-0.6B-GGUF
lb config set server_enabled false
lb config set context_length 8192
lb config show           # verify
lb config show-args      # verify .args content
lb build                 # build from saved state
```

### All config commands

```bash
# Version
lb config set-version "llamafile 0.10" "0.10.0"
lb config set-runtime /path/to/llamafile    # custom binary

# Model
lb config set-model Qwen/Qwen3-0.6B-GGUF   # registry model (HF repo ID)
lb config set-model-file /path/to/model.gguf  # local file
lb config set-quantization weights Q8_0     # per-role quantization
lb config set-quantization mmproj Q8_0      # mmproj quantization

# Configuration
lb config set server_enabled false          # any config key
lb config set context_length 16384          # values are auto-parsed as JSON
lb config set-args-file /path/to/custom.args  # override entire .args

# Output
lb config set-output /tmp/my.llamafile

# GPU
lb config add-gpu cuda                      # add GPU library
lb config add-gpu cuda:linux                # specific OS
lb config remove-gpu cuda                   # remove

# Build tools
lb config set-zipalign /path/to/zipalign    # custom zipalign

# View and reset
lb config show                              # full state with all config values
lb config show-args                         # preview .args content
lb config reset                             # clear all saved state
```

### Defaults

On a fresh state (or after `config reset`), `config show` displays what would be built with all defaults:

```
Version:       llamafile 0.10 / 0.10.0 (default)
Model:         Qwen3 0.6B (weights:Q8_0) (default)
Output:        ~/Desktop/Qwen3-0.6B-Q8_0.llamafile (auto)
GPU libs:      (none)

Configuration (.args parameters):
  context_length = 4096
  enable_web_ui = true
  host = "127.0.0.1"
  ...
  server_enabled = true
  temperature = 0.7

Ready to build.
```

Overrides show the previous value:

```
  context_length = 8192 (was: 4096)
  server_enabled = false (was: true)
```

### One-shot and multi-step can be mixed

CLI args override saved state:

```bash
lb config set-model Qwen/Qwen3-0.6B-GGUF
lb build -o /tmp/override.llamafile         # uses saved model, overrides output
lb build --model unsloth/gpt-oss-20b-GGUF   # one-shot ignores saved state entirely
```


## GPU acceleration

```bash
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda           # CUDA for current OS
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda --gpu rocm  # multiple
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda:linux --gpu cuda:windows  # cross-platform
```


## Verbosity

```bash
RUST_LOG=info lb build --model Qwen/Qwen3-0.6B-GGUF   # build phases (default)
RUST_LOG=debug lb build --model Qwen/Qwen3-0.6B-GGUF  # download progress
RUST_LOG=warn lb build --model Qwen/Qwen3-0.6B-GGUF   # quiet
```


## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments (model not found, bad quantization name) |
| 3 | Download failed (network error, auth required) |
| 4 | Build failed (zipalign error, file copy error) |
| 5 | Cancelled |

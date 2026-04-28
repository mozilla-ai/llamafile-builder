# CLI Usage

Build the CLI (see [README.md](README.md#building) for cross-compilation):

```bash
make                # or: cargo build -p llamafile-builder
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

`--quantization` applies to the weights role. `--mmproj-quantization` selects the mmproj quantization. Multimodal models automatically include the mmproj at its default quantization if not specified.

### Multimodal model (ministral-3-3b)

```bash
lb build --model mistralai/Ministral-3-3B-Instruct-2512-GGUF
```

Automatically includes both weights (Q4_K_M) and mmproj (BF16). The .args file contains `--mmproj /zip/<mmproj-filename>.gguf`.

### Multimodal with explicit quantizations (qwen3-vl-4b)

```bash
lb build --model Qwen/Qwen3-VL-4B-Instruct-GGUF --quantization Q8_0 --mmproj-quantization Q8_0
```

Selects Q8_0 for both weights and vision adapter (instead of defaults Q4_K_M + F16).

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

Configure incrementally, then build. The current recipe persists between invocations at `~/.llamafile-builder/recipes/{uuid}.json`.

### View default configuration

```bash
lb recipe show               # shows what would be built with all defaults
lb recipe show-args          # preview the .args file content
lb recipe disk-requirements  # preview per-destination disk space needed
```

### Configure step by step

```bash
lb recipe set-model Qwen/Qwen3-0.6B-GGUF
lb recipe set server_enabled false
lb recipe set context_length 8192
lb recipe show           # verify
lb recipe show-args      # verify .args content
lb build                 # build from saved state
lb recipe save my-qwen   # name it for later reuse
```

### Recipe management

Build configurations are stored as **recipes** — named, UUID-identified configurations that persist across sessions and are shared by all interfaces.

```bash
lb recipe new                    # create a new empty recipe, set as current
lb recipe list                   # list all saved recipes (* = current)
lb recipe load my-qwen           # load a saved recipe by name or UUID
lb recipe save my-qwen           # name/save the current recipe (resolves all defaults)
lb recipe delete my-qwen         # delete a recipe by name or UUID
lb recipe reset                  # clear current recipe state (but keep the recipe)
```

Every `recipe set-*` mutation auto-persists to the current recipe. The `[recipe: ...]` indicator shows which recipe was updated.

When you `recipe save`, all defaults are frozen into a full snapshot — rebuilding from a saved recipe produces the same result regardless of future config file changes.

### All recipe set commands

```bash
# Version
lb recipe set-version "llamafile 0.10" "0.10.0"
lb recipe set-runtime /path/to/llamafile    # custom binary

# Model
lb recipe set-model Qwen/Qwen3-0.6B-GGUF   # registry model (HF repo ID)
lb recipe set-model-file /path/to/model.gguf  # local file
lb recipe set-ollama-model "qwen3:0.6b"     # local Ollama model
lb recipe set-lmstudio-model "Qwen3.5-0.8B-GGUF"  # local LM Studio model
lb recipe set-quantization weights Q8_0     # per-role quantization
lb recipe set-quantization mmproj Q8_0      # mmproj quantization

# Configuration
lb recipe set server_enabled false          # any config key
lb recipe set context_length 16384          # values are auto-parsed as JSON
lb recipe set-args-file /path/to/custom.args  # override entire .args

# Output
lb recipe set-output /tmp/my.llamafile

# GPU
lb recipe add-gpu cuda                      # add GPU library
lb recipe add-gpu cuda:linux                # specific OS
lb recipe remove-gpu cuda                   # remove

# Build tools
lb recipe set-zipalign /path/to/zipalign    # custom zipalign

# View
lb recipe show                              # full state with all config values
lb recipe show-args                         # preview .args content
lb recipe disk-requirements                 # per-destination space estimate (cache, hf cache, output)
```

### Defaults

On a fresh state (or after `recipe reset`), `recipe show` displays what would be built with all defaults:

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
lb recipe set-model Qwen/Qwen3-0.6B-GGUF
lb build -o /tmp/override.llamafile         # uses saved model, overrides output
lb build --model unsloth/gpt-oss-20b-GGUF   # one-shot ignores saved state entirely
```


## GPU acceleration

```bash
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda           # CUDA for current OS
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda --gpu rocm  # multiple
lb build --model Qwen/Qwen3-0.6B-GGUF --gpu cuda:linux --gpu cuda:windows  # cross-platform
```


## Interactive mode (JSON-RPC)

Start a stateful JSON-RPC 2.0 session over stdin/stdout:

```bash
lb --interactive
```

Send one JSON object per line on stdin, receive one response per line on stdout. Tracing goes to stderr. See [JSON-RPC.md](JSON-RPC.md) for the full protocol reference and examples.

Quick test:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"list_versions"}' | lb --interactive 2>/dev/null
```

Multi-step session:

```bash
cat << 'EOF' | lb --interactive 2>/dev/null
{"jsonrpc":"2.0","id":1,"method":"set_model","params":{"id":"Qwen/Qwen3-0.6B-GGUF"}}
{"jsonrpc":"2.0","id":2,"method":"set_config","params":{"key":"server_enabled","value":false}}
{"jsonrpc":"2.0","id":3,"method":"show_args"}
{"jsonrpc":"2.0","id":4,"method":"build"}
EOF
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
| 3 | Download failed (network error, auth required, insufficient disk space) |
| 4 | Build failed (zipalign error, file copy error) |
| 5 | Cancelled |

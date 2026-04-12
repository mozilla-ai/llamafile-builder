# CLI Usage Examples

All examples assume you're in the workspace root. Build the CLI first:

```bash
cargo build -p llamafile-cli
alias lb=./target/debug/llamafile-builder
```

## Listing available data

```bash
lb versions                         # available llamafile runtime versions
lb models                           # all models with default quantization and size
lb models show qwen3-0.6b           # full model details (quantizations, files, roles)
lb models --search qwen             # filter by name/org
lb models --tag vision              # filter by tag
lb system-info                      # OS, arch, memory, GPU detection
lb ollama list                      # local Ollama models
lb lm-studio list                   # local LM Studio models
```

Add `--json` before the subcommand for machine-readable output:

```bash
lb --json models show qwen3-0.6b
```


## Building llamafiles

### Example 1: qwen3-0.6b (simplest case)

Single weights file, one quantization (Q8_0, 639 MB). No mmproj.

```bash
lb build --model qwen3-0.6b
```

This uses all defaults: latest runtime version (0.10.0), Q8_0 quantization (the only one), default .args config (server mode, 4096 context), output to `~/Desktop/Qwen3-0.6B-Q8_0.llamafile`.

Override the output path:

```bash
lb build --model qwen3-0.6b -o /tmp/qwen3.llamafile
```

Use a specific runtime version:

```bash
lb build --model qwen3-0.6b --version 0.9.3
```

Override context length:

```bash
lb build --model qwen3-0.6b --context-length 8192
```


### Example 2: gpt-oss-20b (choosing between quantizations)

Three quantization levels for weights: Q5_K_S (11.7 GB), Q8_0 (12.1 GB), MXFP4 (12.1 GB). No mmproj.

Default quantization (Q5_K_S, first in the list):

```bash
lb build --model gpt-oss-20b
```

Specific quantization:

```bash
lb build --model gpt-oss-20b --quantization Q8_0
lb build --model gpt-oss-20b --quantization MXFP4
```

Note: `--quantization` applies to the weights role. The quantization name is case-insensitive.


### Example 3: ministral-3-3b (multimodal with mmproj)

Two weight quantizations (Q4_K_M, BF16) plus a vision adapter (mmproj, BF16 only). The mmproj is automatically included because the model defines it.

Default (Q4_K_M weights + BF16 mmproj):

```bash
lb build --model ministral-3-3b-instruct-2512
```

Full-precision weights:

```bash
lb build --model ministral-3-3b-instruct-2512 --quantization BF16
```

Both builds include the mmproj file automatically. The .args file will contain `--mmproj /zip/<mmproj-filename>.gguf`.


### Example 4: qwen3-vl-4b-instruct (multiple quantizations per role)

Two weight quantizations (Q4_K_M, Q8_0) and two mmproj quantizations (F16, Q8_0).

Default (Q4_K_M weights + F16 mmproj):

```bash
lb build --model qwen3-vl-4b-instruct
```

Higher-quality weights:

```bash
lb build --model qwen3-vl-4b-instruct --quantization Q8_0
```

Note: the current CLI only selects quantization for the weights role. The mmproj quantization stays at the default (first in the list, F16). Per-role quantization selection will be available in Phase 5d (multi-step CLI) via:

```bash
lb config set-model qwen3-vl-4b-instruct
lb config set-quantization weights Q8_0
lb config set-quantization mmproj Q8_0
lb build
```


## Using GPU acceleration

Add CUDA libraries for Linux:

```bash
lb build --model qwen3-0.6b --gpu cuda
```

Multiple GPU types:

```bash
lb build --model qwen3-0.6b --gpu cuda --gpu rocm
```

Specific OS target:

```bash
lb build --model qwen3-0.6b --gpu cuda:linux --gpu cuda:windows
```


## Using local files

Custom model file from disk:

```bash
lb build --model /path/to/my-model.gguf
```

Custom runtime binary:

```bash
lb build --model qwen3-0.6b --runtime /path/to/llamafile
```

Custom zipalign binary:

```bash
lb build --model qwen3-0.6b --zipalign /path/to/zipalign
```

Custom .args file (bypasses schema-driven generation):

```bash
lb build --model qwen3-0.6b --args-file /path/to/my.args
```


## Verbosity

The CLI logs to stderr via `tracing`. Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=info lb build --model qwen3-0.6b     # default, shows build phases
RUST_LOG=debug lb build --model qwen3-0.6b    # detailed, shows download progress
RUST_LOG=warn lb build --model qwen3-0.6b     # quiet, only warnings and errors
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

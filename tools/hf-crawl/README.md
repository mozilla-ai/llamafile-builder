# hf-crawl

A small CLI that crawls a Hugging Face GGUF repository and emits a
[`models.json`](../../data/models.json) entry for `llamafile-builder`.

It pulls everything it can from the Hugging Face API (file list, LFS SHA-256
pointers, license, tags), and asks a local OpenAI-compatible LLM to summarise
the README into a one-sentence description, pick the right capability tags,
and extract the context length.

## What it produces

For each repo, `hf-crawl` outputs a JSON object matching the layout of the
existing entries in `data/models.json`:

- `id`, `source: "hf"`, `name`, `organization`
- `parameters` / `param_count` — taken from HF's parsed GGUF metadata
  (`info.gguf.total`); falls back to a regex over the repo name (e.g.
  `0.6B` → 600 000 000) when HF hasn't parsed the file
- `license` (mapped from the HF `license` slug to a friendly form)
- `description` — one-sentence summary of the README (LLM)
- `tags` — subset of `chat`, `vision`, `code`, `reasoning`, `tools`, `audio`, `embedding` (LLM)
- `context_length` — taken from HF's parsed GGUF metadata
  (`info.gguf.context_length`); falls back to whatever the LLM extracts
  from the README, default `4096`
- `created_at` — ISO 8601 UTC timestamp of when the HF repo was created
  (`info.created_at`)
- `files` — grouped into `weights` and (if present) `mmproj` roles, each with
  one quantization per allowed name. File `size_bytes` and `sha256` come from
  the LFS pointer returned by the HF API — no GGUFs are downloaded.

By default, only the following quantizations are kept; anything else is
skipped:

```
Q4_K_M, Q6_K, Q8_0, MXFP4, F16, BF16
```

Override the list with `--quant NAME` (repeatable) — e.g.
`--quant Q4_K_M --quant Q8_0` to crawl just those two.

Two further filtering rules apply:

- **Multi-shard ggufs are dropped.** `llamafile-builder` cannot yet assemble
  a model split across `*-NNNNN-of-NNNNN.gguf` shards, so any such file is
  skipped and a log line is emitted. If the only available variant of a
  large-precision quant (often BF16) is sharded, that quant won't appear
  in the output for that model.
- **Plain variants are preferred over Unsloth-Dynamic (`UD-*`).** When a
  repo ships both `model-Q4_K_M.gguf` and `model-UD-Q4_K_M.gguf`, only the
  plain one is kept. `UD-*` files are only emitted when no plain variant
  exists for the same quant.

If two files still match the same quant after these rules (e.g. two model
variants packaged in one repo), the crawler aborts with an error listing
the conflicting filenames. Tighten `--quant` to drop the offending name
and pick a winner by hand.

## Setup

```sh
cd tools/hf-crawl
uv sync
```

You also need an OpenAI-compatible chat endpoint running locally — for example
`llama-server` from `llama.cpp`, or an `llamafile` built with `hf-crawl`'s
target model. The default URL is `http://localhost:8080`.

If the model repo is gated, log in with `huggingface-cli login` or export
`HF_TOKEN` before running.

## Usage

```sh
# Print a fresh models.json document containing the one crawled entry
uv run hf-crawl Qwen/Qwen3-0.6B-GGUF

# Override the display name
uv run hf-crawl Qwen/Qwen3-0.6B-GGUF --model-name "Qwen3 0.6B"

# Merge into an existing models.json and re-emit the whole document
uv run hf-crawl Qwen/Qwen3-1.7B-GGUF --input ../../data/models.json \
    > /tmp/models.json
```

Output is always a full `models.json` document — `config_version`,
`min_builder_version`, and a `data` array. Without `--input`, the array
holds just the new entry. With `--input`, the existing document is loaded
and the new entry is appended, or replaces an entry with the same `id`.

Add `--sort FIELD` to sort the `data` array by any entry field — common
choices are `name`, `created_at`, `param_count`, `organization`. Pair with
`--reverse` (`-r`) for descending order. Entries lacking the chosen field
are kept at the end regardless of direction.

```sh
uv run hf-crawl Qwen/Qwen3-1.7B-GGUF --input ../../data/models.json \
    --sort param_count --reverse
```

### LLM endpoint configuration

| Flag                | Env var               | Default                  |
|---------------------|-----------------------|--------------------------|
| `--llm-base-url`    | `HF_CRAWL_LLM_URL`    | `http://localhost:8080`  |
| `--llm-model`       | `HF_CRAWL_LLM_MODEL`  | `local`                  |
| `--llm-api-key`     | `HF_CRAWL_LLM_API_KEY`| _unset_                  |
| `--readme-chars`    | —                     | `8000`                   |

The LLM is called once per repo with the README (truncated to
`--readme-chars`) and must return a JSON object of the form

```json
{"description": "...", "tags": ["chat", "tools"], "context_length": 32768}
```

Most modern instruct models can do this reliably with
`response_format: {"type": "json_object"}`.

## Limitations

- `param_count` and `context_length` come from `info.gguf.*` when HF has
  parsed the GGUF (almost always the case today). When HF has not parsed
  it, `param_count` is derived from the repo name and `context_length`
  comes from the LLM's reading of the README. Repos with neither signal
  fall back to `parameters: "Unknown"`, `param_count: 0`,
  `context_length: 4096` — fix manually.
- `organization` defaults to the HF account prefix. If the *creator* differs
  from the uploader (e.g. `unsloth/gpt-oss-20b-GGUF` → "OpenAI"), edit the
  output JSON before committing.
- The script trusts the LFS-pointer SHA-256 returned by HF; it does not
  re-hash the files locally.

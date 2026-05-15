# version-crawl

A small CLI that builds a [`versions.json`](../../data/versions.json) entry
for a `llamafile` release by combining:

- the **GitHub release** (binary URL + size + sha256, zipalign URL + size +
  sha256, release date, release notes URL, release body for summarisation), and
- the **Hugging Face repo** that ships the GPU dynamic libraries
  (`ggml-cuda.{so,dll}`, `ggml-rocm.{so,dll}`, `ggml-vulkan.{so,dll}` — URL,
  size, sha256 from the LFS pointer).

Optionally asks a local OpenAI-compatible LLM to compress the release body
into a one-line feature summary in the style of the existing entries.

## What it produces

A release dict matching the layout of items under `data[*].releases[*]` in
[`data/versions.json`](../../data/versions.json):

- `version`, `description`, `release_date`
- `download_url`, `size_bytes`, `sha256` — the thin llamafile binary
- `zipalign_url`, `zipalign_size_bytes`, `zipalign_sha256`
- `gpu_libs.{cuda,rocm,vulkan}.{linux,windows}.{download_url,size_bytes,sha256}`,
  emitted only for backends/platforms whose `ggml-*` file actually exists in
  the HF repo

`description` is rendered as
`<feature list> ([release notes](<github release html_url>))`. The bracketed
suffix is appended by the tool — the LLM is only asked for the leading
feature list.

The GitHub asset sha256 is read from the API's `digest` field; if that field
is missing (older release), the tool streams the asset and hashes it locally.

## Setup

```sh
cd tools/version-crawl
uv sync
```

If you want LLM-generated descriptions, you also need an OpenAI-compatible
chat endpoint running locally — for example `llama-server` from `llama.cpp`,
or an `llamafile` built with a suitable model. Default URL: `http://localhost:8080`.

To increase the GitHub API rate limit, export `GH_TOKEN` (or `GITHUB_TOKEN`).

## Usage

```sh
# Print just the new release entry on stdout
uv run version-crawl 0.10.1 mozilla-ai/llamafile_0.10

# Merge into an existing versions.json — the new entry is inserted at the
# top of the matching family group's 'releases' list, and the full document
# is printed.
uv run version-crawl 0.10.1 mozilla-ai/llamafile_0.10 \
    --input ../../data/versions.json > /tmp/versions.json

# Skip the LLM and provide the description by hand
uv run version-crawl 0.10.1 mozilla-ai/llamafile_0.10 \
    --description "Server, CLI, chat, combined mode, Metal+CUDA+Vulkan support on Linux+Windows"
```

If a release entry with the same `version` already exists in the matched
group, it is replaced rather than duplicated.

### Choosing the target group

By default the new entry is added to the group whose existing releases share
the `MAJOR.MINOR` family with the new version (e.g. `0.10.x` → the
`"llamafile 0.10"` group). If no such group exists, the first group with
`tag: "latest"` is used.

Override with `--target-group "<name>"` to force a specific group.

### LLM endpoint configuration

| Flag                | Env var                    | Default                  |
|---------------------|----------------------------|--------------------------|
| `--llm-base-url`    | `VERSION_CRAWL_LLM_URL`    | `http://localhost:8080`  |
| `--llm-model`       | `VERSION_CRAWL_LLM_MODEL`  | `local`                  |
| `--llm-api-key`     | `VERSION_CRAWL_LLM_API_KEY`| _unset_                  |
| `--body-chars`      | —                          | `6000`                   |

The LLM is called once per release with the release body (truncated to
`--body-chars`) and must return a JSON object of the form

```json
{"description": "Server, CLI, chat, combined mode, Metal+CUDA+Vulkan support on Linux+Windows"}
```

Most modern instruct models can do this reliably with
`response_format: {"type": "json_object"}`.

## Limitations

- Only the **thin** llamafile binary is captured. The fat `llamafile-<v>.zip`
  and `llamafile-<v>` assets, and `whisperfile-*`, are ignored.
- Only the three GPU backends `cuda`, `rocm`, `vulkan` are emitted by
  default (matching the current `versions.json` schema). The HF repo may
  also expose variants like `ggml-cuda-cublas.{so,dll}` and
  `ggml-cuda-tinyblas.{so,dll}`; these are skipped. Use `--gpu-type` to
  override the allowed set if the schema later grows new backends.
- The HF API has no notion of "this file belongs to release X". The tool
  trusts that the HF repo passed on the command line holds the GPU libs
  that go with the GitHub release; pick the right one (e.g.
  `mozilla-ai/llamafile_0.10` for any `0.10.x` release).
- GitHub asset SHA256 is taken from the API `digest` field when present,
  otherwise computed by streaming the asset locally (slower but correct).
  The HF LFS sha256 is trusted as-is — files are not re-hashed.

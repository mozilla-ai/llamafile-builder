"""HF Hub crawling — extracts the fields needed for a models.json entry."""

from __future__ import annotations

import logging
import re
from datetime import timezone
from typing import Iterable

from huggingface_hub import HfApi, ModelCard
from huggingface_hub.errors import HfHubHTTPError, RepositoryNotFoundError
from huggingface_hub.utils import EntryNotFoundError

from .errors import HfRepoError
from .llm import LlmClient

log = logging.getLogger(__name__)


# Quantization names we accept. Match is case-insensitive; the canonical
# (uppercase) form is emitted.
ALLOWED_QUANTS_DEFAULT: tuple[str, ...] = (
    "Q4_K_M",
    "Q6_K",
    "Q8_0",
    "MXFP4",
    "F16",
    "BF16",
)

# Tag enum we want to emit (must match the proposed schema update).
TAG_ENUM: tuple[str, ...] = (
    "chat",
    "vision",
    "code",
    "reasoning",
    "tools",
    "audio",
    "embedding",
)


# Map of common HF license slugs to human-friendly forms used in models.json.
LICENSE_FRIENDLY: dict[str, str] = {
    "apache-2.0": "Apache 2.0",
    "mit": "MIT",
    "bsd-3-clause": "BSD 3-Clause",
    "bsd-2-clause": "BSD 2-Clause",
    "gpl-3.0": "GPL 3.0",
    "lgpl-3.0": "LGPL 3.0",
    "agpl-3.0": "AGPL 3.0",
    "cc-by-4.0": "CC BY 4.0",
    "cc-by-sa-4.0": "CC BY-SA 4.0",
    "cc-by-nc-4.0": "CC BY-NC 4.0",
    "cc0-1.0": "CC0 1.0",
    "llama2": "Llama 2 Community License",
    "llama3": "Llama 3 Community License",
    "llama3.1": "Llama 3.1 Community License",
    "llama3.2": "Llama 3.2 Community License",
    "llama3.3": "Llama 3.3 Community License",
    "gemma": "Gemma Terms of Use",
    "other": "Other",
}


def _humanize_license(value: str | None) -> str:
    if not value:
        return "Unknown"
    return LICENSE_FRIENDLY.get(value.lower(), value)


def _humanize_repo_name(repo_id: str) -> str:
    """Derive a display name from a repo id like 'Qwen/Qwen3-0.6B-GGUF'."""
    _, _, tail = repo_id.partition("/")
    tail = re.sub(r"-?GGUF$", "", tail, flags=re.IGNORECASE)
    return tail.replace("-", " ").replace("_", " ").strip()


def _format_param_count(n: int) -> str:
    """Render a parameter count as a friendly 'B' string ('7.5B', '20B', '0.6B')."""
    billions = n / 1_000_000_000
    s = f"{billions:.1f}" if billions >= 1 else f"{billions:.2f}"
    if "." in s:
        s = s.rstrip("0").rstrip(".")
    return f"{s}B"


def _extract_param_count(repo_name: str) -> tuple[str, int] | None:
    """Return (parameters, param_count) parsed from a repo name, or None.

    Recognises tokens like '7B', '0.6B', '1.7B', '20B', and treats hyphens
    between digits as decimal separators (e.g. '3-3B' -> '3.3B'). Tokens
    immediately followed by another letter (e.g. 'Bit') are ignored.
    """
    # Look at the segment after the org prefix only.
    _, _, tail = repo_name.partition("/")
    # Lookbehind prevents matching the '3' inside 'Qwen3-8B' as the size; the
    # leading digit must be at a word boundary so '-3-3B' still matches.
    match = re.search(r"(?<![A-Za-z0-9])(\d+(?:[.-]\d+)?)\s*[Bb](?![A-Za-z])", tail)
    if not match:
        return None
    raw = match.group(1).replace("-", ".")
    try:
        value = float(raw)
    except ValueError:
        return None
    parameters = f"{raw}B"
    param_count = int(round(value * 1_000_000_000))
    return parameters, param_count


def _quant_from_filename(filename: str, allowed: Iterable[str]) -> str | None:
    """Match a quantization name from a filename. Case-insensitive."""
    name = filename.rsplit("/", 1)[-1].rsplit(".", 1)[0]
    # Try longest-first so 'Q4_K_M' wins over a hypothetical shorter prefix.
    for quant in sorted(allowed, key=len, reverse=True):
        pattern = rf"(?<![A-Za-z0-9_]){re.escape(quant)}(?![A-Za-z0-9_])"
        if re.search(pattern, name, re.IGNORECASE):
            return quant
    return None


def _classify_role(filename: str) -> str:
    return "mmproj" if "mmproj" in filename.lower() else "weights"


def _lfs_sha256(sibling) -> str | None:
    lfs = getattr(sibling, "lfs", None)
    if lfs is None:
        return None
    # lfs is a BlobLfsInfo (attribute access) on modern hub, dict on older.
    if isinstance(lfs, dict):
        return lfs.get("sha256")
    return getattr(lfs, "sha256", None)


def _lfs_size(sibling) -> int | None:
    size = getattr(sibling, "size", None)
    if size is not None:
        return size
    lfs = getattr(sibling, "lfs", None)
    if isinstance(lfs, dict):
        return lfs.get("size")
    return getattr(lfs, "size", None) if lfs else None


def _collect_files(siblings, allowed_quants: Iterable[str]) -> list[dict]:
    """Group gguf siblings by role, filter to allowed quants, sort by size."""
    by_role: dict[str, list[dict]] = {"weights": [], "mmproj": []}
    for sib in siblings:
        filename = sib.rfilename
        if not filename.lower().endswith(".gguf"):
            continue
        quant = _quant_from_filename(filename, allowed_quants)
        if quant is None:
            log.info("skipping %s: no allowed quantization match", filename)
            continue
        size = _lfs_size(sib)
        sha = _lfs_sha256(sib)
        if size is None or sha is None:
            log.warning("skipping %s: missing LFS size or sha256", filename)
            continue
        role = _classify_role(filename)
        by_role[role].append(
            {
                "name": quant,
                "filename": filename,
                "size_bytes": int(size),
                "sha256": sha,
            }
        )

    files: list[dict] = []
    if by_role["weights"]:
        files.append(
            {
                "role": "weights",
                "label": "Model Weights",
                "quantizations": sorted(by_role["weights"], key=lambda q: q["size_bytes"]),
            }
        )
    if by_role["mmproj"]:
        files.append(
            {
                "role": "mmproj",
                "label": "Vision Adapter (mmproj)",
                "quantizations": sorted(by_role["mmproj"], key=lambda q: q["size_bytes"]),
            }
        )
    return files


def _load_readme(repo_id: str) -> str:
    try:
        card = ModelCard.load(repo_id)
    except (EntryNotFoundError, OSError) as exc:
        log.warning("could not load model card for %s: %s", repo_id, exc)
        return ""
    return card.content or ""


_LLM_SYSTEM = """\
You extract structured metadata about an LLM from its Hugging Face model card.
You always respond with a single JSON object, no prose, no markdown.
"""

_LLM_USER_TEMPLATE = """\
Below is the README of a Hugging Face model repository. Extract:

- description: ONE sentence (max ~25 words) summarising what this model is and \
its notable capabilities. Do not copy a paragraph verbatim.
- tags: subset of {tag_enum}. Apply these meanings:
    chat       - the model is conversational / instruction-tuned
    vision     - accepts images (multimodal)
    code       - specialised or notably strong at code generation
    reasoning  - has explicit reasoning / chain-of-thought modes
    tools      - supports tool / function calling
    audio      - accepts audio input
    embedding  - is an embedding model (not generative)
  Include "chat" for any instruction/chat-tuned generative model. Omit "chat" \
for pure embedding models.
- context_length: integer max context window in tokens. If multiple are \
mentioned, pick the default/native one. Use 4096 if you cannot determine it.

Reply with exactly this JSON shape:
{{"description": "...", "tags": ["..."], "context_length": 4096}}

Repo id: {repo_id}
HF tags: {hf_tags}

README:
---
{readme}
---
"""


def _llm_enrich(
    *,
    repo_id: str,
    hf_tags: list[str],
    readme: str,
    llm: LlmClient,
    readme_char_limit: int,
) -> dict:
    truncated = readme[:readme_char_limit]
    user = _LLM_USER_TEMPLATE.format(
        tag_enum=list(TAG_ENUM),
        repo_id=repo_id,
        hf_tags=hf_tags,
        readme=truncated or "(README is empty)",
    )
    reply = llm.chat_json(_LLM_SYSTEM, user)
    return reply


def _coerce_tags(raw) -> list[str]:
    if not isinstance(raw, list):
        return ["chat"]
    seen: list[str] = []
    for item in raw:
        if isinstance(item, str) and item in TAG_ENUM and item not in seen:
            seen.append(item)
    return seen or ["chat"]


def crawl(
    *,
    repo_id: str,
    display_name: str | None,
    llm: LlmClient,
    allowed_quants: Iterable[str] = ALLOWED_QUANTS_DEFAULT,
    readme_char_limit: int = 8000,
) -> dict:
    """Return a models.json entry for the given repo."""
    api = HfApi()
    try:
        info = api.model_info(repo_id, files_metadata=True)
    except RepositoryNotFoundError as exc:
        raise HfRepoError(
            f"{repo_id}: repository not found (or gated and you are not "
            f"authenticated — run `huggingface-cli login`)"
        ) from exc
    except HfHubHTTPError as exc:
        raise HfRepoError(f"{repo_id}: Hugging Face API error: {exc}") from exc

    files = _collect_files(info.siblings or [], allowed_quants)
    if not files:
        raise HfRepoError(
            f"{repo_id}: no GGUF files matched the allowed quantizations "
            f"({sorted(allowed_quants)})"
        )

    org_prefix, _, _ = repo_id.partition("/")
    name = display_name or _humanize_repo_name(repo_id)

    # Prefer HF's parsed GGUF metadata: it knows the real param count and the
    # native context length from the file itself. Fall back to repo-name regex
    # only when HF hasn't parsed the GGUF (older or unusual repos).
    gguf_meta = getattr(info, "gguf", None)
    if not isinstance(gguf_meta, dict):
        gguf_meta = {}
    gguf_total = gguf_meta.get("total")
    gguf_ctx = gguf_meta.get("context_length")

    if isinstance(gguf_total, int) and gguf_total > 0:
        param_count = gguf_total
        parameters = _format_param_count(gguf_total)
    else:
        params = _extract_param_count(repo_id)
        parameters = params[0] if params else "Unknown"
        param_count = params[1] if params else 0

    card_data = getattr(info, "cardData", None) or getattr(info, "card_data", None) or {}
    license_value = None
    if isinstance(card_data, dict):
        license_value = card_data.get("license")
    else:
        license_value = getattr(card_data, "license", None)
    license_str = _humanize_license(license_value)

    readme = _load_readme(repo_id)
    enrich = _llm_enrich(
        repo_id=repo_id,
        hf_tags=list(info.tags or []),
        readme=readme,
        llm=llm,
        readme_char_limit=readme_char_limit,
    )

    description = str(enrich.get("description", "")).strip() or name
    tags = _coerce_tags(enrich.get("tags"))
    if isinstance(gguf_ctx, int) and gguf_ctx > 0:
        context_length = gguf_ctx
    else:
        try:
            context_length = int(enrich.get("context_length", 4096))
        except (TypeError, ValueError):
            context_length = 4096

    created_at = (
        info.created_at.astimezone(timezone.utc).replace(microsecond=0).isoformat()
    )

    entry: dict = {
        "id": repo_id,
        "source": "hf",
        "name": name,
        "organization": org_prefix,
        "parameters": parameters,
        "param_count": param_count,
        "description": description,
        "license": license_str,
        "tags": tags,
        "context_length": context_length,
        "created_at": created_at,
        "files": files,
    }
    return entry

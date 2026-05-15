"""Crawl a llamafile release across GitHub + Hugging Face into a versions.json entry."""

from __future__ import annotations

import hashlib
import logging
import os
import re
from dataclasses import dataclass
from datetime import datetime
from typing import Iterable

import httpx
from huggingface_hub import HfApi
from huggingface_hub.errors import HfHubHTTPError, RepositoryNotFoundError

from .errors import GhReleaseError, HfRepoError
from .llm import LlmClient

log = logging.getLogger(__name__)


GH_API_BASE = "https://api.github.com"
GH_DEFAULT_REPO = "mozilla-ai/llamafile"

# GPU library naming: the existing versions.json schema names three backends.
GPU_TYPES_DEFAULT: tuple[str, ...] = ("cuda", "rocm", "vulkan")

_GGML_RE = re.compile(r"^ggml-([a-z0-9]+)\.(so|dll)$", re.IGNORECASE)
_PLATFORM_BY_EXT = {"so": "linux", "dll": "windows"}


# --------------------------------------------------------------------------- GH


@dataclass(frozen=True)
class _GhAsset:
    name: str
    url: str
    size: int
    sha256: str


@dataclass(frozen=True)
class _GhRelease:
    version: str
    html_url: str
    published_at: str  # ISO date (YYYY-MM-DD)
    body: str
    binary: _GhAsset
    zipalign: _GhAsset


def _gh_headers() -> dict[str, str]:
    headers = {
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return headers


def _gh_release_json(repo: str, version: str) -> dict:
    url = f"{GH_API_BASE}/repos/{repo}/releases/tags/{version}"
    try:
        resp = httpx.get(url, headers=_gh_headers(), timeout=60.0, follow_redirects=True)
    except httpx.HTTPError as exc:
        raise GhReleaseError(f"could not reach {url}: {exc}") from exc
    if resp.status_code == 404:
        raise GhReleaseError(
            f"{repo} has no release tagged {version!r} (or it is a draft / private)"
        )
    if resp.status_code >= 400:
        body = (resp.text or "")[:300].replace("\n", " ")
        raise GhReleaseError(
            f"GitHub API returned HTTP {resp.status_code} for {url}: {body}"
        )
    return resp.json()


def _stream_sha256(url: str) -> tuple[str, int]:
    """Stream-download `url` and return (sha256_hex, total_bytes)."""
    h = hashlib.sha256()
    total = 0
    with httpx.stream(
        "GET", url, headers=_gh_headers(), timeout=300.0, follow_redirects=True
    ) as resp:
        if resp.status_code >= 400:
            raise GhReleaseError(
                f"could not download {url}: HTTP {resp.status_code}"
            )
        for chunk in resp.iter_bytes(chunk_size=1 << 20):
            h.update(chunk)
            total += len(chunk)
    return h.hexdigest(), total


def _asset_sha256(asset: dict) -> str | None:
    """Return the asset's sha256 from the GH `digest` field, or None."""
    digest = asset.get("digest")
    if not isinstance(digest, str):
        return None
    algo, _, value = digest.partition(":")
    if algo.lower() != "sha256" or not value:
        return None
    return value.lower()


def _make_gh_asset(asset: dict) -> _GhAsset:
    name = asset["name"]
    url = asset["browser_download_url"]
    size = int(asset["size"])
    sha = _asset_sha256(asset)
    if sha is None:
        log.info("asset %s has no digest in GH API; streaming to hash", name)
        sha, hashed_size = _stream_sha256(url)
        if hashed_size != size:
            log.warning(
                "asset %s: GH reported size %d but streamed %d bytes",
                name, size, hashed_size,
            )
            size = hashed_size
    return _GhAsset(name=name, url=url, size=size, sha256=sha)


def _pick_thin_asset(assets: list[dict], version: str) -> dict:
    """Pick the 'thin' llamafile binary. Errors if zero or multiple match."""
    candidates = [
        a for a in assets
        if a["name"].lower().startswith("llamafile")
        and "thin" in a["name"].lower()
    ]
    if not candidates:
        raise GhReleaseError(
            f"release {version}: no asset matched 'llamafile*thin*' "
            f"(found: {[a['name'] for a in assets]})"
        )
    if len(candidates) > 1:
        names = ", ".join(a["name"] for a in candidates)
        raise GhReleaseError(
            f"release {version}: multiple 'thin' assets matched: {names}. "
            f"Tighten the pattern or pick one by hand."
        )
    return candidates[0]


def _pick_zipalign_asset(assets: list[dict], version: str) -> dict:
    candidates = [a for a in assets if a["name"].lower().startswith("zipalign")]
    if not candidates:
        raise GhReleaseError(
            f"release {version}: no asset matched 'zipalign*'"
        )
    if len(candidates) > 1:
        names = ", ".join(a["name"] for a in candidates)
        raise GhReleaseError(
            f"release {version}: multiple zipalign assets matched: {names}"
        )
    return candidates[0]


def crawl_gh_release(repo: str, version: str) -> _GhRelease:
    data = _gh_release_json(repo, version)
    assets = data.get("assets") or []
    binary_asset = _make_gh_asset(_pick_thin_asset(assets, version))
    zipalign_asset = _make_gh_asset(_pick_zipalign_asset(assets, version))
    published_at = data.get("published_at") or ""
    try:
        date_only = datetime.fromisoformat(
            published_at.replace("Z", "+00:00")
        ).date().isoformat()
    except ValueError:
        date_only = published_at[:10] if published_at else ""
    return _GhRelease(
        version=version,
        html_url=data.get("html_url") or "",
        published_at=date_only,
        body=data.get("body") or "",
        binary=binary_asset,
        zipalign=zipalign_asset,
    )


# --------------------------------------------------------------------------- HF


def _hf_lfs_sha256(sibling) -> str | None:
    lfs = getattr(sibling, "lfs", None)
    if lfs is None:
        return None
    if isinstance(lfs, dict):
        return lfs.get("sha256")
    return getattr(lfs, "sha256", None)


def _hf_lfs_size(sibling) -> int | None:
    size = getattr(sibling, "size", None)
    if size is not None:
        return size
    lfs = getattr(sibling, "lfs", None)
    if isinstance(lfs, dict):
        return lfs.get("size")
    return getattr(lfs, "size", None) if lfs else None


def crawl_hf_gpu_libs(
    repo_id: str,
    *,
    allowed_gpu_types: Iterable[str] = GPU_TYPES_DEFAULT,
) -> dict[str, dict[str, dict]]:
    """Return {gpu_type: {platform: {download_url, size_bytes, sha256}}}."""
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

    allowed = {t.lower() for t in allowed_gpu_types}
    gpu_libs: dict[str, dict[str, dict]] = {}
    for sib in info.siblings or []:
        filename = sib.rfilename
        match = _GGML_RE.match(filename)
        if not match:
            continue
        gpu_type = match.group(1).lower()
        ext = match.group(2).lower()
        if gpu_type not in allowed:
            log.info("skipping %s: gpu type %r not in allowed set", filename, gpu_type)
            continue
        platform = _PLATFORM_BY_EXT[ext]
        size = _hf_lfs_size(sib)
        sha = _hf_lfs_sha256(sib)
        if size is None or sha is None:
            log.warning("skipping %s: missing LFS size or sha256", filename)
            continue
        url = (
            f"https://huggingface.co/{repo_id}/resolve/main/{filename}"
        )
        gpu_libs.setdefault(gpu_type, {})[platform] = {
            "download_url": url,
            "size_bytes": int(size),
            "sha256": sha,
        }
    return gpu_libs


# ------------------------------------------------------------------ LLM summary


_LLM_SYSTEM = """\
You summarise a llamafile release note into a single-line description for a \
software registry entry. You always respond with a single JSON object, no \
prose, no markdown.
"""

_LLM_USER_TEMPLATE = """\
Below is the body of a GitHub release for the `llamafile` project. Produce a \
single short line listing the major user-visible features and supported \
backends/platforms introduced (or carried) by this release. Mirror the style \
of these existing examples:

- "Server, CLI, chat, combined mode, Metal+CUDA(linux) support"
- "Server, CLI, chat, combined mode, Metal+CUDA+Vulkan support on Linux+Windows"
- "Phi4 + Qwen3 Support, Localscore plaintext output"
- "DeepSeek Distil R1 Models, Gemma 3, IBM granite support"

Rules:
- No leading verb ("Adds", "Includes"). Just the feature list.
- Comma-separated. Roughly 8-15 words. Single sentence, no trailing period.
- Do NOT mention PR numbers, contributor handles, or internal refactors.
- Do NOT include "[release notes](...)" - the caller appends that.

Reply with exactly this JSON shape:
{{"description": "..."}}

Release version: {version}
Release notes body:
---
{body}
---
"""


def _llm_describe(*, version: str, body: str, llm: LlmClient, body_char_limit: int) -> str:
    truncated = body[:body_char_limit]
    user = _LLM_USER_TEMPLATE.format(
        version=version, body=truncated or "(release body is empty)"
    )
    reply = llm.chat_json(_LLM_SYSTEM, user)
    description = str(reply.get("description", "")).strip()
    # Strip surrounding quotes / trailing punctuation a model might still add.
    description = description.strip('"').strip("'").rstrip(".")
    return description


# ----------------------------------------------------------------------- compose


def crawl(
    *,
    version: str,
    hf_repo: str,
    llm: LlmClient | None,
    gh_repo: str = GH_DEFAULT_REPO,
    description_override: str | None = None,
    body_char_limit: int = 6000,
    allowed_gpu_types: Iterable[str] = GPU_TYPES_DEFAULT,
) -> dict:
    """Return a versions.json release entry for the given llamafile version."""
    gh = crawl_gh_release(gh_repo, version)
    gpu_libs = crawl_hf_gpu_libs(hf_repo, allowed_gpu_types=allowed_gpu_types)

    if description_override is not None:
        description_core = description_override.strip()
    elif llm is None:
        description_core = ""
    else:
        description_core = _llm_describe(
            version=version, body=gh.body, llm=llm, body_char_limit=body_char_limit,
        )
    if gh.html_url:
        description = f"{description_core} ([release notes]({gh.html_url}))"
    else:
        description = description_core

    entry: dict = {
        "version": version,
        "description": description,
        "release_date": gh.published_at,
        "download_url": gh.binary.url,
        "size_bytes": gh.binary.size,
        "sha256": gh.binary.sha256,
        "zipalign_url": gh.zipalign.url,
        "zipalign_size_bytes": gh.zipalign.size,
        "zipalign_sha256": gh.zipalign.sha256,
    }
    if gpu_libs:
        # Emit in a stable order: cuda, rocm, vulkan, then anything else.
        order = list(GPU_TYPES_DEFAULT) + sorted(
            k for k in gpu_libs if k not in GPU_TYPES_DEFAULT
        )
        ordered: dict[str, dict[str, dict]] = {}
        for key in order:
            if key in gpu_libs:
                # also stable-order platforms
                platforms = gpu_libs[key]
                ordered[key] = {
                    p: platforms[p] for p in ("linux", "windows") if p in platforms
                }
        entry["gpu_libs"] = ordered
    return entry

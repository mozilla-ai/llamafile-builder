"""Command-line entry point for version-crawl."""

from __future__ import annotations

import json
import logging
import os
import re
import sys
from pathlib import Path

import click

from .crawler import GH_DEFAULT_REPO, GPU_TYPES_DEFAULT, crawl
from .errors import VersionCrawlError
from .llm import LlmClient


_VERSION_FAMILY_RE = re.compile(r"^(\d+)\.(\d+)")


def _version_family(version: str) -> str | None:
    """Return the `MAJOR.MINOR` prefix of a version string, or None."""
    match = _VERSION_FAMILY_RE.match(version)
    if not match:
        return None
    return f"{match.group(1)}.{match.group(2)}"


@click.command()
@click.argument("version")
@click.argument("hf_repo")
@click.option(
    "--gh-repo",
    default=GH_DEFAULT_REPO,
    show_default=True,
    help="GitHub repo (owner/name) hosting the llamafile release.",
)
@click.option(
    "--input",
    "input_path",
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    default=None,
    help="Existing versions.json to merge into. The crawled entry is inserted "
    "at the top of the matching family group's 'releases' list (or replaces "
    "any entry with the same 'version'), and the full document is printed.",
)
@click.option(
    "--description",
    "description_override",
    default=None,
    help="One-line feature summary. Skips the LLM call. The '([release notes](...))' "
    "suffix is appended automatically.",
)
@click.option(
    "--llm-base-url",
    default=lambda: os.environ.get("VERSION_CRAWL_LLM_URL", "http://localhost:8080"),
    show_default="http://localhost:8080 (env: VERSION_CRAWL_LLM_URL)",
    help="Base URL of an OpenAI-compatible chat completions endpoint.",
)
@click.option(
    "--llm-model",
    default=lambda: os.environ.get("VERSION_CRAWL_LLM_MODEL", "local"),
    show_default="local (env: VERSION_CRAWL_LLM_MODEL)",
    help="Model name passed to the chat completions endpoint.",
)
@click.option(
    "--llm-api-key",
    default=lambda: os.environ.get("VERSION_CRAWL_LLM_API_KEY"),
    help="Optional API key for the LLM endpoint (env: VERSION_CRAWL_LLM_API_KEY).",
)
@click.option(
    "--gpu-type",
    "gpu_types",
    multiple=True,
    metavar="NAME",
    help=f"Allowed GPU backend (repeatable). Defaults to {', '.join(GPU_TYPES_DEFAULT)}.",
)
@click.option(
    "--body-chars",
    default=6000,
    show_default=True,
    help="Truncate the release body to this many characters before the LLM call.",
)
@click.option(
    "--target-group",
    default=None,
    help="When merging with --input, force insertion into the group whose 'name' "
    "matches this value. Defaults to picking the group whose existing releases "
    "share the same MAJOR.MINOR family, falling back to the first group.",
)
@click.option(
    "-v",
    "--verbose",
    count=True,
    help="Increase logging verbosity (-v for INFO, -vv for DEBUG).",
)
def main(
    version: str,
    hf_repo: str,
    gh_repo: str,
    input_path: Path | None,
    description_override: str | None,
    llm_base_url: str,
    llm_model: str,
    llm_api_key: str | None,
    gpu_types: tuple[str, ...],
    body_chars: int,
    target_group: str | None,
    verbose: int,
) -> None:
    """Crawl GitHub release VERSION and HF repo HF_REPO for a llamafile.

    VERSION is the GitHub release tag, e.g. '0.10.1'.
    HF_REPO is the Hugging Face repo holding the ggml-* GPU libraries,
    e.g. 'mozilla-ai/llamafile_0.10'.
    """
    level = logging.WARNING - 10 * min(verbose, 2)
    logging.basicConfig(
        level=level,
        format="%(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )

    llm: LlmClient | None = None
    if description_override is None:
        llm = LlmClient(base_url=llm_base_url, model=llm_model, api_key=llm_api_key)

    allowed_gpu_types = tuple(t.lower() for t in gpu_types) or GPU_TYPES_DEFAULT

    try:
        entry = crawl(
            version=version,
            hf_repo=hf_repo,
            llm=llm,
            gh_repo=gh_repo,
            description_override=description_override,
            body_char_limit=body_chars,
            allowed_gpu_types=allowed_gpu_types,
        )
    except VersionCrawlError as exc:
        raise click.ClickException(str(exc)) from exc

    if input_path is None:
        click.echo(json.dumps(entry, indent=2, ensure_ascii=False))
        return

    document = json.loads(input_path.read_text())
    if not isinstance(document, dict) or "data" not in document:
        raise click.ClickException(
            f"{input_path}: expected an object with a 'data' array"
        )
    groups = document.get("data") or []
    if not isinstance(groups, list):
        raise click.ClickException(f"{input_path}: 'data' must be an array")

    group = _pick_group(groups, version, target_group)
    if group is None:
        raise click.ClickException(
            "could not locate a matching group in --input. "
            "Pass --target-group <name> or add a group entry by hand first."
        )

    releases = group.setdefault("releases", [])
    if not isinstance(releases, list):
        raise click.ClickException(
            f"group {group.get('name')!r} has a non-array 'releases' field"
        )

    replaced = False
    for i, existing in enumerate(releases):
        if isinstance(existing, dict) and existing.get("version") == entry["version"]:
            releases[i] = entry
            replaced = True
            break
    if not replaced:
        releases.insert(0, entry)

    document["data"] = groups
    click.echo(json.dumps(document, indent=2, ensure_ascii=False))


def _pick_group(groups: list, version: str, target_group: str | None) -> dict | None:
    """Choose which group in versions.json should receive the new release.

    Strategy:
    1. If --target-group was passed, match by 'name'.
    2. Otherwise pick the first group whose existing releases share the
       MAJOR.MINOR family with `version`.
    3. Otherwise fall back to the first group.
    """
    if target_group is not None:
        for g in groups:
            if isinstance(g, dict) and g.get("name") == target_group:
                return g
        return None

    family = _version_family(version)
    if family is not None:
        for g in groups:
            if not isinstance(g, dict):
                continue
            for rel in g.get("releases") or []:
                if not isinstance(rel, dict):
                    continue
                if _version_family(rel.get("version", "")) == family:
                    return g

    for g in groups:
        if isinstance(g, dict):
            return g
    return None


if __name__ == "__main__":
    main()

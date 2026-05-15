"""Command-line entry point for hf-crawl."""

from __future__ import annotations

import json
import logging
import os
import sys
from pathlib import Path

import click

from .crawler import ALLOWED_QUANTS_DEFAULT, crawl
from .errors import HfCrawlError
from .llm import LlmClient

DEFAULT_CONFIG_VERSION = 1
DEFAULT_MIN_BUILDER_VERSION = "0.1.0"


@click.command()
@click.argument("repo_id")
@click.option(
    "--model-name",
    "model_name",
    default=None,
    help="Human-readable display name. Defaults to a humanised form of the repo id.",
)
@click.option(
    "--input",
    "input_path",
    type=click.Path(exists=True, dir_okay=False, path_type=Path),
    default=None,
    help="Existing models.json to merge into. The crawled entry is appended "
    "(or replaces an entry with the same id) and the full document is printed.",
)
@click.option(
    "--llm-base-url",
    default=lambda: os.environ.get("HF_CRAWL_LLM_URL", "http://localhost:8080"),
    show_default="http://localhost:8080 (env: HF_CRAWL_LLM_URL)",
    help="Base URL of an OpenAI-compatible chat completions endpoint.",
)
@click.option(
    "--llm-model",
    default=lambda: os.environ.get("HF_CRAWL_LLM_MODEL", "local"),
    show_default="local (env: HF_CRAWL_LLM_MODEL)",
    help="Model name passed to the chat completions endpoint.",
)
@click.option(
    "--llm-api-key",
    default=lambda: os.environ.get("HF_CRAWL_LLM_API_KEY"),
    help="Optional API key for the LLM endpoint (env: HF_CRAWL_LLM_API_KEY).",
)
@click.option(
    "--quant",
    "quants",
    multiple=True,
    metavar="NAME",
    help="Allowed quantization (repeatable). Defaults to "
    f"{', '.join(ALLOWED_QUANTS_DEFAULT)}.",
)
@click.option(
    "--readme-chars",
    default=8000,
    show_default=True,
    help="Truncate the README to this many characters before sending to the LLM.",
)
@click.option(
    "--sort",
    "sort_field",
    default=None,
    metavar="FIELD",
    help="Sort the output 'data' array by FIELD (e.g. name, created_at, "
    "param_count). Entries lacking the field are placed at the end.",
)
@click.option(
    "-r",
    "--reverse",
    is_flag=True,
    default=False,
    help="Reverse the --sort order (descending).",
)
@click.option(
    "-v",
    "--verbose",
    count=True,
    help="Increase logging verbosity (-v for INFO, -vv for DEBUG).",
)
def main(
    repo_id: str,
    model_name: str | None,
    input_path: Path | None,
    llm_base_url: str,
    llm_model: str,
    llm_api_key: str | None,
    quants: tuple[str, ...],
    readme_chars: int,
    sort_field: str | None,
    reverse: bool,
    verbose: int,
) -> None:
    """Crawl Hugging Face REPO_ID and emit a models.json entry on stdout.

    REPO_ID has the form 'organization/model_name', e.g. 'Qwen/Qwen3-0.6B-GGUF'.
    """
    level = logging.WARNING - 10 * min(verbose, 2)
    logging.basicConfig(
        level=level,
        format="%(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )

    allowed_quants = tuple(q.upper() for q in quants) or ALLOWED_QUANTS_DEFAULT

    llm = LlmClient(base_url=llm_base_url, model=llm_model, api_key=llm_api_key)
    try:
        entry = crawl(
            repo_id=repo_id,
            display_name=model_name,
            llm=llm,
            allowed_quants=allowed_quants,
            readme_char_limit=readme_chars,
        )
    except HfCrawlError as exc:
        raise click.ClickException(str(exc)) from exc

    if input_path is not None:
        document = json.loads(input_path.read_text())
        if not isinstance(document, dict) or "data" not in document:
            raise click.ClickException(
                f"{input_path}: expected an object with a 'data' array"
            )
        data = document.get("data") or []
    else:
        document = {
            "config_version": DEFAULT_CONFIG_VERSION,
            "min_builder_version": DEFAULT_MIN_BUILDER_VERSION,
            "data": [],
        }
        data = document["data"]

    replaced = False
    for i, existing in enumerate(data):
        if isinstance(existing, dict) and existing.get("id") == entry["id"]:
            data[i] = entry
            replaced = True
            break
    if not replaced:
        data.append(entry)

    if sort_field is not None:
        data = _sorted_entries(data, sort_field, reverse=reverse)

    document["data"] = data
    click.echo(json.dumps(document, indent=2, ensure_ascii=False))


def _sorted_entries(entries: list, field: str, *, reverse: bool) -> list:
    """Sort entries by `field`. Entries missing the field stay at the end.

    Raises a ClickException if entries hold mixed types for the field, which
    Python's sort would otherwise surface as a TypeError mid-stream.
    """
    present = [e for e in entries if isinstance(e, dict) and field in e]
    missing = [e for e in entries if not (isinstance(e, dict) and field in e)]

    def sort_key(entry):
        value = entry[field]
        return value.casefold() if isinstance(value, str) else value

    try:
        present.sort(key=sort_key, reverse=reverse)
    except TypeError as exc:
        raise click.ClickException(
            f"cannot sort by {field!r}: entries hold incomparable types ({exc})"
        ) from exc
    return present + missing


if __name__ == "__main__":
    main()

"""Domain exceptions raised by hf-crawl.

The CLI catches these and converts them to ``click.ClickException`` so the
user sees a short, actionable error instead of a Python traceback.
"""


class HfCrawlError(Exception):
    """Base class for expected hf-crawl failures."""


class LlmUnavailableError(HfCrawlError):
    """The LLM endpoint could not be reached or returned an unusable reply."""


class HfRepoError(HfCrawlError):
    """The Hugging Face repo could not be inspected (missing, gated, etc.)."""

"""Domain exceptions raised by version-crawl.

The CLI catches these and converts them to ``click.ClickException`` so the
user sees a short, actionable error instead of a Python traceback.
"""


class VersionCrawlError(Exception):
    """Base class for expected version-crawl failures."""


class GhReleaseError(VersionCrawlError):
    """A GitHub release could not be fetched or is missing required assets."""


class HfRepoError(VersionCrawlError):
    """The Hugging Face repo could not be inspected (missing, gated, etc.)."""


class LlmUnavailableError(VersionCrawlError):
    """The LLM endpoint could not be reached or returned an unusable reply."""

"""OpenAI-compatible chat completion client used for release-note summarisation."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

import httpx

from .errors import LlmUnavailableError


@dataclass(frozen=True)
class LlmClient:
    base_url: str
    model: str
    api_key: str | None = None
    timeout: float = 120.0

    def chat_json(self, system: str, user: str) -> dict:
        """Call /v1/chat/completions and parse the assistant reply as JSON.

        Raises :class:`LlmUnavailableError` for any transport, HTTP-status, or
        parsing failure so the CLI can render a clean error message.
        """
        headers = {"Content-Type": "application/json"}
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        payload = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.0,
            "response_format": {"type": "json_object"},
        }
        url = self.base_url.rstrip("/") + "/v1/chat/completions"

        try:
            resp = httpx.post(url, headers=headers, json=payload, timeout=self.timeout)
        except httpx.ConnectError as exc:
            raise LlmUnavailableError(
                f"could not connect to LLM endpoint at {self.base_url} "
                f"(is the server running?): {exc}"
            ) from exc
        except httpx.TimeoutException as exc:
            raise LlmUnavailableError(
                f"LLM endpoint at {self.base_url} timed out after {self.timeout}s"
            ) from exc
        except httpx.HTTPError as exc:
            raise LlmUnavailableError(
                f"LLM endpoint at {self.base_url} unreachable: {exc}"
            ) from exc

        if resp.status_code >= 400:
            body = (resp.text or "")[:300].replace("\n", " ")
            raise LlmUnavailableError(
                f"LLM endpoint returned HTTP {resp.status_code} for "
                f"{url}: {body}"
            )

        try:
            data = resp.json()
            content = data["choices"][0]["message"]["content"]
        except (ValueError, KeyError, IndexError, TypeError) as exc:
            raise LlmUnavailableError(
                f"LLM endpoint returned an unexpected response shape: {exc}"
            ) from exc

        try:
            return _parse_json_block(content)
        except (ValueError, json.JSONDecodeError) as exc:
            snippet = (content or "")[:300].replace("\n", " ")
            raise LlmUnavailableError(
                f"LLM reply was not valid JSON ({exc}): {snippet!r}"
            ) from exc


def _parse_json_block(text: str) -> dict:
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    match = re.search(r"\{.*\}", text, re.DOTALL)
    if not match:
        raise ValueError("no JSON object found in reply")
    return json.loads(match.group(0))

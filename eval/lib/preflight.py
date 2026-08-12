"""LLM endpoint pre-flight check for the eval runner.

Probes the agent-under-test's LLM backend BEFORE running cases, so a
misconfigured endpoint (dead server / wrong port / doubly-pathed /v1 /
non-JSON proxy) fails fast with a clear message instead of silently
failing every case and looking like a model-capability regression.

This is the cure for the 2026-08-12 scar: ``AGENT_LLM_ENDPOINT`` carrying
``/v1`` made llama.cpp append its own ``/v1`` → ``/v1/v1/...`` → 404 → every
case's LLM call failed → 18/30 fake regressions, 12min wasted.

Run tests: ``python3 eval/lib/test_preflight.py``.
"""
from __future__ import annotations

import requests


def _probe_url(backend_type: str, endpoint: str) -> str:
    """Return the models-list URL for the backend.

    - ollama: native ``/api/tags``. The backend tolerates a trailing ``/v1``
      in the endpoint, but ``/api/tags`` does not live under ``/v1``, so strip
      it before probing.
    - llamacpp / openai / cloud (OpenAI-compatible): ``{endpoint}/v1/models``.
      The backend appends ``/v1`` itself, so ``AGENT_LLM_ENDPOINT`` must be a
      base URL without ``/v1`` — a doubly-pathed endpoint 404s here, which is
      exactly the misconfig we want to catch.
    """
    endpoint = endpoint.rstrip("/")
    if backend_type == "ollama":
        if endpoint.endswith("/v1"):
            endpoint = endpoint[: -len("/v1")]
        return f"{endpoint}/api/tags"
    return f"{endpoint}/v1/models"


def _extract_models(backend_type: str, payload) -> list:
    if backend_type == "ollama":
        return [m.get("name", "") for m in (payload.get("models") or [])]
    # OpenAI-compatible (llamacpp / openai / cloud): {data: [{id: ...}]}
    return [m.get("id", "") for m in (payload.get("data") or [])]


def probe_llm_endpoint(backend_type, endpoint, model, api_key=None, timeout=5):
    """Probe the LLM endpoint. Returns ``(ok, message)``.

    - ``ok=False`` → endpoint is unusable; the caller MUST abort (fail-fast).
    - ``ok=True``  → endpoint is reachable + parseable; safe to run cases.
      Model-match is *advisory*: a name mismatch warns but does not abort,
      because ``/v1/models`` id formats vary across servers and we refuse to
      introduce a new false-block on top of an uncertain signal.
    """
    url = _probe_url(backend_type, endpoint)
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    try:
        resp = requests.get(url, headers=headers, timeout=timeout)
    except requests.RequestException as e:
        return False, (
            f"cannot reach LLM endpoint {url} ({e.__class__.__name__}: {e}). "
            f"Is the server running and host/port correct?"
        )
    if resp.status_code != 200:
        hint = ""
        if resp.status_code == 404 and backend_type != "ollama":
            hint = (
                " 404 at /v1/models usually means AGENT_LLM_ENDPOINT includes "
                "'/v1' — the backend appends /v1 itself; give a base URL without it."
            )
        return False, f"LLM endpoint {url} returned HTTP {resp.status_code}.{hint}"
    try:
        payload = resp.json()
    except ValueError:
        snippet = (resp.text or "")[:120]
        return False, (
            f"LLM endpoint {url} returned non-JSON (HTTP 200). snippet: {snippet!r}"
        )
    models = _extract_models(backend_type, payload)
    if model in models:
        return True, f"LLM endpoint OK: model {model!r} listed at {url}."
    return True, (
        f"LLM endpoint reachable at {url} but model {model!r} not in reported "
        f"list {models}; proceeding (id formats vary across servers)."
    )

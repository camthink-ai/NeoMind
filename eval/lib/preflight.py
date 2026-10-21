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


def _probe_urls(backend_type: str, endpoint: str) -> list[str]:
    """Return candidate models-list URLs, most likely first.

    - ollama: native ``/api/tags``. The backend tolerates a trailing ``/v1``
      in the endpoint, but ``/api/tags`` does not live under ``/v1``, so strip
      it before probing.
    - llamacpp / openai / cloud (OpenAI-compatible): the runtime passes the
      instance endpoint STRAIGHT THROUGH as its base URL and appends only
      ``/chat/completions`` — so the working convention is an endpoint that
      already includes ``/v1`` (default OpenAI URLs do too). Older builds
      appended ``/v1`` themselves, which is why the 2026-08-12 scar saw a
      doubly-pathed URL. Both conventions are probed so neither false-blocks;
      a genuinely dead or doubly-pathed endpoint still fails every candidate.
    """
    endpoint = endpoint.rstrip("/")
    if backend_type == "ollama":
        if endpoint.endswith("/v1"):
            endpoint = endpoint[: -len("/v1")]
        return [f"{endpoint}/api/tags"]
    urls = [f"{endpoint}/models"]
    if not endpoint.endswith("/v1"):
        urls.append(f"{endpoint}/v1/models")
    return urls


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
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    last_error = ""
    payload = None
    url = ""
    for candidate in _probe_urls(backend_type, endpoint):
        url = candidate
        try:
            resp = requests.get(url, headers=headers, timeout=timeout)
        except requests.RequestException as e:
            last_error = (
                f"cannot reach LLM endpoint {url} ({e.__class__.__name__}: {e}). "
                f"Is the server running and host/port correct?"
            )
            continue
        if resp.status_code != 200:
            hint = ""
            if resp.status_code == 404 and backend_type != "ollama":
                hint = (
                    " 404 on the models list usually means the endpoint is "
                    "misconfigured — check that it includes the API prefix "
                    "(`/v1` for OpenAI-compatible servers) and that the server "
                    "exposes `/models`."
                )
            last_error = f"LLM endpoint {url} returned HTTP {resp.status_code}.{hint}"
            continue
        try:
            payload = resp.json()
        except ValueError:
            snippet = (resp.text or "")[:120]
            last_error = (
                f"LLM endpoint {url} returned non-JSON (HTTP 200). "
                f"snippet: {snippet!r}"
            )
            continue
        break
    if payload is None:
        return False, last_error or "LLM endpoint could not be probed."
    models = _extract_models(backend_type, payload)
    if model in models:
        return True, f"LLM endpoint OK: model {model!r} listed at {url}."
    return True, (
        f"LLM endpoint reachable at {url} but model {model!r} not in reported "
        f"list {models}; proceeding (id formats vary across servers)."
    )

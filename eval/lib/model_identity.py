"""Served-model identity check for eval runs.

A run against the wrong model is worse than a failed run: it produces
plausible numbers that invalidate every conclusion drawn from them
(2026-10-21: an A/B silently ran on lfm25-2.6b because the endpoint on
29375 had been swapped, and the only visible hint was max_ctx=128000 in
the logs).

`check_served_model()` asks the server what it is actually serving:
  - llama.cpp: GET {base}/props -> model_path (base = endpoint minus /v1)
  - OpenAI-compatible: GET {base}/v1/models (or {base}/models) -> ids

It returns:
  - ok:       the requested model name is contained in the served identity
  - mismatch: the server reports a DIFFERENT model -> callers must refuse
  - unknown:  the server exposes neither /props nor /models -> warn only
              (proxies may hide the identity; unknown is not proof of error)

Run tests: `.venv/bin/python eval/lib/test_model_identity.py`
"""
from __future__ import annotations

import json
import re

import requests


def _base(endpoint: str) -> str:
    e = endpoint.strip().rstrip("/")
    return e[: -len("/v1")] if e.endswith("/v1") else e


def _norm(s: str) -> str:
    """Lowercase alphanumerics only: `minicpm5-2b-q4_k_m.gguf` -> `minicpm52bq4km`."""
    return re.sub(r"[^a-z0-9]", "", s.lower())


def probe_identity(backend_type: str, endpoint: str, api_key: str | None = None,
                   timeout: float = 5.0) -> dict:
    """Return {identity: str|None, source: str} for the served model."""
    base = _base(endpoint)
    headers = {"Authorization": f"Bearer {api_key}"} if api_key else {}
    try:
        r = requests.get(f"{base}/props", headers=headers, timeout=timeout)
        if r.status_code == 200:
            d = r.json()
            ident = d.get("model_path") or d.get("model_alias") or ""
            if ident:
                return {"identity": ident, "source": f"{base}/props"}
    except (requests.RequestException, ValueError):
        pass

    for url in (f"{base}/v1/models", f"{base}/models"):
        try:
            r = requests.get(url, headers=headers, timeout=timeout)
            if r.status_code != 200:
                continue
            d = r.json()
            ids = [m.get("id", "") for m in (d.get("data") or []) if m.get("id")]
            if ids:
                return {"identity": ",".join(ids), "source": url}
        except (requests.RequestException, ValueError):
            continue
    return {"identity": None, "source": "no /props or /models identity"}


def check_served_model(backend_type: str, endpoint: str, api_key: str | None,
                       requested: str) -> tuple[str, str]:
    """Return (status, message) with status in {ok, mismatch, unknown}."""
    info = probe_identity(backend_type, endpoint, api_key)
    ident = info.get("identity")
    if not ident:
        return (
            "unknown",
            f"could not determine the served model ({info.get('source')}); "
            f"requested {requested!r} — proceeding (unknown is not a mismatch)",
        )
    if _norm(requested) and _norm(requested) in _norm(ident):
        return (
            "ok",
            f"served model matches {requested!r} ({info['source']}: {ident})",
        )
    return (
        "mismatch",
        f"served model does NOT match AGENT_LLM_MODEL={requested!r} — "
        f"server reports {ident!r} ({info['source']}). Refusing to run: "
        f"results from a different model are invalid.",
    )


def record_identity(run_dir, backend_type: str, endpoint: str, api_key: str | None,
                    requested: str) -> dict:
    """Write the served-model provenance into the run dir (best effort)."""
    info = probe_identity(backend_type, endpoint, api_key)
    rec = {"requested": requested, "served_identity": info.get("identity"),
           "source": info.get("source"), "endpoint": endpoint}
    try:
        from pathlib import Path
        (Path(run_dir) / "served_model.json").write_text(
            json.dumps(rec, ensure_ascii=False, indent=1) + "\n"
        )
    except Exception:  # noqa: BLE001 — provenance must never block a run
        pass
    return rec

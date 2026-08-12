"""Tests for preflight.py — run: `python3 eval/lib/test_preflight.py`.

Monkeypatches preflight.requests.get (no live server) to verify the
fail-fast decision logic for the LLM-endpoint misconfig classes that
previously wasted whole eval runs looking like model-capability regressions:
dead server / wrong port (conn refused), doubly-pathed /v1 → 404 (the
2026-08-12 scar: 18/30 fake regressions, 12min), non-JSON 200, and the
advisory (non-aborting) model-not-listed case.
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import preflight as pf  # noqa: E402
import requests as _real_requests  # noqa: E402


class FakeResp:
    def __init__(self, status=200, body=None, text=""):
        self.status_code = status
        self._body = body  # None → json() raises (simulates non-JSON)
        self.text = text

    def json(self):
        if self._body is None:
            raise ValueError("not json")
        return self._body


class _Patch:
    """Temporarily replace pf.requests.get. handler(url) -> FakeResp, or None
    to simulate a connection error."""

    def __init__(self, handler):
        self.handler = handler
        self._saved = None

    def __enter__(self):
        self._saved = pf.requests.get

        def fake(url, headers=None, timeout=None):
            r = self.handler(url)
            if r is None:
                raise _real_requests.ConnectionError(
                    f"simulated conn refused: {url}")
            return r

        pf.requests.get = fake
        return self

    def __exit__(self, *exc):
        pf.requests.get = self._saved


def test_llamacpp_ok_model_listed():
    with _Patch(lambda u: FakeResp(200, {"data": [{"id": "qwen3.5:4b"}]})):
        ok, msg = pf.probe_llm_endpoint("llamacpp", "http://h:8080",
                                        "qwen3.5:4b")
    assert ok is True, msg
    assert "listed" in msg


def test_llamacpp_double_v1_returns_404_fail_fast():
    # The 2026-08-12 scar: AGENT_LLM_ENDPOINT=http://h:8080/v1 → the backend
    # appends /v1 itself → /v1/v1/models → 404 → every case's LLM call fails.
    with _Patch(lambda u: FakeResp(404)):
        ok, msg = pf.probe_llm_endpoint("llamacpp", "http://h:8080/v1",
                                        "qwen3.5:4b")
    assert ok is False, msg
    assert "404" in msg
    assert "/v1" in msg  # the base-url hint must surface


def test_connection_refused_fail_fast():
    with _Patch(lambda u: None):  # simulate conn refused / dead server
        ok, msg = pf.probe_llm_endpoint("llamacpp", "http://dead:9999", "x")
    assert ok is False, msg
    assert "cannot reach" in msg


def test_ollama_ok_and_strips_v1():
    # ollama tolerates /v1 in the backend config; /api/tags does NOT live
    # under /v1, so the probe must strip it.
    seen = {}

    def handler(u):
        seen["url"] = u
        return FakeResp(200, {"models": [{"name": "qwen3:8b"}]})

    with _Patch(handler):
        ok, msg = pf.probe_llm_endpoint("ollama", "http://h:11434/v1",
                                        "qwen3:8b")
    assert ok is True, msg
    assert seen["url"] == "http://h:11434/api/tags"  # /v1 stripped


def test_model_not_listed_is_advisory_not_abort():
    # Reachable + parseable → proceed. A model-name mismatch warns but does
    # NOT hard-fail (id formats vary across servers; don't introduce a new
    # false-block).
    with _Patch(lambda u: FakeResp(200, {"data": [{"id": "gemma:4b"}]})):
        ok, msg = pf.probe_llm_endpoint("llamacpp", "http://h:8080",
                                        "qwen3.5:4b")
    assert ok is True, msg
    assert "not in" in msg  # warning surfaces


def test_non_json_200_fail_fast():
    # A 200 HTML page (e.g. a proxy in front) is not a usable models endpoint.
    with _Patch(lambda u: FakeResp(200, text="<html>nginx</html>")):
        ok, msg = pf.probe_llm_endpoint("llamacpp", "http://h:8080", "x")
    assert ok is False, msg
    assert "non-JSON" in msg


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"  PASS  {fn.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"  FAIL  {fn.__name__}: {e!r}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"  ERROR {fn.__name__}: {e!r}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(_run_all())

"""Tests for model_identity.py — `.venv/bin/python eval/lib/test_model_identity.py`."""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import model_identity as mi  # noqa: E402
import requests as _real_requests  # noqa: E402


class FakeResp:
    def __init__(self, status=200, body=None):
        self.status_code = status
        self._body = body

    def json(self):
        if self._body is None:
            raise ValueError("not json")
        return self._body


class _Patch:
    def __init__(self, handler):
        self.handler = handler

    def __enter__(self):
        self._saved = mi.requests.get

        def fake(url, headers=None, timeout=None):
            r = self.handler(url)
            if r is None:
                raise _real_requests.ConnectionError(url)
            return r
        mi.requests.get = fake
        return self

    def __exit__(self, *exc):
        mi.requests.get = self._saved


def test_props_identity_matches_requested_model():
    def h(u):
        if u.endswith("/props"):
            return FakeResp(200, {"model_path": "/m/minicpm5-2b/minicpm5-2b-q4_k_m.gguf"})
        return FakeResp(404)
    with _Patch(h):
        st, msg = mi.check_served_model("openai", "http://h:29375/v1", "k", "minicpm5-2b")
    assert st == "ok", msg


def test_props_identity_mismatch_is_fatal():
    # The exact 2026-10-21 failure: endpoint serves lfm25 while the run asks
    # for minicpm5-2b.
    def h(u):
        if u.endswith("/props"):
            return FakeResp(200, {"model_path": "/tmp/shots/models/lfm25-2.6b/lfm25-2.6b-qad_q4_0.gguf"})
        return FakeResp(404)
    with _Patch(h):
        st, msg = mi.check_served_model("openai", "http://h:29375/v1", "k", "minicpm5-2b")
    assert st == "mismatch", msg
    assert "lfm25" in msg, msg


def test_models_endpoint_is_used_when_props_missing():
    def h(u):
        if u.endswith("/props"):
            return FakeResp(404)
        if u.endswith("/v1/models"):
            return FakeResp(200, {"data": [{"id": "minicpm5-2b"}]})
        return FakeResp(404)
    with _Patch(h):
        st, msg = mi.check_served_model("openai", "http://h:8080", "k", "minicpm5-2b")
    assert st == "ok", msg


def test_unknown_identity_is_advisory_not_fatal():
    with _Patch(lambda u: FakeResp(404)):
        st, msg = mi.check_served_model("openai", "http://h:8080", None, "x")
    assert st == "unknown", msg
    assert "not a mismatch" in msg, msg


def test_base_strips_v1_for_props():
    assert mi._base("http://h:29375/v1") == "http://h:29375"
    assert mi._base("http://h:29375/") == "http://h:29375"


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

"""Tests for seed.py — run: `python3 eval/lib/test_seed.py`.

Verifies the template-seeding-race retry in _seed_devices (no live server):
races are retried; real errors raise immediately.
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import seed  # noqa: E402


class FakeResp:
    def __init__(self, ok=True, status=200, text=""):
        self.ok = ok
        self.status_code = status
        self.text = text


class FakeServer:
    def __init__(self, responses):
        self.responses = list(responses)
        self.calls = 0

    def post(self, path, body):
        self.calls += 1
        if self.responses:
            return self.responses.pop(0)
        return FakeResp(True, 200, "")


class _NoSleep:
    """Null out seed.time.sleep so retry tests don't wait real time."""

    def __enter__(self):
        self._saved = seed.time.sleep
        seed.time.sleep = lambda *a, **k: None
        return self

    def __exit__(self, *exc):
        seed.time.sleep = self._saved


def test_is_template_race_signature():
    assert seed._is_template_race(
        "Device type template 'ne101_camera' not found") is True
    assert seed._is_template_race("invalid device id") is False
    assert seed._is_template_race("") is False


def test_template_race_retried_then_succeeds():
    with _NoSleep():
        srv = FakeServer([
            FakeResp(False, 500, "Device type template 'ne101_camera' not found"),
            FakeResp(False, 500, "...template... not found..."),
            FakeResp(True, 200, ""),
        ])
        seed._seed_devices(srv, [{"device_id": "x", "device_type": "ne101_camera"}])
    assert srv.calls == 3, f"expected 3 attempts, got {srv.calls}"


def test_non_template_error_raises_immediately():
    srv = FakeServer([FakeResp(False, 400, "invalid device id")])
    raised = False
    try:
        seed._seed_devices(srv, [{"device_id": "x"}])
    except RuntimeError as e:
        raised = True
        assert "400" in str(e)
    assert raised and srv.calls == 1, "real errors must not retry"


def test_success_first_try():
    srv = FakeServer([FakeResp(True, 200, "")])
    seed._seed_devices(srv, [{"device_id": "x"}])
    assert srv.calls == 1


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

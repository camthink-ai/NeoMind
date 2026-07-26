"""Tests for state_query.py new types — run: `python3 eval/lib/test_state_query.py`.

Monkeypatches `requests.get` (no live server needed) to verify dispatch +
assertion logic for the types added for zero-coverage categories
(extension/widget/llm/settings). Live-server correctness is verified when a
real eval run hits these endpoints.
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import state_query as sq  # noqa: E402


class FakeResp:
    def __init__(self, status=200, body=None):
        self.status_code = status
        self._body = body if body is not None else {}

    def json(self):
        return self._body


class _Patch:
    """Temporarily replace sq.requests.get with a path-matching fake."""

    def __init__(self, mapping):
        self.mapping = mapping  # url-suffix -> FakeResp
        self._saved = None

    def __enter__(self):
        self._saved = sq.requests.get

        def fake(url, headers=None, timeout=None):
            for suf, r in self.mapping.items():
                if url.endswith(suf):
                    return r
            return FakeResp(404, {"success": False, "error": "not found"})

        sq.requests.get = fake
        return self

    def __exit__(self, *exc):
        sq.requests.get = self._saved


def test_extension_installed_found_by_id():
    with _Patch({"/extensions/yolo": FakeResp(200, {"success": True})}):
        r = sq.run_query(
            {"type": "extension_installed", "params": {"id": "yolo"},
             "expected": True}, "http://b", "k")
    assert r["actual"] is True and r["passed"] is True


def test_extension_installed_missing():
    with _Patch({"/extensions/nope": FakeResp(404, {}), "/extensions": FakeResp(200, {"data": []})}):
        r = sq.run_query(
            {"type": "extension_installed", "params": {"id": "nope"},
             "expected": True}, "http://b", "k")
    assert r["passed"] is False


def test_settings_value_with_field():
    body = {"success": True, "data": {"retention_days": 30, "auto_cleanup": True}}
    with _Patch({"/settings/retention": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "settings_value", "params": {"key": "retention", "field": "retention_days"},
             "expected": 30}, "http://b", "k")
    assert r["actual"] == 30 and r["passed"] is True


def test_settings_value_whole_object():
    body = {"success": True, "data": {"tz": "UTC"}}
    with _Patch({"/settings/timezone": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "settings_value", "params": {"key": "timezone"},
             "expected": {"tz": "UTC"}}, "http://b", "k")
    assert r["actual"] == {"tz": "UTC"} and r["passed"] is True


def test_widget_and_llm_backend_dispatch():
    # Same _id_or_name_exists path; just verify they route + assert.
    with _Patch({"/frontend-components/w1": FakeResp(200, {"success": True}),
                 "/llm-backends/glm": FakeResp(200, {"success": True})}):
        rw = sq.run_query(
            {"type": "widget_exists", "params": {"id": "w1"}, "expected": True},
            "http://b", "k")
        rl = sq.run_query(
            {"type": "llm_backend_exists", "params": {"id": "glm"}, "expected": True},
            "http://b", "k")
    assert rw["passed"] is True and rl["passed"] is True


def test_device_type_has_metric_present():
    # Normalized name match: asserted 'mystery-vibration-sensor' matches an
    # auto-generated device_type 'mystery_wibration_sensor' / name with spaces.
    body = {"success": True, "data": {"device_types": [
        {"device_type": "mystery_wibration_sensor", "name": "Mystery Vibration Sensor",
         "metrics": [{"name": "rpm"}, {"name": "vibration_mm_s"}]}]}}
    with _Patch({"/device-types": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "device_type_has_metric",
             "params": {"id": "mystery-vibration-sensor", "metric": "rpm"},
             "expected": True}, "http://b", "k")
    assert r["actual"] is True and r["passed"] is True


def test_device_type_has_metric_absent():
    body = {"success": True, "data": {"device_types": [
        {"device_type": "x", "name": "X", "metrics": [{"name": "rpm"}]}]}}
    with _Patch({"/device-types": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "device_type_has_metric",
             "params": {"id": "x", "metric": "temp"}, "expected": True},
            "http://b", "k")
    assert r["passed"] is False


def test_device_command_sent_found():
    body = {"success": True, "data": [
        {"command_name": "set_speed", "status": "success"},
        {"command_name": "stop", "status": "failed"}]}
    with _Patch({"/devices/pump-A/commands": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "device_command_sent",
             "params": {"id": "pump-A", "command": "set_speed"}, "expected": True},
            "http://b", "k")
    assert r["actual"] is True and r["passed"] is True


def test_device_command_sent_absent():
    body = {"success": True, "data": [{"command_name": "stop"}]}
    with _Patch({"/devices/pump-A/commands": FakeResp(200, body)}):
        r = sq.run_query(
            {"type": "device_command_sent",
             "params": {"id": "pump-A", "command": "set_speed"}, "expected": True},
            "http://b", "k")
    assert r["passed"] is False


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

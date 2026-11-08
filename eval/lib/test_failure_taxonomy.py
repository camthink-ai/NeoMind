"""Tests for the failure taxonomy (classify_failure / tier_of).

Run standalone: `python3 eval/lib/test_failure_taxonomy.py`.

Each bucket gets one synthetic execution; the assertions pin the PRIORITY
ORDER as much as the classification (e.g. wrong_tool beats missing_tool when
both signals fire, rejected_args requires the rejected call to be in an
expected family, passed/unasserted executions return None).
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import hard_signal as hs  # noqa: E402


def _shell_call(command: str, exit_code: int = 0) -> dict:
    return {"name": "shell", "arguments": {"command": command},
            "result": {"exit_code": exit_code, "stdout": "", "stderr": ""}}


def _turn(*tool_calls) -> dict:
    return {"user": "", "assistant_message": "", "tool_calls": list(tool_calls),
            "processing_time_ms": 1000}


def _record(turns=None, status=None, sqs=None) -> dict:
    return {"case_id": "x", "lang": "en", "turn_records": turns or [],
            "state_queries": sqs or [], "status": status}


_DEVICE_CASE = {
    "id": "device-create-temp-sensor", "lang": "en",
    "expect": {"tools": ["shell"], "commands": ["neomind device create"]},
}


# ------------------------------------------------------------------- tiers ----

def test_tier_of_matches_official_report_cut():
    # The 20 multi ids of the 2026-10-29 gate (prefix families).
    multi = ("agent-cron-schedule", "agent-full-lifecycle", "api-key-lifecycle",
             "deep-memory-transform", "extension-market-install",
             "guided-mqtt-onboarding", "haccp-compliance-deploy",
             "multi-turn-correction", "multi-turn-threshold-change")
    for cid in multi:
        assert hs.tier_of(cid) == "multi", cid
    simple = ("device-create-temp-sensor", "rule-threshold-notify",
              "dashboard-add-widget", "settings-set-retention",
              "security-injection-device-name", "message-webhook-channel")
    for cid in simple:
        assert hs.tier_of(cid) == "simple", cid
    assert hs.tier_of("") == "simple"
    assert hs.tier_of(None) == "simple"


# ------------------------------------------------------------- runtime buckets

def test_timeout_bucket():
    mode = hs.classify_failure(_DEVICE_CASE, _record(status="timeout"))
    assert mode == "timeout"


def test_agent_error_bucket():
    mode = hs.classify_failure(
        _DEVICE_CASE, _record(status="agent_error"))
    assert mode == "agent_error"


def test_harness_error_bucket():
    for status in ("seed_failure", "llm_config_error", "schema_error"):
        mode = hs.classify_failure(_DEVICE_CASE, _record(status=status))
        assert mode == "harness_error", status


# -------------------------------------------------------- competence buckets

def test_wrong_tool_beats_missing_tool():
    # Model grabbed file_write AND never called shell — wrong_tool is the
    # more diagnostic signal and must win.
    rec = _record(turns=[_turn({"name": "file_write", "arguments": {},
                                "result": "ok"})])
    mode = hs.classify_failure(_DEVICE_CASE, rec)
    assert mode == "wrong_tool"


def test_rejected_args_bucket():
    # Typed the right family, CLI refused the flags (clap exit 2).
    rec = _record(turns=[_turn(
        _shell_call("neomind device create --id s1 --enabled true", exit_code=2))])
    mode = hs.classify_failure(_DEVICE_CASE, rec)
    assert mode == "rejected_args"


def test_rejected_unrelated_command_is_missing_tool():
    # The rejected call is NOT in the expected family — the family was simply
    # never attempted successfully.
    rec = _record(turns=[_turn(
        _shell_call("neomind widget list --json", exit_code=2))])
    mode = hs.classify_failure(_DEVICE_CASE, rec)
    assert mode == "missing_tool"


def test_missing_tool_bucket():
    # Silence — nothing called at all.
    rec = _record(turns=[_turn()])
    mode = hs.classify_failure(_DEVICE_CASE, rec)
    assert mode == "missing_tool"


def test_wrong_args_bucket():
    case = {
        "id": "settings-retention", "lang": "en",
        "expect": {
            "tools": ["shell"],
            "commands": ["neomind settings set-retention"],
            "substrings": {"neomind settings set-retention":
                           ["--interval-hours", "168"]},
        },
    }
    # Right family, exit 0, wrong value.
    rec = _record(turns=[_turn(_shell_call(
        "neomind settings set-retention --interval-hours 24"))])
    mode = hs.classify_failure(case, rec)
    assert mode == "wrong_args"


def test_state_mismatch_bucket():
    # Commands all matched; the asserted end state still did not hold.
    rec = _record(
        turns=[_turn(_shell_call("neomind device create --id s1"))],
        sqs=[{"type": "device_exists", "params": {"id": "s1"},
              "expected": True, "passed": False}])
    mode = hs.classify_failure(_DEVICE_CASE, rec)
    assert mode == "state_mismatch"


def test_sq_only_failure_is_state_mismatch():
    # No tool expectation at all (state-query-only case): the commands axis is
    # silent, so the only thing that can have failed is the end state.
    case = {"id": "x", "lang": "en",
            "state_queries": [{"type": "device_count", "params": {},
                               "expected": 1}]}
    rec = _record(sqs=[{"passed": False}])
    mode = hs.classify_failure(case, rec)
    assert mode == "state_mismatch"


# ------------------------------------------------------------ non-failures ----

def test_pass_returns_none():
    rec = _record(
        turns=[_turn(_shell_call("neomind device create --id s1"))],
        sqs=[{"type": "device_exists", "params": {"id": "s1"},
              "expected": True, "passed": True}])
    assert hs.classify_failure(_DEVICE_CASE, rec) is None


def test_unasserted_returns_none():
    case = {"id": "x", "lang": "en"}
    assert hs.classify_failure(case, _record()) is None


def test_hard_supplied_skips_compute():
    # Replay path: archived scores.jsonl rows carry `hard`; classification
    # must not need the case definition.
    hard = hs.compute(_DEVICE_CASE, _record(status="timeout"))
    assert hs.classify_failure({}, _record(status="timeout"), hard=hard) == "timeout"


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

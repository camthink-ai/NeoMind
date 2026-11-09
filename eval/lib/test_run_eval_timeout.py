"""Tests for the 2026-11-09 timeout-path contract in run_eval.py.

Run standalone: `python3 eval/lib/test_run_eval_timeout.py`.

Contract under review (2026-11-09):
  - the cap-timeout record PRESERVES already-captured turn records
  - final status is "timeout"
  - timed_out is registered exactly once across return-path/exception-path
  - failure_reasons precedence is stable when multiple markers co-occur
"""
from __future__ import annotations

import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import run_eval  # noqa: E402 — inserts eval/lib itself

sys.path.insert(0, os.path.dirname(__file__))
import hard_signal as hs  # noqa: E402


def test_timeout_record_preserves_turns_status_and_attachments():
    turns = [{"user": "first", "tool_calls": [{"name": "shell"}]}]
    salvaged = [{"role": "user", "tools": [], "content": "partial"}]
    guards = {"fired": {"narration_nudge": 1}}
    rec = run_eval._timeout_record({"id": "x", "lang": "en"}, turns,
                                   "please create a rule", salvaged, guards)
    assert rec["status"] == "timeout"
    assert rec["turn_records"] == turns  # already-captured trace survives
    assert rec["server_history"] == salvaged
    assert rec["guards"] == guards
    assert "please create a rule" in rec["message"]


def test_timeout_record_without_salvage_has_no_key():
    rec = run_eval._timeout_record({"id": "x", "lang": "en"}, [], "q")
    assert rec["status"] == "timeout"
    assert "server_history" not in rec


def test_note_timeout_exactly_once_and_only_for_timeouts():
    timed_out = []
    rec = {"status": "timeout"}
    other = {"status": None}
    run_eval._note_timeout(timed_out, "k[en]", rec)   # return path
    run_eval._note_timeout(timed_out, "k[en]", rec)   # exception path replay
    run_eval._note_timeout(timed_out, "k[en]", other)
    assert timed_out == ["k[en]"]


def test_failure_reasons_multi_marker_precedence():
    # unexpected_argument outranks mutually_exclusive when both appear —
    # pinned so triage counts stay stable across classifier edits.
    call = {"name": "shell", "arguments": {"command": "neomind rule create --x"},
            "result": {"exit_code": 2, "stdout": "",
                       "stderr": "error: unexpected argument '--x' found; "
                                 "note: --name cannot be used with '--body'"}}
    r = hs.failure_reasons({"turn_records": [{"tool_calls": [call]}]})
    assert r == {"unexpected_argument": 1}


# ---------------------------- prep-phase timeout passthrough (2026-11-10) ----

def test_prep_phase_case_timeout_propagates(tmp_root=None):
    # Review repro: spawn() raising _CaseTimeout came back mislabeled as
    # "seed_failure: spawn failed:". Every prep phase must let the alarm
    # exception reach the case-level handler (status == "timeout").
    import tempfile
    from pathlib import Path
    import json as _json
    tmp = Path(tempfile.mkdtemp())
    case_file = tmp / "case.json"
    case_file.write_text(_json.dumps({
        "id": "c1", "lang": "en", "category": "device", "workflow": "device",
        "scenario_type": "single_turn", "description": "x", "turns": [{"user": "hi"}],
        "setup": {"fixture": "seed-empty", "extras": {}},
        "expectations": {"per_turn": ["x"], "overall": "y"}}))
    real_server = run_eval.server

    class _AlarmSrv:
        def __init__(self, *a, **k): pass
        def spawn(self, *a, **k): raise run_eval._CaseTimeout()
        def shutdown(self): pass

    class _FakeServerModule:
        TestServer = _AlarmSrv

    run_eval.server = _FakeServerModule  # type: ignore
    try:
        raised = False
        try:
            run_eval.run_case(str(case_file))
        except run_eval._CaseTimeout:
            raised = True  # correct: reaches the case-level timeout handler
        assert raised, "prep-phase _CaseTimeout must propagate, not become seed_failure"
    finally:
        run_eval.server = real_server


# --------------------------------------------- salvage hard time bound ------

class _SlowSrv:
    """Fake server whose history endpoint trickles far past any read timeout."""
    def get(self, path, timeout=30.0):
        time.sleep(6)
        raise AssertionError("must not complete")


class _FastSrv:
    def get(self, path, timeout=30.0):
        class R:
            def raise_for_status(self): pass
            def json(self):
                return {"data": {"messages": [
                    {"role": "assistant", "content": "partial",
                     "tool_calls": [{"name": "shell",
                                     "arguments": {"command": "neomind x"}}]}]}}
        return R()


def test_salvage_hard_total_cap():
    # requests' timeout only bounds connect + EACH read wait; the salvage must
    # return within its own wall deadline even if the fetch never completes.
    t0 = time.monotonic()
    out = run_eval._salvage_server_history(_SlowSrv(), "s", total_cap_s=1.0)
    elapsed = time.monotonic() - t0
    assert out is None
    assert elapsed < 5.0, f"salvage exceeded its hard cap: {elapsed:.1f}s"


def test_salvage_does_not_block_process_exit():
    # Acceptance part 2 (2026-11-09 review): a lingering fetch must not delay
    # INTERPRETER exit. The subprocess below calls salvage against a server
    # that trickles for 10s with a 1s cap; a non-daemon worker (e.g. a
    # ThreadPoolExecutor thread, which threading joins at exit) would keep the
    # process alive ~10s. Daemon workers let it exit in ~1s.
    import subprocess
    import sys as _sys
    code = (
        "import sys, time; sys.path.insert(0, %r); import run_eval\n"
        "class Slow:\n"
        "    def get(self, path, timeout=30.0): time.sleep(10); raise AssertionError\n"
        "out = run_eval._salvage_server_history(Slow(), 's', total_cap_s=1.0)\n"
        "assert out is None\n"
        "print('ok')\n"
    ) % (os.path.join(os.path.dirname(__file__), ".."),)
    t0 = time.monotonic()
    proc = subprocess.run([_sys.executable, "-c", code], capture_output=True,
                          text=True, timeout=30)
    elapsed = time.monotonic() - t0
    assert proc.returncode == 0 and "ok" in proc.stdout, proc.stderr[-300:]
    assert elapsed < 5.0, f"process exit blocked by lingering salvage: {elapsed:.1f}s"


def test_salvage_fast_path_parses_history():
    out = run_eval._salvage_server_history(_FastSrv(), "s")
    assert out == [{"role": "assistant", "tools": ["shell: neomind x"],
                    "content": "partial"}]


# --------------------------- cmd_regression contract via latency mock --------

def test_regression_timeout_contract_with_mocked_case(tmp_root=None):
    # Full contract through the real cmd_regression loop with run_case mocked
    # to sleep past the turn: elapsed_ms (wall clock around run_case) covers
    # the dying turn; scores row carries status/failure_mode timeout; the
    # timed_out summary registers the case exactly once.
    import tempfile
    from pathlib import Path
    import argparse as _ap
    import contextlib
    import io
    tmp = Path(tempfile.mkdtemp())
    (tmp / "cases" / "en" / "device").mkdir(parents=True)
    (tmp / "cases" / "en" / "device" / "case1.json").write_text(__import__("json").dumps({
        "id": "case1", "lang": "en", "category": "device", "workflow": "device",
        "scenario_type": "single_turn", "description": "x",
        "turns": [{"user": "hi"}],
        "expectations": {"per_turn": ["x"], "overall": "y"},
        "expect": {"tools": ["shell"], "commands": ["neomind device list"]}}))
    (tmp / "set.txt").write_text("case1\n")
    real_run_case = run_eval.run_case
    real_identity = run_eval._check_model_identity
    real_bin_version = run_eval._bin_version
    real_resolve = run_eval.server._resolve_neomind_bin

    def slow_case(path):
        time.sleep(1.1)  # the dying turn's wall time — must land in elapsed_ms
        return {"case_id": "case1", "lang": "en",
                "turn_records": [{"user": "hi"}], "state_queries": [],
                "suspected_fallback": False, "guards": {}, "status": "timeout",
                "error_type": "timeout", "message": "cap exceeded during turn"}

    run_eval.run_case = slow_case
    run_eval._check_model_identity = lambda *a, **k: 0
    run_eval._bin_version = lambda: None
    run_eval.server._resolve_neomind_bin = lambda: "stub"
    try:
        args = _ap.Namespace(root=str(tmp / "cases"), regression_set=str(tmp / "set.txt"),
                             lang="en", rounds=1, baseline=None, update_baseline=False,
                             case_timeout=30, run_dir=str(tmp / "run"),
                             skip_preflight=True, allow_model_mismatch=False,
                             skip_multimodal=True)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            code = run_eval.cmd_regression(args)
        rows = [__import__("json").loads(l) for l in
                (tmp / "run" / "scores.jsonl").read_text().splitlines() if l.strip()]
        assert code == 0 and len(rows) == 1
        row = rows[0]
        assert row["status"] == "timeout" and row["failure_mode"] == "timeout"
        assert row["elapsed_ms"] >= 1000, "elapsed must cover the dying turn"
        text = buf.getvalue()
        assert "timeouts 1/1" in text
        assert text.count("case1[en]") - text.count("case1[en] rounds") >= 1
        # registered exactly once in the timed_out listing
        to_line = [l for l in text.splitlines() if "timed out" in l]
        assert to_line and to_line[0].count("case1[en]") == 1
    finally:
        run_eval.run_case = real_run_case
        run_eval._check_model_identity = real_identity
        run_eval._bin_version = real_bin_version
        run_eval.server._resolve_neomind_bin = real_resolve


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

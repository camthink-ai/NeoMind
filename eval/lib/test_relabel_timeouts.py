"""Tests for relabel_timeouts.py — layouts, counting, idempotency, immutability.

Run standalone: `python3 eval/lib/test_relabel_timeouts.py`."""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import relabel_timeouts as rt  # noqa: E402


def _classic_dir(root: Path):
    cases = [
        {"case_id": "a", "lang": "en", "status": "agent_error",
         "message": "turn failed ('x'): _CaseTimeout: exceeded 600s"},
        {"case_id": "b", "lang": "en", "status": "agent_error",
         "message": "turn failed ('y'): asyncio.TimeoutError"},
        {"case_id": "c", "lang": "zh", "status": None, "message": None},
    ]
    (root / "cases.jsonl").write_text(
        "\n".join(json.dumps(c) for c in cases) + "\n")
    (root / "scores.jsonl").write_text(
        "\n".join(json.dumps({"case_id": c["case_id"], "hard": {}}) for c in cases) + "\n")


def _controller_dir(root: Path):
    t = root / "trials" / "arm--a--en"
    t.mkdir(parents=True)
    (t / "cases.jsonl").write_text(json.dumps(
        {"case_id": "a", "lang": "en", "status": "agent_error",
         "message": "_CaseTimeout"}) + "\n")
    (t / "result.json").write_text(json.dumps(
        {"key": "arm--a--en", "status": "agent_error"}))


def test_classic_layout_counts_and_true_errors_remain():
    with tempfile.TemporaryDirectory() as d:
        root = Path(d) / "run1"
        root.mkdir()
        _classic_dir(root)
        r = rt.relabel(root, str(root))
        assert r["trials"] == 3
        assert r["agent_error_total"] == 2
        assert r["relabelled_to_timeout"] == 1   # only the _CaseTimeout one
        assert r["true_agent_error_remaining"] == 1


def test_controller_layout_and_idempotency_and_immutability():
    with tempfile.TemporaryDirectory() as d:
        root = Path(d) / "gate" / "arm"
        (root).mkdir(parents=True)
        _controller_dir(root)
        before = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
        r1 = rt.relabel(root, "arm")
        r2 = rt.relabel(root, "arm")  # idempotent: same inputs -> same counts
        assert r1 == r2
        assert r1["relabelled_to_timeout"] == 1
        after = {p: p.read_bytes() for p in root.rglob("*") if p.is_file()}
        assert before == after  # originals untouched


def test_no_timeout_data_yields_zero():
    with tempfile.TemporaryDirectory() as d:
        root = Path(d) / "clean"
        root.mkdir()
        (root / "cases.jsonl").write_text(json.dumps(
            {"case_id": "a", "lang": "en", "status": None}) + "\n")
        r = rt.relabel(root, "clean")
        assert r["relabelled_to_timeout"] == 0 and r["trials"] == 1


def test_missing_result_json_still_relabels_from_raw_record():
    # 2026-11-10 review repro: result.json absent + raw record clearly
    # agent_error/_CaseTimeout was counted as 0 errors, 0 timeouts. The raw
    # cases.jsonl is the primary source and must catch it alone.
    with tempfile.TemporaryDirectory() as d:
        t = Path(d) / "gate" / "arm" / "trials" / "arm--a--en"
        t.mkdir(parents=True)
        (t / "cases.jsonl").write_text(json.dumps(
            {"case_id": "a", "lang": "en", "status": "agent_error",
             "message": "turn failed ('x'): _CaseTimeout: exceeded"}) + "\n")
        r = rt.relabel(Path(d) / "gate" / "arm", "arm")
        assert r["relabelled_to_timeout"] == 1
        assert r["true_agent_error_remaining"] == 0


def test_status_conflict_between_raw_and_result_reported():
    with tempfile.TemporaryDirectory() as d:
        t = Path(d) / "gate" / "arm" / "trials" / "arm--a--en"
        t.mkdir(parents=True)
        (t / "cases.jsonl").write_text(json.dumps(
            {"case_id": "a", "lang": "en", "status": "agent_error",
             "message": "_CaseTimeout"}) + "\n")
        (t / "result.json").write_text(json.dumps({"status": "timeout"}))
        r = rt.relabel(Path(d) / "gate" / "arm", "arm")
        assert r["relabelled_to_timeout"] == 1          # raw record governs
        assert r["status_conflict_count"] == 1           # but conflict surfaced
        assert r["status_conflicts"][0]["raw"] == "agent_error"


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

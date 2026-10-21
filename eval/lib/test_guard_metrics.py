"""Tests for the turn-guard ledger ingest + report aggregation.

Run standalone: `.venv/bin/python eval/lib/test_guard_metrics.py`

Covers the two new pieces of the eval-side guard metrics:
  - `_parse_guard_metrics` (ANSI-heavy tracing lines → structured events)
  - `report.aggregate` / `write_grade_card` (nudge effectiveness ratio)
"""
from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
from pathlib import Path

_LIB = Path(__file__).resolve().parent
_EVAL = _LIB.parent
sys.path.insert(0, str(_LIB))

import report  # noqa: E402

_spec = importlib.util.spec_from_file_location("run_eval", _EVAL / "run_eval.py")
_run_eval = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_run_eval)

parse = _run_eval._parse_guard_metrics

ANSI = "\x1b[2m"


def _line(ts: str, guard: str, outcome: str, round_no: int, reason: str) -> str:
    """Mirror tracing's ANSI-coloured structured output."""
    return (
        f"{ANSI}{ts}\x1b[0m turn guard decision "
        f'\x1b[3mguard\x1b[0m=\x1b[2m"{guard}"\x1b[0m '
        f'\x1b[3moutcome\x1b[0m=\x1b[2m"{outcome}"\x1b[0m '
        f'\x1b[3mround\x1b[0m=\x1b[2m{round_no}\x1b[0m '
        f'\x1b[3mreason\x1b[0m=\x1b[2m"{reason}"\x1b[0m'
    )


def test_parses_ansi_coloured_decisions():
    lines = [
        "[server:err] " + _line("t1", "narration_nudge", "fired", 1, "promise_without_action"),
        "unrelated log line",
        _line("t2", "narration_nudge_outcome", "fired", 2, "tool_call_in_nudge_round"),
    ]
    g = parse(lines)
    assert len(g["events"]) == 2, g
    assert g["fired"] == {"narration_nudge": 1, "narration_nudge_outcome": 1}, g
    assert g["nudge_rounds_observed"] == 1, g
    assert g["nudge_recovered"] == 1, g


def test_counts_suppressed_and_failed_nudges():
    lines = [
        _line("t1", "plan_only_suppression", "suppressed", 0, "user asked for a plan"),
        _line("t2", "narration_nudge", "suppressed", 1, "plan_only"),
        _line("t3", "narration_nudge_outcome", "skipped", 2, "no_tool_call_in_nudge_round"),
    ]
    g = parse(lines)
    assert g["suppressed"] == {
        "plan_only_suppression": 1,
        "narration_nudge": 1,
    }, g
    assert g["nudge_rounds_observed"] == 1, g
    assert g["nudge_recovered"] == 0, g


def test_missing_guard_data_degrades_gracefully():
    # Old score lines predate the ledger: no key, no crash.
    agg = report.aggregate(json.dumps({"case_id": "x", "hard": {}}) + "\n")
    assert agg["nudge_rounds_observed"] == 0, agg
    assert dict(agg["guard_fired"]) == {}, agg


def test_report_aggregates_guard_counts_and_ratio():
    rows = [
        {
            "case_id": "a",
            "lang": "zh",
            "category": "smoke",
            "hard": {},
            "guards": {
                "fired": {"narration_nudge": 1},
                "suppressed": {"narration_label": 1},
                "nudge_rounds_observed": 1,
                "nudge_recovered": 1,
            },
        },
        {
            "case_id": "b",
            "lang": "zh",
            "category": "smoke",
            "hard": {},
            "guards": {
                "fired": {"narration_nudge": 1},
                "suppressed": {},
                "nudge_rounds_observed": 1,
                "nudge_recovered": 0,
            },
        },
    ]
    scores = "\n".join(json.dumps(r) for r in rows) + "\n"
    agg = report.aggregate(scores)
    assert agg["guard_fired"]["narration_nudge"] == 2, dict(agg["guard_fired"])
    assert agg["guard_suppressed"]["narration_label"] == 1, dict(agg["guard_suppressed"])
    assert agg["nudge_rounds_observed"] == 2, agg
    assert agg["nudge_recovered"] == 1, agg

    with tempfile.TemporaryDirectory() as td:
        out = Path(td) / "grade-card.md"
        report.write_grade_card(agg, out)
        md = out.read_text()
    assert "## Turn Guards" in md, md
    assert "narration_nudge" in md, md
    assert "1/2 (50%)" in md, md


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

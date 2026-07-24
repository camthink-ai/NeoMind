"""Tests for eval.sft.curate. Run: .venv/bin/python eval/sft/test_curate.py"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from eval.sft import curate as C  # noqa: E402


def _cr(*tool_calls, passed=True, with_system=True):
    """Build a CaseRecord with the given (name, args) tool calls in order."""
    raw = [{"role": "user", "content": "do it"}]
    for name, args in tool_calls:
        raw.append({"role": "assistant", "content": "", "tool_calls": [{"name": name, "arguments": args}]})
    raw.append({"role": "assistant", "content": "done"})
    return {
        "case_id": "device-create-x",
        "state_queries": [{"type": "device_exists", "passed": passed}],
        "turn_records": [{"raw_messages": raw}],
    }, ([{"system": "SYS" * 100}] if with_system else [])


def test_golden_case():
    cr, tr = _cr(("skill", {"action": "load", "id": "device-onboarding"}),
                 ("shell", {"command": "neomind device create --name cam"}))
    s = C.score_case(cr, tr, lang="en")
    assert s["golden"] is True, s["reasons"]
    assert s["tools_used"] == ["shell", "skill"]
    assert s["shell_domain_verbs"] == ["device/create"]
    assert s["tool_call_count"] == 2


def test_reject_state_query_failed():
    cr, tr = _cr(("shell", {"command": "neomind device create --name cam"}), passed=False)
    s = C.score_case(cr, tr)
    assert not s["golden"] and any("state_query_failed" in r for r in s["reasons"])


def test_no_assertion_is_golden_by_default():
    # list/control/query cases have no state change to assert — still a correct
    # workflow demo, so golden by default (needed for coverage).
    raw = [
        {"role": "user", "content": "list devices"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"name": "shell", "arguments": {"command": "neomind device list"}}]},
        {"role": "assistant", "content": "here they are"},
    ]
    cr = {"case_id": "device-list", "state_queries": [], "turn_records": [{"raw_messages": raw}]}
    s = C.score_case(cr, [{"system": "SYS" * 100}])
    assert s["golden"] is True, s["reasons"]


def test_no_assertion_rejected_under_strict():
    raw = [
        {"role": "user", "content": "list"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"name": "shell", "arguments": {"command": "neomind device list"}}]},
    ]
    cr = {"case_id": "device-list", "state_queries": [], "turn_records": [{"raw_messages": raw}]}
    s = C.score_case(cr, [{"system": "SYS" * 100}], strict_outcome=True)
    assert not s["golden"]
    assert any("no_state_query" in r for r in s["reasons"])


def test_reject_non_ideal_tool():
    # file_write is the student's failure mode -> reject for a CRUD case.
    cr, tr = _cr(("file_write", {"path": "/etc/x", "content": "hack"}))
    s = C.score_case(cr, tr)
    assert not s["golden"]
    assert any(r.startswith("non_ideal_tools") for r in s["reasons"])
    assert "file_write" in s["bad_tools"]


def test_reject_duplicate_call_retry():
    cr, tr = _cr(("shell", {"command": "neomind device create --name cam"}),
                 ("shell", {"command": "neomind device create --name cam"}))  # identical retry
    s = C.score_case(cr, tr)
    assert not s["golden"] and "duplicate_call_retry" in s["reasons"]


def test_distinct_calls_not_duplicate():
    # same tool, different args = legit, not a retry
    cr, tr = _cr(("shell", {"command": "neomind device create --name a"}),
                 ("shell", {"command": "neomind device list"}))
    s = C.score_case(cr, tr)
    assert "duplicate_call_retry" not in s["reasons"]


def test_reject_too_many_calls():
    calls = [("shell", {"command": f"neomind device list --p{i}"}) for i in range(C.MAX_TOOL_CALLS + 1)]
    cr, tr = _cr(*calls)
    s = C.score_case(cr, tr)
    assert not s["golden"] and any(r.startswith("too_many_calls") for r in s["reasons"])


def test_reject_no_system_prompt():
    cr, tr = _cr(("shell", {"command": "neomind device create"}), with_system=False)
    s = C.score_case(cr, tr)
    assert not s["golden"] and "no_system_prompt_in_trace" in s["reasons"]


def test_case_looks_clean_mirror_bar_minus_fidelity():
    # clean: shell only, no failed sq, within ceiling
    cr, _ = _cr(("shell", {"command": "neomind device create --name a"}))
    assert C.case_looks_clean(cr) is True
    # dirty: non-ideal tool
    cr, _ = _cr(("file_write", {"path": "/x", "content": "y"}))
    assert C.case_looks_clean(cr) is False
    # dirty: failed state_query
    cr, _ = _cr(("shell", {"command": "neomind device create"}), passed=False)
    assert C.case_looks_clean(cr) is False
    # dirty: duplicate retry
    cr, _ = _cr(("shell", {"command": "neomind device create"}),
                ("shell", {"command": "neomind device create"}))
    assert C.case_looks_clean(cr) is False


def test_parse_shell_domain_verb():
    assert C._parse_shell_domain_verb("neomind rule create --name x") == ("rule", "create")
    assert C._parse_shell_domain_verb("neomind device list") == ("device", "list")
    assert C._parse_shell_domain_verb("echo hi") is None
    assert C._parse_shell_domain_verb("") is None


def test_domain_of():
    assert C._domain_of("device-create-camera") == "device"
    assert C._domain_of("rule-create-temp-high") == "rule"
    assert C._domain_of("agent-configure-x") in ("agent",)  # falls to prefix match
    assert C._domain_of("widget-delete-existing") == "widget"


def test_coverage_report_aggregation_and_gaps():
    g = C.score_case(*_cr(("shell", {"command": "neomind device create"})), lang="en")
    g["golden"] = True  # force
    f = C.score_case(*_cr(("shell", {"command": "neomind device create"}), passed=False), lang="zh")
    rep = C.coverage_report([g, f])
    assert rep["total_cases"] == 2 and rep["golden_cases"] == 1
    assert rep["domain_x_lang_golden"].get("device/en") == 1
    assert "device/zh" in rep["domain_lang_gaps"]  # the failed zh case is a gap
    assert rep["tool_demo_count"].get("shell") == 1


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn(); print(f"  PASS  {fn.__name__}")
        except Exception as e:  # noqa: BLE001
            failed += 1; print(f"  FAIL  {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{len(fns)-failed}/{len(fns)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(_run_all())

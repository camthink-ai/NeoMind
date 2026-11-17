"""Tests for skill_routing grading — the contract the §8 gates read.

Synthetic records mirror the runner's cases.jsonl shape (turn_records ->
tool_calls with name/arguments/result/success; arguments may arrive as a
JSON string, results as JSON string or dict — the WS layer has shipped both).
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import skill_routing as sr  # noqa: E402


def _tc(name, arguments, result=None, success=True):
    if isinstance(arguments, dict):
        arguments = json.dumps(arguments)
    if isinstance(result, (dict, list)):
        result = json.dumps(result)
    return {"name": name, "arguments": arguments, "result": result, "success": success}


def _record(*tool_calls_per_turn):
    turns = [{"user": f"t{i}", "tool_calls": tcs, "assistant_message": ""}
             for i, tcs in enumerate(tool_calls_per_turn)]
    return {"turn_records": turns}


CASE = {
    "skill_routing": {
        "policy": "optional",
        "acceptable": ["dashboard-management"],
        "irrelevant": ["device-onboarding"],
    }
}


def test_exact_load_is_applicable_and_not_fuzzy():
    rec = _record([_tc("skill", {"action": "load", "id": "dashboard-management"},
                       {"id": "dashboard-management", "guide": "..."})])
    g = sr.grade_trial(CASE, rec)
    assert g["first_load"] == "dashboard-management"
    assert g["first_load_applicable"] is True
    assert g["silent_misloads"] == []
    assert g["n_load"] == 1


def test_fuzzy_load_to_unacceptable_skill_is_silent_misload():
    rec = _record([_tc("skill", {"action": "load", "id": "设备"},
                       {"id": "device-onboarding", "resolved_from": "设备"})])
    g = sr.grade_trial(CASE, rec)
    assert g["first_load_applicable"] is False
    assert len(g["silent_misloads"]) == 1
    assert g["silent_misloads"][0]["resolved"] == "device-onboarding"
    assert g["irrelevant_loaded"] == ["device-onboarding"]


def test_policy_none_with_any_load_is_unnecessary():
    case = {"skill_routing": {"policy": "none", "acceptable": [],
                              "irrelevant": ["device-onboarding"]}}
    rec = _record([_tc("skill", {"action": "load", "id": "system-info"},
                       {"id": "system-info"})])
    g = sr.grade_trial(case, rec)
    assert g["unnecessary_load"] is True


def test_needs_selection_is_not_a_load_and_not_a_misload():
    # New contract (skill-routing round): needs_selection is a SUCCESS result
    # with status + candidates — nothing was loaded, so it is neither a load
    # nor a misload. The legacy error-with-metadata shape is also tolerated.
    rec = _record([_tc("skill", {"action": "load", "id": "management"},
                       {"status": "needs_selection", "loaded": False,
                        "requested_id": "management",
                        "candidates": [{"id": "rule-management"},
                                       {"id": "llm-management"}]})])
    g = sr.grade_trial(CASE, rec)
    assert g["n_load"] == 0
    assert g["silent_misloads"] == []
    calls = sr.extract_skill_calls(rec)
    assert calls[0]["candidates"] == ["rule-management", "llm-management"]
    assert calls[0]["status"] == "needs_selection"

    legacy = _record([_tc("skill", {"action": "load", "id": "management"},
                          {"requested_id": "management",
                           "candidates": [{"id": "rule-management"}]},
                          success=False)])
    g2 = sr.grade_trial(CASE, legacy)
    assert g2["n_load"] == 0
    assert g2["silent_misloads"] == []


def test_no_acceptable_set_fuzzy_success_counts_as_misload():
    # The no-match case: any silent resolution is a misload even without an
    # acceptable list.
    case = {"skill_routing": {"policy": "optional", "acceptable": [], "irrelevant": []}}
    rec = _record([_tc("skill", {"action": "load", "id": "xyzzy-guide"},
                       {"id": "extension-management", "resolved_from": "xyzzy-guide"})])
    g = sr.grade_trial(case, rec)
    assert len(g["silent_misloads"]) == 1


def test_search_top3_coverage():
    rec = _record([_tc("skill", {"action": "search", "query": "dashboard"},
                       {"found": True, "matches": [
                           {"id": "widget-management"},
                           {"id": "dashboard-management"},
                           {"id": "rule-management"}]})])
    g = sr.grade_trial(CASE, rec)
    assert (g["top3_total"], g["top3_hit"]) == (1, 1)


def test_search_top3_miss():
    rec = _record([_tc("skill", {"action": "search", "query": "dashboard"},
                       {"found": True, "matches": [{"id": "widget-management"},
                                                   {"id": "rule-management"}]})])
    g = sr.grade_trial(CASE, rec)
    assert (g["top3_total"], g["top3_hit"]) == (1, 0)


def test_empty_search_counts_as_recall_miss():
    # Review P2-3: a case WITH applicable guides must count empty searches in
    # the denominator — one hit + one empty is 50%, not 100%.
    hit = {"action": "search", "query": "可视化"}
    empty = {"action": "search", "query": "面板"}
    rec = _record(
        [_tc("skill", hit, {"found": True, "matches": [{"id": "dashboard-management"}]}),
         _tc("skill", empty, {"found": False, "matches": []})],
    )
    g = sr.grade_trial(CASE, rec)
    assert (g["top3_total"], g["top3_hit"]) == (2, 1)
    assert g["case_searched"] is True
    assert g["case_top3_hit"] is True


def test_aggregate_case_level_coverage_counts_each_case_once():
    # Repeated searching must not reweight coverage: case 1 hits on its second
    # search (covered), case 2 only misses (not covered) -> case coverage 1/2
    # regardless of search counts.
    g1 = sr.grade_trial(CASE, _record([
        _tc("skill", {"action": "search", "query": "a"},
            {"found": False, "matches": []}),
        _tc("skill", {"action": "search", "query": "dashboard"},
            {"found": True, "matches": [{"id": "dashboard-management"}]}),
    ]))
    g2 = sr.grade_trial(CASE, _record([
        _tc("skill", {"action": "search", "query": "b"},
            {"found": False, "matches": []}),
    ]))
    agg = sr.aggregate([g1, g2])
    assert agg["top3_case_denominator"] == 2
    assert agg["top3_case_coverage"] == 0.5
    # Per-search view stays honest too: 1 hit over 3 searches.
    assert (agg["top3_denominator"], agg["top3_hit"]) == (3, 1)


def test_within_turn_duplicate_and_section_loop_counts():
    section_load = {"action": "load", "id": "dashboard-management",
                    "section": "Commands"}
    rec = _record(
        [_tc("skill", section_load, {"id": "dashboard-management", "section": "Commands"}),
         _tc("skill", section_load, {"id": "dashboard-management", "section": "Commands"}),
         _tc("skill", {"action": "load", "id": "dashboard-management"},
             {"id": "dashboard-management"})],
    )
    g = sr.grade_trial(CASE, rec)
    assert g["within_turn_dup_loads"] == 1
    assert g["within_turn_dup_section_loops"] == 1
    assert g["cross_turn_reloads"] == 0


def test_cross_turn_reload_is_counted_separately_not_as_dup():
    load = {"action": "load", "id": "dashboard-management"}
    ok = {"id": "dashboard-management"}
    rec = _record([_tc("skill", load, ok)], [_tc("skill", load, ok)])
    g = sr.grade_trial(CASE, rec)
    assert g["within_turn_dup_loads"] == 0
    assert g["cross_turn_reloads"] == 1


def test_repeated_identical_search_within_turn():
    q = {"action": "search", "query": "仪表盘"}
    rec = _record(
        [_tc("skill", q, {"found": True, "matches": [{"id": "dashboard-management"}]}),
         _tc("skill", q, {"found": True, "matches": [{"id": "dashboard-management"}]})],
    )
    g = sr.grade_trial(CASE, rec)
    assert g["repeated_searches"] == 1


def test_aggregate_rates_and_zero_gates():
    graded = [
        sr.grade_trial(CASE, _record([_tc("skill", {"action": "load", "id": "dashboard-management"},
                                          {"id": "dashboard-management"})])),
        sr.grade_trial({"skill_routing": {"policy": "none", "acceptable": [],
                                          "irrelevant": []}}, _record([])),
        sr.grade_trial(CASE, _record([_tc("skill", {"action": "load", "id": "设备"},
                                          {"id": "device-onboarding", "resolved_from": "设备"})])),
    ]
    agg = sr.aggregate(graded)
    assert agg["trials"] == 3
    assert agg["silent_misload_events"] == 1
    assert agg["first_load_denominator"] == 2
    assert agg["first_load_applicable_rate"] == 0.5
    assert agg["unnecessary_load_rate"] == 0.0
    assert agg["median_prompt_tokens"] is None


def test_median_prompt_tokens_from_context_usage():
    rec = {
        "turn_records": [
            {"tool_calls": [], "context_usage": [{"promptTokens": 5000}, {"promptTokens": 7000}]},
            {"tool_calls": [], "context_usage": [{"promptTokens": 9000}]},
        ]
    }
    g = sr.grade_trial(CASE, rec)
    # finals per turn: 7000, 9000 -> median 9000 (upper middle for even lists
    # is index len//2 = 1; two entries -> 9000)
    assert g["median_final_prompt_tokens"] == 9000


def _run_all() -> int:
    fns = [v for k, v in sorted(globals().items())
           if k.startswith("test_") and callable(v)]
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

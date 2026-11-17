"""Skill-routing grading: per-trial metrics from full turn records.

Reads the runner's cases.jsonl records (turn_records[].tool_calls carry the
`skill` tool's arguments AND full result JSON — verified 2026-11-17: the WS
ToolCallEnd event forwards the untruncated result for small payloads), plus
the case's ``skill_routing`` block ({policy, acceptable, irrelevant}).

Deliberate contract notes:
- A `load` result JSON has {"id": resolved, "resolved_from": requested?} on
  fuzzy resolution; needs_selection/not_found return success=false with a
  "candidates" array in metadata.
- A `search` result JSON has {"found": bool, "matches": [{"id", ...}]}.
- Grading NEVER infers business success from skill calls: business verdicts
  come from state_queries in hard_signal. This module only grades routing
  behavior (loaded what, searched what, repeats, honesty).
"""
from __future__ import annotations

import json
from collections import Counter


def _parse_args(tc: dict) -> dict:
    args = tc.get("arguments")
    if isinstance(args, str):
        try:
            args = json.loads(args)
        except Exception:
            return {}
    return args if isinstance(args, dict) else {}


def _parse_result(tc: dict) -> dict | None:
    r = tc.get("result")
    if isinstance(r, dict):
        return r
    if isinstance(r, str) and r.strip():
        try:
            v = json.loads(r)
            return v if isinstance(v, dict) else None
        except Exception:
            return None
    return None


def extract_skill_calls(record: dict) -> list[dict]:
    """Flatten every `skill` tool call across turns with resolved identity.

    Returns a list of {turn, action, requested, query, section, success,
    resolved_id, fuzzy, candidates, result}. Order preserved (call order).
    """
    calls: list[dict] = []
    for ti, tr in enumerate(record.get("turn_records") or []):
        for tc in tr.get("tool_calls") or []:
            if tc.get("name") != "skill":
                continue
            args = _parse_args(tc)
            res = _parse_result(tc)
            requested = args.get("id")
            resolved = res.get("id") if res else None
            fuzzy = bool(res and res.get("resolved_from")) or (
                bool(resolved) and bool(requested) and requested != resolved
            )
            calls.append({
                "turn": ti,
                "action": args.get("action"),
                "requested": requested,
                "query": args.get("query"),
                "section": args.get("section"),
                "success": bool(tc.get("success")),
                "status": res.get("status") if res else None,
                "resolved_id": resolved,
                "fuzzy": fuzzy,
                "candidates": [c.get("id") for c in (res.get("candidates") or [])
                               if isinstance(c, dict)] if res else [],
                "top3": [m.get("id") for m in (res.get("matches") or [])[:3]
                         if isinstance(m, dict)] if res else [],
                "found": res.get("found") if res else None,
            })
    return calls


def grade_trial(case: dict, record: dict) -> dict:
    """Per-trial skill-routing metrics (see aggregate() for the arm-level view)."""
    sr = case.get("skill_routing") or {}
    policy = sr.get("policy", "optional")
    acceptable = set(sr.get("acceptable") or [])
    irrelevant = set(sr.get("irrelevant") or [])
    calls = extract_skill_calls(record)

    loads = [c for c in calls if c["action"] == "load" and c["success"] and c["resolved_id"]]
    searches = [c for c in calls if c["action"] == "search"]
    creates = [c for c in calls if c["action"] in ("create", "update", "delete")]

    # First-load applicability (only meaningful when a guide was loaded AND the
    # case declares what would be applicable).
    first_load = loads[0]["resolved_id"] if loads else None
    first_applicable = (
        first_load in acceptable if (first_load and acceptable) else None
    )

    # Ambiguous-name SILENT misload: a successful load the case does not accept.
    # With an acceptable set: resolved outside it. Without one (e.g. the
    # no-match case): ANY fuzzy resolution is a silent misload.
    misloads = []
    for c in loads:
        if acceptable:
            if c["resolved_id"] not in acceptable:
                misloads.append({"requested": c["requested"],
                                 "resolved": c["resolved_id"], "fuzzy": c["fuzzy"]})
        elif c["fuzzy"]:
            misloads.append({"requested": c["requested"],
                             "resolved": c["resolved_id"], "fuzzy": True})

    unnecessary = bool(loads) and policy == "none"
    irrelevant_loaded = sorted({c["resolved_id"] for c in loads
                                if c["resolved_id"] in irrelevant})

    # Top-3 candidate coverage. Denominator: EVERY search in a case that
    # declares applicable guides — an empty search result is a recall MISS,
    # not an excuse to shrink the denominator (review P2-3: excluding empty
    # searches inflated coverage). Also tracked per-CASE (any-search-hit) so
    # repeated searching cannot reweight the metric.
    cov_total = cov_hit = 0
    case_searched = bool(searches) and bool(acceptable)
    case_top3_hit = False
    for c in searches:
        if not acceptable:
            continue
        cov_total += 1
        if c["found"] is True and set(c["top3"]) & acceptable:
            cov_hit += 1
            case_top3_hit = True

    # Within-turn duplicate loads (true no-progress repeats; cross-turn reloads
    # after the turn-boundary clear are by-design, counted separately).
    per_turn = Counter()
    for c in loads:
        per_turn[(c["turn"], c["resolved_id"], c["section"])] += 1
    within_turn_dups = sum(n - 1 for n in per_turn.values() if n > 1)
    within_turn_dup_section_loops = sum(
        n - 1 for (t, rid, sec), n in per_turn.items() if n > 1 and sec
    )
    reload_pairs = sorted(
        {(rid, sec) for (t, rid, sec) in per_turn},
        key=lambda k: (k[0], k[1] or ""),
    )  # distinct guide keys loaded this trial
    cross_turn_reloads = 0
    by_key_turns: dict[tuple, set] = {}
    for c in loads:
        by_key_turns.setdefault((c["resolved_id"], c["section"]), set()).add(c["turn"])
    cross_turn_reloads = sum(1 for v in by_key_turns.values() if len(v) > 1)

    # Unproductive searching: repeated identical searches within one turn.
    per_turn_q = Counter()
    for c in searches:
        per_turn_q[(c["turn"], c["query"])] += 1
    repeated_searches = sum(n - 1 for n in per_turn_q.values() if n > 1)

    prompt_tokens = None
    cu = [t.get("context_usage") or [] for t in (record.get("turn_records") or [])]
    finals = [seq[-1].get("promptTokens") for seq in cu
              if seq and isinstance(seq[-1], dict) and seq[-1].get("promptTokens")]
    if finals:
        finals.sort()
        prompt_tokens = finals[len(finals) // 2]

    return {
        "policy": policy,
        "n_search": len(searches),
        "n_load": len(loads),
        "n_create_update_delete": len(creates),
        "first_load": first_load,
        "first_load_applicable": first_applicable,
        "silent_misloads": misloads,
        "unnecessary_load": unnecessary,
        "irrelevant_loaded": irrelevant_loaded,
        "top3_total": cov_total,
        "top3_hit": cov_hit,
        "case_searched": case_searched,
        "case_top3_hit": case_top3_hit,
        "within_turn_dup_loads": within_turn_dups,
        "within_turn_dup_section_loops": within_turn_dup_section_loops,
        "cross_turn_reloads": cross_turn_reloads,
        "repeated_searches": repeated_searches,
        "distinct_guide_keys": len(reload_pairs),
        "median_final_prompt_tokens": prompt_tokens,
    }


def aggregate(graded: list[dict]) -> dict:
    """Arm-level routing metrics (the pre-registered §8 set).

    Business pass rates come from the scores side and are merged by the
    report CLI, not here — routing grading must stay independent of scoring.
    """
    n = len(graded)
    if not n:
        return {"trials": 0}

    with_load_accepted = [g for g in graded
                          if g["first_load"] is not None and g["first_load_applicable"] is not None]
    applicable = sum(1 for g in with_load_accepted if g["first_load_applicable"])
    none_policy = [g for g in graded if g["policy"] == "none"]
    none_loaded = [g for g in none_policy if g["unnecessary_load"]]
    cov_total = sum(g["top3_total"] for g in graded)
    cov_hit = sum(g["top3_hit"] for g in graded)
    case_den = sum(1 for g in graded if g.get("case_searched"))
    case_hit = sum(1 for g in graded if g.get("case_top3_hit"))
    toks = sorted(g["median_final_prompt_tokens"] for g in graded
                  if g["median_final_prompt_tokens"] is not None)

    return {
        "trials": n,
        "silent_misload_events": sum(len(g["silent_misloads"]) for g in graded),
        "first_load_applicable_rate": (applicable / len(with_load_accepted))
        if with_load_accepted else None,
        "first_load_denominator": len(with_load_accepted),
        "top3_coverage": (cov_hit / cov_total) if cov_total else None,
        "top3_denominator": cov_total,
        "top3_hit": cov_hit,
        "top3_case_coverage": (case_hit / case_den) if case_den else None,
        "top3_case_denominator": case_den,
        "unnecessary_load_rate": (len(none_loaded) / len(none_policy))
        if none_policy else None,
        "unnecessary_load_denominator": len(none_policy),
        "within_turn_dup_loads_total": sum(g["within_turn_dup_loads"] for g in graded),
        "within_turn_dup_section_loops_total": sum(g["within_turn_dup_section_loops"] for g in graded),
        "cross_turn_reload_total": sum(g["cross_turn_reloads"] for g in graded),
        "repeated_search_total": sum(g["repeated_searches"] for g in graded),
        "irrelevant_load_events": sum(len(g["irrelevant_loaded"]) for g in graded),
        "skill_create_update_delete_calls": sum(g["n_create_update_delete"] for g in graded),
        "median_prompt_tokens": toks[len(toks) // 2] if toks else None,
    }


def load_run(cases_jsonl: str, scores_jsonl: str | None = None) -> list[dict]:
    """Load runner output into [{case, record, score?}] triples keyed per line."""
    out = []
    scores: dict[tuple, dict] = {}
    if scores_jsonl:
        with open(scores_jsonl) as f:
            for line in f:
                if line.strip():
                    s = json.loads(line)
                    scores[(s.get("case_id"), s.get("lang"), s.get("round"))] = s
    with open(cases_jsonl) as f:
        for line in f:
            if not line.strip():
                continue
            rec = json.loads(line)
            key = (rec.get("case_id"), rec.get("lang"), rec.get("round"))
            out.append({"record": rec, "score": scores.get(key)})
    return out

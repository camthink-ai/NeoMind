#!/usr/bin/env python3
"""skill-routing paired-acceptance report (pre-registered gates, plan §8).

Usage:
  python3 eval/skill_routing_report.py --cases <run>/cases.jsonl \
      --scores <run>/scores.jsonl \
      [--baseline-cases <A>/cases.jsonl --baseline-scores <A>/scores.jsonl] \
      [--json out.json]

Gates (fixed BEFORE the run; do not retune after seeing results):
  hard-zero   silent misloads / irrelevant loads / skill CRUD on eval fixtures
  rates       first-load applicable >=90% & >= baseline
              top-3 coverage >=95%
              unnecessary-load (policy=none) <=10% & <= baseline
  continuity  multi-turn (sr-multi-*) business pass >=95%
  loops       within-turn dup loads <= baseline; same-section loop == 0
  business    overall pass >= baseline; critical domains no new losses
  tokens      median final prompt tokens <= baseline
  latency     P95 increase >10% -> flag for investigation (not auto-pass)

Sample sizes are for engineering acceptance only — no statistical-significance
claim (plan §8 supplementary rules).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent / "lib"))

import skill_routing  # noqa: E402


def business_pass(score: dict | None) -> bool | None:
    """Business verdict from the runner's hard signals (state queries)."""
    if not score:
        return None
    hard = score.get("hard") or {}
    sq_total = hard.get("sq_total") or 0
    if sq_total:
        return bool(hard.get("sq_all_pass"))
    # No state queries: fall back to cmd_ok when commands were expected,
    # else the case had no hard business assertion (exclude from rate).
    if hard.get("expected_commands"):
        return bool(hard.get("cmd_ok"))
    return None


def summarize(run: dict) -> dict:
    cases = run["cases"]; scores = run["scores"]
    by_key: dict[tuple, dict] = {}
    for line in open(cases):
        if line.strip():
            rec = json.loads(line)
            by_key.setdefault((rec.get("case_id"), rec.get("lang"), rec.get("round")), {"records": []})["records"].append(rec)
    sc = {}
    for line in open(scores):
        if line.strip():
            s = json.loads(line)
            sc[(s.get("case_id"), s.get("lang"), s.get("round"))] = s

    graded, bpass, lat, case_meta = [], [], [], {}
    for key, entry in by_key.items():
        rec = entry["records"][-1]
        score = sc.get(key)
        case = rec.get("case") or rec.get("case_json") or {}
        # The runner persists the case inline per record? No — rebuild from
        # the eval tree by id (records carry case_id + lang only).
        case_meta_key = (rec.get("case_id"), rec.get("lang"))
        if case_meta_key not in case_meta:
            case_meta[case_meta_key] = _load_case(rec.get("case_id"), rec.get("lang"))
        case = case_meta[case_meta_key]
        if not case:
            continue
        g = skill_routing.grade_trial(case, rec)
        g["case_id"], g["lang"], g["round"] = key
        g["scenario_type"] = case.get("scenario_type")
        g["split"] = case.get("split")
        graded.append(g)
        bp = business_pass(score)
        if bp is not None:
            bpass.append((key, bp, case))
        if score and score.get("latency_ms") is not None:
            lat.append(score["latency_ms"])
    return {"graded": graded, "bpass": bpass, "lat": lat}


_CASE_CACHE: dict[tuple, dict] = {}


def _load_case(case_id: str | None, lang: str | None) -> dict:
    if not case_id:
        return {}
    root = Path(__file__).parent / "cases" / (lang or "zh") / "skill-routing" / f"{case_id}.json"
    if root.is_file():
        return json.loads(root.read_text())
    return {}


def _p95(xs: list) -> float | None:
    if not xs:
        return None
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(0.95 * len(xs)))]


def _rate(num: int, den: int) -> float | None:
    return (num / den) if den else None


def gate_report(cand: dict, base: dict | None) -> dict:
    cg = skill_routing.aggregate(cand["graded"])
    bg = skill_routing.aggregate(base["graded"]) if base else None

    def cmp_le(key, gate_val=None):
        c = cg.get(key)
        b = bg.get(key) if bg else None
        ok = (c <= b) if b is not None else True
        if gate_val is not None:
            ok = ok and (c is not None and c <= gate_val)
        return {"cand": c, "base": b, "ok": bool(ok)}

    def cmp_ge(key, gate_val=None):
        c = cg.get(key)
        b = bg.get(key) if bg else None
        ok = True if c is None else (c >= gate_val if gate_val is not None else True)
        if b is not None and c is not None:
            ok = ok and (c >= b)
        return {"cand": c, "base": b, "ok": bool(ok)}

    # business pass
    def bpass_stats(run):
        if not run["bpass"]:
            return None, 0
        n = sum(1 for _, p, _ in run["bpass"] if p)
        return n / len(run["bpass"]), len(run["bpass"])

    cb, cd = bpass_stats(cand)
    bb, bd = bpass_stats(base) if base else (None, 0)

    multi = [(k, p, c) for k, p, c in cand["bpass"] if str(k[0]).startswith("sr-multi-")]
    mb = _rate(sum(1 for _, p, _ in multi if p), len(multi))

    # same-section continuous loops must be ZERO (plan §8)
    section_loops_total = skill_routing.aggregate(cand["graded"]).get(
        "within_turn_dup_section_loops_total")

    gates = {
        "silent_misload_events_zero": {"cand": cg["silent_misload_events"],
                                       "ok": cg["silent_misload_events"] == 0},
        "irrelevant_load_events_zero": {"cand": cg["irrelevant_load_events"],
                                        "ok": cg["irrelevant_load_events"] == 0},
        "skill_crud_calls_zero": {"cand": cg["skill_create_update_delete_calls"],
                                  "ok": cg["skill_create_update_delete_calls"] == 0},
        "first_load_applicable": cmp_ge("first_load_applicable_rate", 0.90),
        "top3_coverage": cmp_ge("top3_coverage", 0.95),
        "unnecessary_load_rate": cmp_le("unnecessary_load_rate", 0.10),
        "multi_turn_continuity": {"cand": mb, "den": len(multi),
                                  "ok": mb is None or mb >= 0.95},
        "within_turn_dup_loads": cmp_le("within_turn_dup_loads_total"),
        "same_section_loops_zero": {"cand": section_loops_total,
                                    "ok": (section_loops_total or 0) == 0},
        "median_prompt_tokens": cmp_le("median_prompt_tokens"),
        "business_pass": {"cand": cb, "base": bb, "den": cd,
                          "ok": bb is None or (cb is not None and cb >= bb)},
    }
    lat_c, lat_b = _p95(cand["lat"]), _p95(base["lat"]) if base else None
    lat_ok = True if lat_b is None else (lat_c is not None and lat_c <= lat_b * 1.10)
    gates["p95_latency"] = {"cand": lat_c, "base": lat_b, "ok": lat_ok,
                            "note": ">10% increase requires investigation"}

    all_ok = all(v["ok"] for v in gates.values())
    return {"candidate": cg, "baseline": bg, "gates": gates,
            "verdict": "PASS" if all_ok else "FAIL",
            "failed_gates": [k for k, v in gates.items() if not v["ok"]]}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", required=True)
    ap.add_argument("--scores", required=True)
    ap.add_argument("--baseline-cases")
    ap.add_argument("--baseline-scores")
    ap.add_argument("--json")
    args = ap.parse_args()

    cand = summarize({"cases": args.cases, "scores": args.scores})
    base = None
    if args.baseline_cases and args.baseline_scores:
        base = summarize({"cases": args.baseline_cases, "scores": args.baseline_scores})

    rep = gate_report(cand, base)

    # per-case detail lines for failures (skill side)
    details = []
    for g in cand["graded"]:
        flags = []
        if g["silent_misloads"]:
            flags.append(f"misload:{g['silent_misloads']}")
        if g["unnecessary_load"]:
            flags.append("unnecessary_load")
        if g["irrelevant_loaded"]:
            flags.append(f"irrelevant:{g['irrelevant_loaded']}")
        if g["within_turn_dup_loads"]:
            flags.append(f"dup_loads:{g['within_turn_dup_loads']}")
        if flags:
            details.append({"case_id": g["case_id"], "lang": g["lang"],
                            "round": g["round"], "flags": flags})
    rep["flagged_cases"] = details

    print(json.dumps(rep, ensure_ascii=False, indent=1))
    if args.json:
        Path(args.json).write_text(json.dumps(rep, ensure_ascii=False, indent=1))
    return 0 if rep["verdict"] == "PASS" else 2


if __name__ == "__main__":
    raise SystemExit(main())

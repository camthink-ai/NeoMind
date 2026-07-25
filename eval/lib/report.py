"""Aggregate scores.jsonl -> grade-card.md.

Two headlines:
  - **Hard Pass Rate** (PRIMARY, reliable): state_query pass + tool_match,
    broken down by category. From hard_signal.compute() under each score's
    `hard` key.
  - **Soft Judge Grade** (SECONDARY, inflation-prone): weighted dimension
    scores from the LLM judge.

Per project calibration (2026-07-20): trust the hard signals; the judge grade
is soft and reads high. Old runs without a `hard` key degrade gracefully
(hard stats stay empty).
"""
from __future__ import annotations

import json
from collections import defaultdict
from pathlib import Path

import hard_signal

WEIGHTS = {
    "tool_accuracy": 25.0,
    "task_completion": 25.0,
    "response_quality": 20.0,
    "context_retention": 15.0,
    "error_recovery": 15.0,
    "language_adherence": 5.0,
}

ERROR_STATUSES = {
    "agent_error",
    "runtime_error",
    "seed_failure",
    "llm_config_error",
    "agent_timeout",
}


def grade_letter(score: float) -> str:
    if score >= 85:
        return "A"
    if score >= 70:
        return "B"
    if score >= 55:
        return "C"
    if score >= 40:
        return "D"
    return "F"


def aggregate(scores_jsonl: str) -> dict:
    agg = {
        "total_cases": 0,
        "malformed": 0,
        "agent_errors": 0,
        "suspected_fallback": 0,
        "by_dimension": defaultdict(list),
        "by_lang": defaultdict(list),
        "overall_per_case": [],
        # hard signals
        "hard_passed": 0,
        "hard_denom": 0,
        "wrong_tool": 0,
        "unasserted": 0,
        "agent_failed": 0,
        "by_category_hard": defaultdict(
            lambda: {"n": 0, "pass": 0, "wrong": 0, "unasserted": 0}
        ),
    }

    for line in (scores_jsonl or "").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            s = json.loads(line)
        except json.JSONDecodeError:
            agg["malformed"] += 1
            continue

        agg["total_cases"] += 1
        if s.get("suspected_fallback"):
            agg["suspected_fallback"] += 1
        if s.get("status") in ERROR_STATUSES:
            agg["agent_errors"] += 1

        # --- hard signals (primary) ---
        hard = s.get("hard")
        cat = s.get("category") or "?"
        if isinstance(hard, dict):
            if hard.get("agent_failed"):
                agg["agent_failed"] += 1
            elif hard.get("unasserted"):
                agg["unasserted"] += 1
                agg["by_category_hard"][cat]["unasserted"] += 1
            else:
                agg["hard_denom"] += 1
                agg["by_category_hard"][cat]["n"] += 1
                if hard_signal.pass_for_case(hard) is True:
                    agg["hard_passed"] += 1
                    agg["by_category_hard"][cat]["pass"] += 1
                if hard.get("wrong_tool"):
                    agg["wrong_tool"] += 1
                    agg["by_category_hard"][cat]["wrong"] += 1

        # --- soft judge (secondary) ---
        scores = s.get("scores") or {}
        if not isinstance(scores, dict):
            continue
        present = {k: v for k, v in scores.items() if isinstance(v, (int, float))}
        if not present:
            continue
        total_w = sum(WEIGHTS.get(k, 0.0) for k in present)
        case_overall = 0.0
        for k, v in present.items():
            agg["by_dimension"][k].append(float(v))
            if total_w > 0:
                case_overall += float(v) * WEIGHTS.get(k, 0.0) / total_w
        # case_overall is 0-10; convert to 0-100.
        agg["overall_per_case"].append(case_overall * 10.0)
        agg["by_lang"][s.get("lang", "?")].append(case_overall * 10.0)

    return agg


def overall(agg: dict) -> float:
    v = agg["overall_per_case"]
    return sum(v) / len(v) if v else 0.0


def hard_pass_rate(agg: dict) -> dict:
    denom = agg["hard_denom"]
    pct = (100.0 * agg["hard_passed"] / denom) if denom else 0.0
    return {
        "passed": agg["hard_passed"],
        "denom": denom,
        "pct": pct,
        "wrong_tool": agg["wrong_tool"],
        "unasserted": agg["unasserted"],
        "agent_failed": agg["agent_failed"],
    }


def compare(base_agg: dict, new_agg: dict) -> dict:
    """Per-category hard pass-rate delta (new vs base).

    Used by `run_eval.py compare` to spot regressions vs a committed baseline
    (e.g. did SFT/help hurt a category?). Returns base/new overall hard rates
    + one row per category.
    """
    bh = hard_pass_rate(base_agg)
    nh = hard_pass_rate(new_agg)
    cats = sorted(set(base_agg["by_category_hard"]) | set(new_agg["by_category_hard"]))
    rows = []
    for cat in cats:
        b = base_agg["by_category_hard"].get(cat, {"n": 0, "pass": 0})
        n = new_agg["by_category_hard"].get(cat, {"n": 0, "pass": 0})
        rows.append({
            "category": cat,
            "base_pct": (100.0 * b["pass"] / b["n"]) if b["n"] else None,
            "new_pct": (100.0 * n["pass"] / n["n"]) if n["n"] else None,
            "base_n": b["n"],
            "new_n": n["n"],
        })
    return {"base": bh, "new": nh, "rows": rows}


def write_grade_card(agg: dict, out_path: Path):
    md = ["# NeoMind Chat Eval Report", ""]

    # --- PRIMARY: hard pass rate ---
    hp = hard_pass_rate(agg)
    md.append(
        f"## Hard Pass Rate (primary): **{hp['passed']}/{hp['denom']} "
        f"({hp['pct']:.0f}%)**"
    )
    md.append("")
    md.append(
        f"_wrong_tool cases: {hp['wrong_tool']} · unasserted: {hp['unasserted']} "
        f"· agent_failed: {hp['agent_failed']}_"
    )
    md.append("")

    if agg["by_category_hard"]:
        md.append("### Hard Pass by Category")
        md.append("")
        md.append("| Category | Pass / N | wrong_tool | unasserted |")
        md.append("|---|---|---|---|")
        for cat in sorted(agg["by_category_hard"]):
            d = agg["by_category_hard"][cat]
            md.append(
                f"| {cat} | {d['pass']} / {d['n']} | {d['wrong']} | {d['unasserted']} |"
            )
        md.append("")

    # --- SECONDARY: soft judge grade ---
    grade = grade_letter(overall(agg))
    md.append(
        f"## Soft Judge Grade (secondary, inflation-prone): **{grade} "
        f"({overall(agg):.1f})**"
    )
    md.append("")

    denom = agg["total_cases"] + agg["malformed"]
    malformed_rate = agg["malformed"] / denom if denom else 0.0
    if malformed_rate > 0.05:
        md.append(
            f"⚠️ Malformed score lines: {malformed_rate*100:.1f}% — "
            "results may be unreliable."
        )
        md.append("")
    if agg["agent_errors"] > 0:
        md.append(
            f"⚠️ Agent failures: {agg['agent_errors']} case(s) excluded from averages."
        )
        md.append("")

    md.append("| Dimension | Avg (0-10) |")
    md.append("|---|---|")
    dim_avg = {k: sum(v) / len(v) for k, v in agg["by_dimension"].items() if v}
    for dim, avg in dim_avg.items():
        md.append(f"| {dim} | {avg:.2f} |")

    md.append("")
    md.append("## By Language")
    md.append("")
    md.append("| Lang | Cases | Avg (0-100) |")
    md.append("|---|---|---|")
    for lang, vs in agg["by_lang"].items():
        avg = sum(vs) / len(vs) if vs else 0.0
        md.append(f"| {lang} | {len(vs)} | {avg:.1f} |")

    md.append("")
    md.append(f"Suspected fallback cases: {agg['suspected_fallback']}")
    md.append("")

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("\n".join(md))

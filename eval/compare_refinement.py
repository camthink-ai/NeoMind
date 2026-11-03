#!/usr/bin/env python3
"""Compare aligned offline refinements; abstention never counts as a pass."""
import argparse
import collections
import copy
import json
from pathlib import Path

from lib.answer_refinement import delivery_issues, digest
from lib.semantic_evidence import compile_evidence


def unchanged_execution(baseline, candidate):
    other = copy.deepcopy(candidate)
    source = baseline.get("turn_records", [])
    target = other.get("turn_records", [])
    if not source or not target:
        return baseline == candidate
    for key in ("assistant_message", "processing_time_ms", "finish_reason", "generation_complete"):
        if key in source[-1]:
            target[-1][key] = source[-1][key]
        else:
            target[-1].pop(key, None)
    return baseline == other


def compare(attempts, baseline_reviews, candidate_reviews):
    if not len(attempts) == len(baseline_reviews) == len(candidate_reviews) or not attempts:
        raise ValueError("unaligned_or_empty_inputs")
    pairs, transitions, skipped = [], collections.Counter(), collections.Counter()
    failures = 0
    delivery_failures = 0
    for index, (attempt, before, after) in enumerate(zip(attempts, baseline_reviews, candidate_reviews), 1):
        old, new = attempt["baseline"], attempt["candidate"]
        if not unchanged_execution(old, new):
            raise ValueError("tool_or_state_execution_changed")
        for review, record in [(before, old), (after, new)]:
            if review.get("verdict") not in {"PASS", "FAIL", "ABSTAIN"}:
                raise ValueError("invalid_review_verdict")
            if review["verdict"] != "ABSTAIN":
                turn = record["turn_records"][-1]
                if (review.get("answer_sha256") != digest(turn["assistant_message"])
                    or review.get("evidence_sha256") != digest(compile_evidence(record))
                    or review.get("case_id") != record.get("case_id")
                    or review.get("target_turn") != len(record["turn_records"]) - 1):
                    raise ValueError("review_does_not_match_answer")
        if before.get("rubric_sha256") != after.get("rubric_sha256"):
            raise ValueError("judge_rubrics_differ")
        if attempt["status"] == "skipped":
            if old != new:
                raise ValueError("skipped_control_changed")
            skipped[attempt["reason"]] += 1
            continue
        if attempt["status"] not in {"produced", "failed"}:
            raise ValueError("unknown_attempt_status")
        failures += attempt["status"] == "failed"
        transition = before["verdict"] + "->" + after["verdict"]
        issues = delivery_issues(new["turn_records"][-1]["assistant_message"],
                                 new["turn_records"][-1].get("user", ""))
        delivery_failures += bool(issues)
        transitions[transition] += 1
        pairs.append({"line": index, "case_id": old.get("case_id"), "lang": old.get("lang"),
                      "round": old.get("round"), "transition": transition,
                      "refinement_status": attempt["status"], "delivery_issues": issues,
                      "extra_ms": attempt.get("elapsed_ms")})
    gained = sum(p["transition"] == "FAIL->PASS" for p in pairs)
    lost = sum(p["transition"].startswith("PASS->") and p["transition"] != "PASS->PASS" for p in pairs)
    incomplete = sum("ABSTAIN" in p["transition"] for p in pairs)
    return {"total": len(attempts), "eligible_pairs": len(pairs), "skipped_controls": dict(skipped),
            "transitions": dict(transitions), "gained_passes": gained, "lost_passes": lost,
            "incomplete_pairs": incomplete, "failed_refinements": failures,
            "delivery_failures": delivery_failures,
            "screening_promising": bool(pairs) and gained > 0 and lost == 0 and incomplete == 0 and failures == 0 and delivery_failures == 0,
            "production_acceptance": False, "full_regression_score": None,
            "limitation": "Offline paired screening; execution inherited, judge configuration must be frozen by the runner; no statistical/generalization claim.",
            "pairs": pairs}


def read(path):
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--attempts", required=True, type=Path)
    parser.add_argument("--baseline-reviews", required=True, type=Path)
    parser.add_argument("--candidate-reviews", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    manifests = [json.loads((p.parent / "manifest.json").read_text())
                 for p in (args.baseline_reviews, args.candidate_reviews)]
    if (not manifests[0].get("judge") or manifests[0].get("judge") != manifests[1].get("judge")
        or manifests[0].get("source_sha256") != manifests[1].get("source_sha256")):
        raise ValueError("review_configuration_or_implementation_differs")
    result = compare(read(args.attempts), read(args.baseline_reviews), read(args.candidate_reviews))
    with args.output.open("x") as file:
        file.write(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps(result, ensure_ascii=False), flush=True)
    return 0 if result["screening_promising"] else 2


if __name__ == "__main__":
    raise SystemExit(main())

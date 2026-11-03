#!/usr/bin/env python3
"""Calibrated, blinded paired review of offline refinement attempts."""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import getpass
import hashlib
import json
import os
from pathlib import Path
import warnings

from compare_refinement import compare
from lib.semantic_review import OpenAICompatibleJudge, review_record
from lib.answer_refinement import delivery_issues


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--attempts", required=True, type=Path)
    parser.add_argument("--calibration-dir", required=True, type=Path)
    parser.add_argument("--run-dir", required=True, type=Path)
    parser.add_argument("--judge-url", required=True)
    parser.add_argument("--judge-model", required=True)
    parser.add_argument("--ask-key", action="store_true")
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--workers", type=int, choices=[1, 2, 3, 4], default=2)
    parser.add_argument("--review-rejected", action="store_true", help="Diagnose already rejected candidates, at extra judge cost")
    args = parser.parse_args(argv)
    attempts = [json.loads(l) for l in args.attempts.read_text().splitlines() if l.strip()]
    if not attempts:
        parser.error("empty attempts")
    blockers = []
    for index, attempt in enumerate(attempts, 1):
        if attempt["status"] == "failed":
            blockers.append({"line": index, "reason": attempt["reason"]})
        elif attempt["status"] == "produced":
            turn = attempt["candidate"]["turn_records"][-1]
            issues = delivery_issues(turn["assistant_message"], turn.get("user", ""))
            if issues:
                blockers.append({"line": index, "reason": "delivery_checks_failed", "issues": issues})
    if not any(a["status"] == "produced" for a in attempts):
        blockers.append({"reason": "no_produced_candidates"})
    if blockers and not args.review_rejected:
        args.run_dir.mkdir(parents=True, exist_ok=False)
        result = {"screening_promising": False, "production_acceptance": False,
                  "semantic_evaluation": "not_run; failed earlier gates", "model_judge_calls": 0,
                  "blockers": blockers,
                  "attempts_sha256": hashlib.sha256(args.attempts.read_bytes()).hexdigest(),
                  "policy_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
        (args.run_dir / "summary.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result), flush=True)
        return 2
    key = os.getenv("NEOMIND_SEMANTIC_JUDGE_API_KEY")
    if args.ask_key:
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                key = getpass.getpass("Semantic judge API key (not stored): ")
        except getpass.GetPassWarning:
            parser.error("secure terminal unavailable")
    judge = OpenAICompatibleJudge(args.judge_url, args.judge_model, key, timeout=args.timeout)
    calibration = json.loads((args.calibration_dir / "summary.json").read_text())
    frozen = json.loads((args.calibration_dir / "manifest.json").read_text())
    if calibration.get("calibration_accepted") is not True:
        parser.error("reference calibration was not accepted")
    # Timeout affects availability, not the rubric or sampling. Record it explicitly.
    if {k: v for k, v in frozen["judge"].items() if k != "timeout"} != {
        k: v for k, v in judge.configuration.items() if k != "timeout"
    }:
        parser.error("judge configuration differs from calibration")
    source = Path(__file__).parent
    for name in ("lib/semantic_review.py", "lib/semantic_evidence.py"):
        if hashlib.sha256((source / name).read_bytes()).hexdigest() != frozen["source_sha256"].get(name):
            parser.error("review implementation differs from calibration")
    args.run_dir.mkdir(parents=True, exist_ok=False)
    paths = [Path(__file__), source / "semantic_review.py", source / "lib/semantic_review.py",
             source / "lib/semantic_evidence.py", source / "compare_refinement.py"]
    manifest = {"judge": judge.configuration, "workers": args.workers,
                "source_sha256": {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
                "attempts_sha256": hashlib.sha256(args.attempts.read_bytes()).hexdigest(),
                "calibration_manifest_sha256": hashlib.sha256((args.calibration_dir / "manifest.json").read_bytes()).hexdigest(),
                "calibration_summary_sha256": hashlib.sha256((args.calibration_dir / "summary.json").read_bytes()).hexdigest(),
                "calibration_timeout": frozen["judge"]["timeout"],
                "blinding": "No side label or grading feedback is passed to the judge or generator.",
                "calibration_limitation": "Authored reference cases, not independent human validation."}
    for side in ("baseline", "candidate"):
        dest = args.run_dir / side; dest.mkdir()
        (dest / "manifest.json").write_text(json.dumps(manifest, indent=2))
    # Preserve exact tested implementation without credentials or environment dumps.
    for p in paths:
        dest = args.run_dir / "implementation" / p.relative_to(source)
        dest.parent.mkdir(parents=True, exist_ok=True); dest.write_bytes(p.read_bytes())
    rows = {side: [None] * len(attempts) for side in ("baseline", "candidate")}
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        pending = {}
        for i, attempt in enumerate(attempts):
            sides = ("baseline", "candidate") if i % 2 == 0 else ("candidate", "baseline")
            for side in sides:
                pending[pool.submit(review_record, attempt[side], judge)] = (i, side)
        for future in as_completed(pending):
            i, side = pending[future]; result = future.result()
            rows[side][i] = result
            (args.run_dir / side / f"{i + 1:04}.json").write_text(json.dumps(result, ensure_ascii=False))
            print(i + 1, side, result["verdict"], result["reason"], flush=True)
    for side, values in rows.items():
        (args.run_dir / side / "reviews.jsonl").write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in values))
    result = compare(attempts, rows["baseline"], rows["candidate"])
    (args.run_dir / "summary.json").write_text(json.dumps(result, ensure_ascii=False, indent=2))
    print(json.dumps(result, ensure_ascii=False), flush=True)
    return 0 if result["screening_promising"] else 2


if __name__ == "__main__":
    raise SystemExit(main())

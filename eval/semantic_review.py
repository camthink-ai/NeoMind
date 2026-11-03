#!/usr/bin/env python3
"""Opt-in semantic review/calibration. Original records and hard scores are read-only."""
from __future__ import annotations

import argparse
import collections
import datetime
import getpass
import hashlib
import json
import os
from pathlib import Path
import sys
import warnings

from lib.semantic_review import OpenAICompatibleJudge, review_record


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--calibration", type=Path, help="Human-labelled calibration JSON")
    source.add_argument("--records", type=Path, help="Captured cases.jsonl (requires completion observability)")
    parser.add_argument("--run-dir", required=True, type=Path, help="New output directory; never overwrite")
    parser.add_argument("--judge-url", default=os.getenv("NEOMIND_SEMANTIC_JUDGE_URL"))
    parser.add_argument("--judge-model", default=os.getenv("NEOMIND_SEMANTIC_JUDGE_MODEL"))
    parser.add_argument("--ask-key", action="store_true", help="Read key without terminal echo; never store it")
    parser.add_argument("--max-tokens", type=int, default=8192)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--turn-index", type=int)
    args = parser.parse_args(argv)
    if bool(args.judge_url) != bool(args.judge_model):
        parser.error("provide both judge-url and judge-model, or neither")
    key = os.getenv("NEOMIND_SEMANTIC_JUDGE_API_KEY")
    if args.ask_key:
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("error", getpass.GetPassWarning)
                key = getpass.getpass("Semantic judge API key (not stored): ")
        except getpass.GetPassWarning:
            parser.error("secure terminal unavailable; configure the runtime key or use a remote TTY")
    judge = (OpenAICompatibleJudge(args.judge_url, args.judge_model, key,
                                  args.max_tokens, args.timeout) if args.judge_url else None)
    input_path = args.calibration or args.records
    if args.calibration:
        data = json.loads(input_path.read_text())
        examples = data.get("examples")
        if data.get("schema_version") != 1 or not isinstance(examples, list) or not examples:
            parser.error("invalid calibration schema")
        ids = set()
        for example in examples:
            if (not isinstance(example, dict) or not isinstance(example.get("id"), str)
                or example["id"] in ids or example.get("expected") not in {"PASS", "FAIL", "ABSTAIN"}
                or not isinstance(example.get("case_record"), dict)):
                parser.error("invalid calibration example")
            ids.add(example["id"])
        work = [(e["id"], e["case_record"], e["expected"]) for e in examples]
    else:
        work = []
        for index, line in enumerate(input_path.read_text().splitlines(), 1):
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except ValueError:
                record = None
            work.append((f"line-{index}", record, None))
    args.run_dir.mkdir(parents=True, exist_ok=False)
    manifest = {"schema_version": 1, "created_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "input_path": str(input_path.resolve()), "input_sha256": sha(input_path),
        "judge": judge.configuration if judge else None,
        "source_sha256": {str(p.relative_to(Path(__file__).parent)): sha(p) for p in [
            Path(__file__), Path(__file__).parent / "lib/semantic_review.py",
            Path(__file__).parent / "lib/semantic_evidence.py"]},
        "original_hard_grading_changed": False,
        "limitation": "LLM entailment judgment needs independent calibration; authentic citations are not a proof of entailment."}
    (args.run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))
    results = []
    unavailable = False
    with (args.run_dir / "reviews.jsonl").open("x") as output:
        for example_id, record, expected in work:
            result = review_record(record, None if unavailable else judge, turn_index=args.turn_index)
            if unavailable and result["reason"] == "judge_unconfigured":
                result["reason"] = "skipped_after_judge_unavailable"
            if result["reason"] == "judge_unavailable":
                unavailable = True
            result["example_id"] = example_id
            if expected is not None:
                result["expected"] = expected
                result["matches_reference"] = result["verdict"] == expected
            output.write(json.dumps(result, ensure_ascii=False) + "\n")
            output.flush()
            results.append(result)
            print(example_id, result["verdict"], result["reason"], flush=True)
    counts = dict(collections.Counter(r["verdict"] for r in results))
    summary = {"total": len(results), "verdict_counts": counts,
               "abstention_reasons": dict(collections.Counter(r["reason"] for r in results if r["verdict"] == "ABSTAIN"))}
    if args.calibration:
        reference = {r["expected"] for r in results}
        accepted = (judge is not None and {"PASS", "FAIL"}.issubset(reference)
                    and bool(results) and all(r["matches_reference"] for r in results))
        summary.update(calibration_accepted=accepted,
                       reference_matches=sum(r["matches_reference"] for r in results),
                       mismatched_ids=[r["example_id"] for r in results if not r["matches_reference"]])
        exit_code = 0 if accepted else 2
    else:
        exit_code = 1 if counts.get("FAIL") else (0 if results and counts.get("PASS") == len(results) else 2)
    (args.run_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary), flush=True)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())

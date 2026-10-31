#!/usr/bin/env python3
"""Re-score exported evidence with the repository's diagnostic/product verdicts.

Usage: python eval/audit_recovery_scores.py scores.jsonl [scores.jsonl ...]
Rows are never silently deduplicated: repeated rounds remain separate trials.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
from pathlib import Path

from lib.hard_signal import pass_for_case, pass_for_case_product


def audit(path: Path) -> dict:
    records = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    counts = Counter((r["case_id"], r["lang"]) for r in records)
    results = {"path": str(path), "trials": len(records), "unique_cases": len(counts),
               "trials_per_case": dict(Counter(counts.values()))}
    for name, verdict in [("diagnostic", pass_for_case), ("product", pass_for_case_product)]:
        verdicts = [verdict(row["hard"]) for row in records]
        passed = sum(value is True for value in verdicts)
        failed = sum(value is False for value in verdicts)
        results[name] = {"passed": passed, "failed": failed,
                         "unasserted_or_excluded": len(verdicts) - passed - failed,
                         "rate": passed / (passed + failed) if passed + failed else None}
    results["legacy_cmd_and_state_passed"] = sum(
        r["hard"].get("cmd_ok") is True and r["hard"].get("sq_all_pass") is not False
        for r in records
    )
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scores", type=Path, nargs="+")
    args = parser.parse_args()
    for score in args.scores:
        print(json.dumps(audit(score), ensure_ascii=False))

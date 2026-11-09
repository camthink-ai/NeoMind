"""Independent relabeler: reclassify mislabeled cap-timeouts in ARCHIVED runs.

2026-11-09 finding: the per-case SIGALRM ``_CaseTimeout`` raised inside a turn
was swallowed by the turn-level generic handler and recorded as
``agent_error``. This tool re-reads the raw records and, wherever the message
carries ``_CaseTimeout``, counts the trial as ``timeout`` in a DERIVED
summary. Originals are never modified.

Works on both layouts:
  - classic regression run dir  (cases.jsonl + scores.jsonl side by side)
  - batch-controller gate dirs  (<root>/gate/<arm>/trials/*/cases.jsonl)

Usage:
  python3 eval/lib/relabel_timeouts.py <dir> [--root-name label]
Multiple dirs may be given (each gate arm dir, or several run dirs).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def iter_case_records(root: Path):
    """Yield (source, case_id, lang, status, message, result_status).

    The RAW cases.jsonl record is the PRIMARY status source (2026-11-10
    review: keying off result.json alone silently drops trials whose result
    file is missing, even when the raw record clearly shows agent_error +
    _CaseTimeout). result_status (controller layout only) is carried alongside
    for explicit conflict reporting — never silently preferred.
    """
    cases = root / "cases.jsonl"
    if cases.exists():  # classic layout: pair with scores.jsonl by line order
        for line in cases.read_text().splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            yield (str(cases), rec.get("case_id"), rec.get("lang"),
                   rec.get("status"), rec.get("message") or "", None)
        return
    for f in sorted(root.glob("trials/*/cases.jsonl")):  # controller layout
        result_file = f.parent / "result.json"
        res_status = None
        if result_file.exists():
            try:
                res_status = json.loads(result_file.read_text()).get("status")
            except Exception:  # noqa: BLE001 — corrupt result must not hide raw status
                res_status = None
        for line in f.read_text().splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            yield (str(f), rec.get("case_id"), rec.get("lang"),
                   rec.get("status"), rec.get("message") or "", res_status)


def relabel(root: Path, root_name: str) -> dict:
    n = agent_err = corrected = 0
    conflict_count = 0
    conflicts = []
    for _src, cid, lang, status, message, res_status in iter_case_records(root):
        n += 1
        if status and res_status and status != res_status:
            conflict_count += 1
            if len(conflicts) < 5:  # list is capped; the count never is
                conflicts.append({"key": f"{cid}[{lang}]",
                                  "raw": status, "result": res_status})
        if status == "agent_error":
            agent_err += 1
            if "_CaseTimeout" in message:
                corrected += 1
    return {"root": root_name, "trials": n, "agent_error_total": agent_err,
            "relabelled_to_timeout": corrected,
            "true_agent_error_remaining": agent_err - corrected,
            "status_conflicts": conflicts,
            "status_conflict_count": conflict_count}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, default=None,
                    help="write combined JSON here (default: <first-dir>/relabel-corrected.json)")
    args = ap.parse_args(argv)
    rows = []
    for d in args.dirs:
        if not d.exists():
            print(f"skip (missing): {d}", file=sys.stderr)
            continue
        rows.append(relabel(d, str(d)))
    for r in rows:
        print(f"{r['root']}: trials={r['trials']} agent_error={r['agent_error_total']} "
              f"-> timeout(relabeled)={r['relabelled_to_timeout']} "
              f"remaining_true_agent_error={r['true_agent_error_remaining']}")
    out = args.out or (args.dirs[0].parent / "relabel-corrected.json")
    out.write_text(json.dumps(rows, ensure_ascii=False, indent=2))
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

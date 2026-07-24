"""Batch-collect teacher traces for SFT — resumable.

Runs the teacher (configured via AGENT_LLM_* env) over eval cases, saving one
CaseRecord per case + per-case LLM traces (via NEOMIND_TRACE_ROOT, which
``server.py`` routes into ``<root>/<case_id>/``). Output layout is exactly
what ``eval.sft.curate`` consumes.

Resumable: skips any case whose ``<case_id>.json`` already exists in
--records-dir, so an interrupted run (or a flaky teacher) just continues.

Env required (same as run_eval):
  AGENT_LLM_BACKEND_TYPE / AGENT_LLM_ENDPOINT / AGENT_LLM_MODEL / AGENT_LLM_API_KEY
  AGENT_LLM_THINKING=false            (we train a no-thinking student)
  NEOMIND_TRACE_ROOT=<dir>            (per-case trace subdirs)
  NEOMIND_TEST_BIN=<release neomind>  (avoid stale PATH binary)
  NEOMIND_SKIP_STALE_CHECK=1          (optional)

Usage:
  .venv/bin/python -m eval.sft.collect --lang zh --records-dir data/sft/zh \\
      --trace-root data/sft/traces-zh [--limit 10] [--filter device]
  .venv/bin/python -m eval.sft.collect --lang en --records-dir data/sft/en \\
      --trace-root data/sft/traces-en   # resume: re-run same command
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import traceback
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO))

REQUIRED_ENV = ["AGENT_LLM_BACKEND_TYPE", "AGENT_LLM_ENDPOINT", "AGENT_LLM_MODEL", "AGENT_LLM_API_KEY"]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Batch-collect teacher traces for SFT (resumable)")
    ap.add_argument("--lang", required=True, choices=["zh", "en"])
    ap.add_argument("--records-dir", type=Path, required=True, help="output <case_id>.json CaseRecords")
    ap.add_argument("--trace-root", type=Path, required=True, help="NEOMIND_TRACE_ROOT (per-case subdirs)")
    ap.add_argument("--root", type=Path, default=REPO / "eval" / "cases", help="eval cases root")
    ap.add_argument("--limit", type=int, default=0, help="stop after N NEW cases (0 = all)")
    ap.add_argument("--retries", type=int, default=3,
                    help="retry a case until it has no FAILED state_query (teacher is flaky). "
                         "Keeps the LAST attempt's record+trace together (consistent).")
    ap.add_argument("--filter", default="", help="substring filter on case file path (e.g. 'device')")
    args = ap.parse_args(argv)

    missing = [v for v in REQUIRED_ENV if not os.environ.get(v)]
    if missing:
        print(f"ERROR: missing env: {missing}", file=sys.stderr)
        return 2
    if not os.environ.get("NEOMIND_TRACE_ROOT"):
        # export what we pass on the CLI so server.py sees it. Resolve to
        # absolute: the neomind subprocess runs with CWD=tmpdir, so a relative
        # trace root would route traces into the (deleted) tmpdir.
        os.environ["NEOMIND_TRACE_ROOT"] = str(args.trace_root.resolve())
    args.records_dir.mkdir(parents=True, exist_ok=True)
    args.trace_root.mkdir(parents=True, exist_ok=True)

    from eval import run_eval  # import after sys.path setup

    cases = run_eval._select_cases(args.root, args.lang, None, None)
    if args.filter:
        cases = [c for c in cases if args.filter in str(c)]
    print(f"{len(cases)} {args.lang} cases; records -> {args.records_dir}", file=sys.stderr)

    done = skipped = failed = 0
    t0 = time.monotonic()
    for i, case_path in enumerate(cases, 1):
        try:
            case_id = run_eval._load_case(case_path).get("id") or case_path.stem
        except Exception:  # noqa: BLE001
            case_id = case_path.stem
        out = args.records_dir / f"{case_id}.json"
        if out.exists():
            skipped += 1
            continue
        if args.limit and done >= args.limit:
            print(f"hit --limit {args.limit}, stopping", file=sys.stderr)
            break

        elapsed = int(time.monotonic() - t0)
        print(f"[{i}/{len(cases)}] {case_id} (new={done} skip={skipped} fail={failed} {elapsed}s)", file=sys.stderr)

        # Retry until no FAILED state_query (teacher is flaky run-to-run).
        # The trace hook APPENDS, so clear the case trace before each attempt
        # and always keep the LAST attempt's record — its trace is the one on
        # disk, so record and trace stay consistent.
        trace_file = Path(os.environ["NEOMIND_TRACE_ROOT"]) / case_id / "anthropic_trace.jsonl"
        best_cr = None
        for attempt in range(1, args.retries + 1):
            if trace_file.exists():
                trace_file.unlink()
            try:
                cr = run_eval.run_case(str(case_path))
                cr.setdefault("lang", args.lang)
                best_cr = cr  # latest attempt; its trace is now on disk
                # Retry on ANY non-golden outcome (not just state_query_failed)
                # — covers non_ideal_tools / too_many_calls / dup_retry too.
                from eval.sft.curate import case_looks_clean
                if case_looks_clean(cr):
                    break  # clean — keep this attempt
                print(f"  attempt {attempt}/{args.retries}: not clean, retry",
                      file=sys.stderr)
            except Exception as e:  # noqa: BLE001
                failed += 1
                print(f"  attempt {attempt}/{args.retries}: {type(e).__name__}: {e}",
                      file=sys.stderr)
                traceback.print_exc(file=sys.stderr)
                best_cr = {  # stub; curate scores it non-golden
                    "case_id": case_id, "lang": args.lang, "turn_records": [],
                    "state_queries": [], "status": "collect_error",
                    "error_type": type(e).__name__, "message": str(e)}
                break  # structural error — retrying won't help

        out.write_text(json.dumps(best_cr, ensure_ascii=False))
        done += 1

    print(f"\ndone: {done} new, {skipped} skipped, {failed} failed "
          f"in {int(time.monotonic()-t0)}s", file=sys.stderr)
    print(f"next: .venv/bin/python -m eval.sft.curate {args.records_dir} "
          f"--trace-root {args.trace_root} --lang {args.lang}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

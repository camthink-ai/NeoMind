#!/usr/bin/env python3
"""Print which parts of the case library the gate actually exercises.

The gate is 47 cases out of ~168 scenarios; knowing which categories fall out
is the difference between "we tested it" and "we tested the part we always
test". Prints per-category counts (library vs gate) and lists workflows with no
gate coverage. Informational — exits non-zero only if a set entry is missing
from the library (drift that would silently shrink the gate at run time).

Usage:
    python3 eval/gate_coverage.py
    python3 eval/gate_coverage.py --set eval/regression_set_vision.txt
"""
from __future__ import annotations

import argparse
import glob
import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load_library() -> dict[str, dict]:
    index: dict[str, dict] = {}
    for f in glob.glob(str(HERE / "cases" / "**" / "*.json"), recursive=True):
        d = json.load(open(f))
        index[d["id"]] = {"category": d.get("category"), "workflow": d.get("workflow"),
                          "lang": d.get("lang")}
    return index


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--set", default=str(HERE / "regression_set.txt"))
    args = ap.parse_args()

    library = load_library()
    ids = [l.strip() for l in Path(args.set).read_text().splitlines() if l.strip()]
    missing = [i for i in ids if i not in library]
    if missing:
        print(f"set entries with no case: {missing}", file=sys.stderr)
        return 1

    # Unique scenarios (an id exists in both zh and en; count once).
    scenarios: dict[str, dict] = {}
    for cid, meta in library.items():
        scenarios.setdefault(cid, meta)

    lib_by_cat = Counter(m["category"] for m in scenarios.values())
    gate_by_cat = Counter(scenarios[i]["category"] for i in ids)
    lib_wf = defaultdict(set)
    for m in scenarios.values():
        lib_wf[m["category"]].add(m["workflow"])
    gate_wf = defaultdict(set)
    for i in ids:
        gate_wf[scenarios[i]["category"]].add(scenarios[i]["workflow"])

    print(f"gate: {len(ids)} cases / library: {len(scenarios)} scenarios\n")
    print(f"{'category':14s} {'gate':>5s} {'library':>8s}  workflows in gate / library")
    for cat in sorted(lib_by_cat, key=lambda c: -lib_by_cat[c]):
        gw, lw = gate_wf.get(cat, set()), lib_wf[cat]
        print(f"{cat:14s} {gate_by_cat.get(cat,0):5d} {lib_by_cat[cat]:8d}  "
              f"{len(gw)}/{len(lw)}  {','.join(sorted(gw))}")

    uncovered = [(c, w) for c in lib_wf for w in lib_wf[c] if w not in gate_wf.get(c, set())]
    # Cases that exist in the library but were deliberately kept OUT of the
    # gate (too slow, external-network, or informational) — so "0 in gate"
    # reads as a decision, not an oversight.
    DELIBERATE = {
        ("tools", "tool-use"),            # vision/image-edit: separate vision set
        ("tools", "tools-web"),           # fetches the real internet
        ("extension", "extension"),       # market cases: requires_network
        ("system", "system-info"),        # trivial single query
        ("system", "onboarding"),         # wizard flow, UI-shaped
        ("rule", "rule-discovery"),       # informational read
        ("device", "device-management"),  # covered via fleet + update cases
        ("scenario", "scenario-transform-pipeline"),
        ("scenario", "scenario-transform-testcode"),
        ("agent", "agent-management"),    # full-lifecycle landed 2026-10-26
        ("widget", "widget"),             # authoring flow; lifecycle is gated
    }
    delib = [f"{c}/{w}" for c, w in uncovered if (c, w) in DELIBERATE]
    pend = [f"{c}/{w}" for c, w in uncovered if (c, w) not in DELIBERATE]
    print(f"\nworkflows with no gate case: {len(uncovered)} "
          f"({len(delib)} deliberate / {len(pend)} to-triage)")
    if delib:
        print("  deliberate (kept out on purpose):")
        for x in sorted(delib):
            print(f"    {x}")
    if pend:
        print("  to-triage (candidates for future gate rounds):")
        for x in sorted(pend):
            print(f"    {x}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

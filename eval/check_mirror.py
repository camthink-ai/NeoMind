#!/usr/bin/env python3
"""Assert zh/en case mirrors are structurally identical.

The case library is bilingual by design: one scenario, two language variants,
same assertions. Six cases had drifted by 2026-10 (en-only state_queries,
zh-only expect blocks) — with `--lang both` now the gate default, that drift
silently made zh a weaker test than en (or vice versa). This check fails the
PR when a mirror pair diverges on anything that affects the verdict:

    - presence/shape of `expect` (tools, commands, substrings)
    - number of `state_queries` (and their types + params + expected)
    - number of turns, scenario_type, category, requires_multimodal
    - seeded extras device/rule/agent counts (fixture divergence)

Deliberately NOT compared: natural-language fields (description, turns[].user,
expectations prose), `response_contains` needles may legitimately differ by
language — but their COUNT is compared.

Usage: python3 eval/check_mirror.py [--root eval/cases]
Exit 1 with a per-case report on any mismatch.
"""
from __future__ import annotations

import argparse
import glob
import json
import sys
from pathlib import Path

VERDICT_KEYS = ("scenario_type", "category", "requires_multimodal")


def load_by_lang(root: Path) -> tuple[dict[str, dict], dict[str, dict]]:
    libs: dict[str, dict[str, dict]] = {"en": {}, "zh": {}}
    for lang in libs:
        for f in glob.glob(str(root / lang / "**" / "*.json"), recursive=True):
            d = json.load(open(f))
            libs[lang][d["id"]] = d
    return libs["en"], libs["zh"]


def _case_text(d: dict) -> str:
    parts = [d.get("description") or ""]
    for t in d.get("turns") or []:
        parts.append(t.get("user") or "")
    for t in (d.get("expectations") or {}).get("per_turn") or []:
        parts.append(t if isinstance(t, str) else "")
    return " ".join(parts)


def _norm_sq(sq: list, case_text: str) -> list:
    """Fingerprint state queries with language-specific entity names masked.

    A mirror pair legitimately differs when each language's turn names its own
    entity (`Battery Patrol` en / `温度巡检` zh) — each side's assertion matches
    its own turn, which is correct. A param value that appears verbatim in the
    case's own text is masked to `<named-in-case>`; values NOT in the text are
    compared literally (that's real drift).

    `response_contains` needles are language-bound by nature (each variant
    asserts a phrase its own language's model should say) — only the TYPE is
    fingerprinted, never the needle value.
    """
    out = []
    for q in sq:
        if q.get("type") == "response_contains":
            out.append(("response_contains", "<lang-bound>", "<lang-bound>"))
            continue
        params = dict(q.get("params") or {})
        for k, v in list(params.items()):
            if isinstance(v, str) and v and v in case_text:
                params[k] = "<named-in-case>"
        expected = q.get("expected")
        if expected is None:
            expected = q.get("expected_min")
        if isinstance(expected, str) and expected and expected in case_text:
            expected = "<named-in-case>"
        out.append((q.get("type"), json.dumps(params, sort_keys=True),
                    json.dumps(expected, sort_keys=True)))
    return sorted(out)


def fingerprint(d: dict) -> dict:
    exp = d.get("expect") or {}
    text = _case_text(d)
    sq_raw = sorted(
        (q.get("type"), json.dumps(q.get("params") or {}, sort_keys=True),
         json.dumps(q.get("expected") if q.get("expected") is not None
                    else q.get("expected_min"), sort_keys=True))
        for q in (d.get("state_queries") or [])
    )
    return {
        "expect_tools": sorted(exp.get("tools") or []),
        "expect_commands": sorted(
            json.dumps(c, sort_keys=True) for c in (exp.get("commands") or [])
        ),
        "expect_substrings": sorted(exp.get("substrings") or []),
        "sq_raw": sq_raw,
        "sq_masked": _norm_sq(d.get("state_queries") or [], text),
        "n_turns": len(d.get("turns") or []),
        "keys": {k: d.get(k) for k in VERDICT_KEYS},
        "extras_scale": {
            k: len(v) for k, v in (d.get("setup", {}).get("extras") or {}).items()
            if isinstance(v, list)
        },
    }


def sq_equal(fe: dict, fz: dict) -> bool:
    """State queries equal if RAW params match (identical assertion) OR the
    name-masked fingerprints match (each language legitimately names its own
    entities). Masking only one side of an otherwise identical pair (the id
    literal appears in one language's prose but not the other's) is not drift.
    """
    return fe["sq_raw"] == fz["sq_raw"] or fe["sq_masked"] == fz["sq_masked"]


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=str(Path(__file__).resolve().parent / "cases"))
    args = ap.parse_args()
    root = Path(args.root)

    en, zh = load_by_lang(root)
    only_en, only_zh = set(en) - set(zh), set(zh) - set(en)
    problems: list[str] = []
    for cid in sorted(only_en):
        problems.append(f"{cid}: en-only (no zh mirror)")
    for cid in sorted(only_zh):
        problems.append(f"{cid}: zh-only (no en mirror)")

    drifted = []
    for cid in sorted(set(en) & set(zh)):
        fe, fz = fingerprint(en[cid]), fingerprint(zh[cid])
        diffs = [k for k in fe if fe[k] != fz[k]]
        if "sq_raw" in diffs or "sq_masked" in diffs:
            if sq_equal(fe, fz):
                diffs = [k for k in diffs if k not in ("sq_raw", "sq_masked")]
        if diffs:
            drifted.append(cid)
            problems.append(f"{cid}: mirror drift on {diffs}")

    print(f"mirrors: {len(set(en) & set(zh))} shared, "
          f"{len(only_en)} en-only, {len(only_zh)} zh-only, "
          f"{len(drifted)} drifted")
    if problems:
        print("\nmirror violations:")
        for p in problems:
            print(f"  {p}")
        return 1
    print("all mirrors structurally identical")
    return 0


if __name__ == "__main__":
    sys.exit(main())

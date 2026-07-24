"""Curate NeoMind teacher traces into a golden SFT dataset.

Applies the 4-criteria "golden" bar to each CaseRecord + trace, then reports
coverage so we can guarantee the dataset is complete + correct before SFT.

Golden bar (all must hold — see memory: minicpm5-neomind-baseline):
  1. OUTCOME     — NO failed state_query. Cases without assertions (list /
                   control / query scenarios have no state change to assert)
                   pass this check by default — they're still correct-workflow
                   demos. Pass --strict-outcome to also require an assertion
                   exists (stronger guarantee, less coverage).
  2. TOOL SELECT — tools used ⊆ allowed_tools(domain). Default {shell, skill}
                   (the ideal NeoMind workflow). Other tools are the student's
                   known failure mode — reject unless the domain genuinely
                   needs them (override via TOOL_ALLOWLIST).
  3. CONVERGENCE — ≤ MAX_TOOL_CALLS tool calls AND no repeated identical
                   (name+args) call (no thrashing).
  4. FIDELITY    — trace carries a non-empty system prompt (byte-faithful
                   per-round system is the trace's whole job).

Coverage report:
  - (domain × verb × lang) cells with ≥1 golden trace, vs gaps (0 golden).
  - per-tool demo count (are all 7 agent tools correctly demonstrated?).

Input layout (produced by a batch run with NEOMIND_TRACE_ROOT set, one
`run-case` CaseRecord per case):
  <records_dir>/<case_id>.json       CaseRecord JSON
  <trace_root>/<case_id>/anthropic_trace.jsonl
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

# NeoMind CRUD is done via the `shell` tool running `neomind <domain> <verb>`.
# The ideal workflow is: skill load <domain-guide> -> shell neomind ... .
# These are the only tools a correct CRUD trace should touch.
DEFAULT_ALLOWED_TOOLS = {"shell", "skill"}

# Domains whose entire purpose IS a non-shell tool -> allow that tool too.
# The `tools` eval category exercises file_edit/file_write/memory/web_fetch/
# image_edit directly — those are CORRECT uses there (would otherwise be
# rejected by the {shell, skill} default and leave 5 tools with zero demos).
TOOL_NATIVE_DOMAINS: dict[str, set[str]] = {
    "tools": {"file_edit", "file_write", "memory", "web_fetch", "image_edit"},
}

MAX_TOOL_CALLS = 8  # clean convergence ceiling; thrashing traces exceed this

# Known business domains (eval case categories + CLI commands).
DOMAINS = {
    "device", "rule", "agent", "dashboard", "transform", "message", "push",
    "widget", "extension", "connector", "llm", "settings", "system", "tools",
}


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def _domain_of(case_id: str) -> str:
    """Infer the business domain from a case_id like 'device-create-camera'."""
    for seg in case_id.split("-"):
        if seg in DOMAINS:
            return seg
    # fall back to the longest known-domain-prefix match (e.g. agent-configure)
    for d in sorted(DOMAINS, key=len, reverse=True):
        if case_id.startswith(d):
            return d
    return case_id.split("-")[0] if case_id else "?"


def _allowed_tools(domain: str) -> set[str]:
    return DEFAULT_ALLOWED_TOOLS | TOOL_NATIVE_DOMAINS.get(domain, set())


def _iter_tool_calls(case_record: dict):
    """Yield each tool_call (name, arguments) the model made, in order.

    Uses ``raw_messages[].tool_calls`` only — it's the authoritative, ordered
    conversation. ``turn_records[].tool_calls`` is an aggregate that DUPLICATES
    the raw_messages entries, so iterating both double-counts every call and
    falsely trips the duplicate-retry check.
    """
    for tr in case_record.get("turn_records", []):
        for m in tr.get("raw_messages", []):
            for tc in (m.get("tool_calls") or []):
                yield tc


def _parse_shell_domain_verb(command: str) -> tuple[str, str] | None:
    """`neomind device create --name ...` -> ('device', 'create')."""
    m = re.search(r"\bneomind\s+(\S+)\s+(\S+)", command or "")
    if not m:
        return None
    return m.group(1), m.group(2)


def case_looks_clean(case_record: dict, *, max_tool_calls: int = MAX_TOOL_CALLS) -> bool:
    """The golden bar MINUS the trace-fidelity check (which needs the trace).

    Used by ``collect``'s retry loop: a teacher attempt is worth keeping (stop
    retrying) when it has no failed state_query, stays within the ideal tool
    set for its domain, converges within the call ceiling, and doesn't repeat
    an identical call. Retrying past a clean attempt wastes teacher calls.
    """
    sqs = case_record.get("state_queries") or []
    if any(not q.get("passed") for q in sqs):
        return False
    allowed = _allowed_tools(_domain_of(case_record.get("case_id") or "?"))
    seen: list[str] = []
    n = 0
    for tc in _iter_tool_calls(case_record):
        n += 1
        if (tc.get("name") or "") not in allowed:
            return False
        sig = f"{tc.get('name')}:{json.dumps(tc.get('arguments', {}), sort_keys=True, ensure_ascii=False)}"
        if sig in seen:
            return False
        seen.append(sig)
    return n <= max_tool_calls


def score_case(case_record: dict, trace_lines: list[dict], *, lang: str = "?",
               strict_outcome: bool = False) -> dict:
    """Apply the golden bar. Returns a score dict (golden: bool + reasons + signals)."""
    case_id = case_record.get("case_id") or "?"
    domain = _domain_of(case_id)

    # 1. OUTCOME — reject only on FAILED assertions. No-assertion cases (list/
    #    control/query) pass by default so we keep their correct-workflow demos.
    sqs = case_record.get("state_queries") or []
    has_assertions = bool(sqs)
    failed = [q for q in sqs if not q.get("passed")]
    outcome_ok = not failed and (has_assertions or not strict_outcome)

    # 2. TOOL SELECT — collect tools used + (domain,verb) from shell commands
    tools_used: set[str] = set()
    shell_dvs: set[tuple[str, str]] = set()
    tool_call_count = 0
    seen_calls: list[str] = []
    dup_retry = False
    for tc in _iter_tool_calls(case_record):
        name = tc.get("name") or ""
        args = tc.get("arguments") or {}
        tools_used.add(name)
        tool_call_count += 1
        sig = f"{name}:{json.dumps(args, sort_keys=True, ensure_ascii=False)}"
        if sig in seen_calls:
            dup_retry = True
        seen_calls.append(sig)
        if name == "shell":
            dv = _parse_shell_domain_verb(args.get("command", ""))
            if dv:
                shell_dvs.add(dv)
    allowed = _allowed_tools(domain)
    bad_tools = sorted(tools_used - allowed)
    tools_ok = not bad_tools

    # 3. CONVERGENCE
    convergence_ok = tool_call_count <= MAX_TOOL_CALLS and not dup_retry

    # 4. FIDELITY
    system = ""
    if trace_lines:
        system = max(((l.get("system") or "") for l in trace_lines), key=len)
    fidelity_ok = bool(system)

    golden = outcome_ok and tools_ok and convergence_ok and fidelity_ok
    reasons: list[str] = []
    if strict_outcome and not has_assertions:
        reasons.append("no_state_query (strict: can't confirm outcome)")
    if failed:
        reasons.append(f"state_query_failed ({len(failed)}/{len(sqs)})")
    if bad_tools:
        reasons.append(f"non_ideal_tools={bad_tools}")
    if tool_call_count > MAX_TOOL_CALLS:
        reasons.append(f"too_many_calls={tool_call_count}")
    if dup_retry:
        reasons.append("duplicate_call_retry")
    if not fidelity_ok:
        reasons.append("no_system_prompt_in_trace")

    return {
        "case_id": case_id,
        "lang": lang,
        "domain": domain,
        "golden": golden,
        "reasons": reasons,
        "tools_used": sorted(tools_used),
        "bad_tools": bad_tools,
        "tool_call_count": tool_call_count,
        "shell_domain_verbs": sorted(f"{d}/{v}" for d, v in shell_dvs),
        "state_queries_passed": sum(1 for q in sqs if q.get("passed")),
        "state_queries_total": len(sqs),
    }


# ---------------------------------------------------------------------------
# Coverage
# ---------------------------------------------------------------------------

def coverage_report(scored: list[dict]) -> dict[str, Any]:
    """Aggregate golden coverage by domain×verb×lang and by tool."""
    golden = [s for s in scored if s["golden"]]

    # domain × lang
    dom_lang: dict[tuple[str, str], int] = defaultdict(int)
    for s in golden:
        dom_lang[(s["domain"], s["lang"])] += 1

    # (domain, verb) actually exercised by golden shell calls, per lang
    dv_lang: dict[tuple[str, str, str], int] = defaultdict(int)
    for s in golden:
        for dv in s["shell_domain_verbs"]:
            d, v = dv.split("/", 1)
            dv_lang[(d, v, s["lang"])] += 1

    # per-tool demo count among golden (across all tools seen)
    tool_demo: dict[str, int] = defaultdict(int)
    for s in golden:
        for t in s["tools_used"]:
            tool_demo[t] += 1

    # gaps: domains with zero golden in either lang
    domains_present = {s["domain"] for s in scored}
    dom_gaps = sorted(
        f"{d}/{lang}" for d in domains_present for lang in ("zh", "en")
        if dom_lang[(d, lang)] == 0
    )

    return {
        "total_cases": len(scored),
        "golden_cases": len(golden),
        "golden_rate": round(len(golden) / len(scored), 3) if scored else 0.0,
        "domain_x_lang_golden": {f"{d}/{l}": n for (d, l), n in sorted(dom_lang.items())},
        "domain_verb_exercised": {f"{d}/{v}/{l}": n for (d, v, l), n in sorted(dv_lang.items())},
        "tool_demo_count": dict(sorted(tool_demo.items())),
        "domain_lang_gaps": dom_gaps,
        "rejection_reasons": _tally_reasons(scored),
    }


def _tally_reasons(scored: list[dict]) -> dict[str, int]:
    tally: dict[str, int] = defaultdict(int)
    for s in scored:
        if not s["golden"]:
            for r in s["reasons"]:
                # bucket non_ideal_tools / too_many_calls by prefix
                tally[r.split("=")[0]] += 1
    return dict(sorted(tally.items(), key=lambda kv: -kv[1]))


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _load_trace(trace_dir: Path) -> list[dict]:
    p = trace_dir / "anthropic_trace.jsonl"
    if not p.exists():
        return []
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Curate teacher traces -> golden SFT dataset + coverage report")
    ap.add_argument("records_dir", type=Path,
                    help="dir of <case_id>.json CaseRecords (one run-case output per file)")
    ap.add_argument("--trace-root", type=Path, required=True,
                    help="NEOMIND_TRACE_ROOT dir with <case_id>/anthropic_trace.jsonl subdirs")
    ap.add_argument("--lang", default="?", help="language tag for all records (zh/en) if not in records")
    ap.add_argument("--out", type=Path, default=Path("golden.jsonl"), help="output golden SFT JSONL")
    ap.add_argument("--report", type=Path, default=Path("coverage_report.md"), help="output coverage report")
    ap.add_argument("--include-non-golden", action="store_true", help="also write non-golden (tagged) rows")
    ap.add_argument("--strict-outcome", action="store_true",
                    help="require a state_query assertion (reject no-assertion cases); "
                         "stronger guarantee, less coverage")
    args = ap.parse_args(argv)

    from eval.sft.render_minicpm import case_to_sft  # local import to avoid cycle at import time

    records = sorted(args.records_dir.glob("*.json"))
    if not records:
        print(f"no CaseRecord JSONs in {args.records_dir}", file=sys.stderr)
        return 1

    scored: list[dict] = []
    n_golden = 0
    with args.out.open("w") as fout:
        for rec_path in records:
            try:
                cr = json.loads(rec_path.read_text())
            except Exception as e:  # noqa: BLE001
                print(f"skip {rec_path.name}: {e}", file=sys.stderr)
                continue
            case_id = cr.get("case_id") or rec_path.stem
            lang = cr.get("lang") or args.lang
            trace = _load_trace(args.trace_root / case_id)
            s = score_case(cr, trace, lang=lang, strict_outcome=args.strict_outcome)
            scored.append(s)
            if s["golden"]:
                ex = case_to_sft(cr, trace, golden_only=False)  # already passed the bar
                if ex:
                    fout.write(json.dumps(ex, ensure_ascii=False) + "\n")
                    n_golden += 1
            elif args.include_non_golden:
                fout.write(json.dumps({"_non_golden": True, **s}, ensure_ascii=False) + "\n")

    report = coverage_report(scored)
    _write_report(args.report, report, args.out, n_golden)
    print(f"scored {len(scored)} cases -> {n_golden} golden ({report['golden_rate']*100:.0f}%) "
          f"-> {args.out}\nreport -> {args.report}", file=sys.stderr)
    return 0


def _write_report(path: Path, report: dict, out_file: Path, n_golden: int) -> None:
    lines = [
        "# SFT Golden Dataset — Coverage Report",
        "",
        f"- cases scored: **{report['total_cases']}**",
        f"- golden: **{n_golden}** ({report['golden_rate']*100:.0f}%)",
        "",
        "## Domain × language (golden count)",
    ]
    for k, v in report["domain_x_lang_golden"].items():
        lines.append(f"- `{k}`: {v}")
    lines += ["", "## (domain/verb) exercised by golden shell calls"]
    for k, v in report["domain_verb_exercised"].items():
        lines.append(f"- `{k}`: {v}")
    lines += ["", "## Per-tool demo count (golden)"]
    for k, v in report["tool_demo_count"].items():
        lines.append(f"- `{k}`: {v}")
    lines += ["", "## Domain/lang gaps (0 golden)"]
    lines += [f"- `{g}`" for g in report["domain_lang_gaps"]] or ["- (none)"]
    lines += ["", "## Rejection reasons (non-golden)",
              "```json", json.dumps(report["rejection_reasons"], indent=2, ensure_ascii=False), "```"]
    path.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())

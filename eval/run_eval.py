#!/usr/bin/env python3
"""NeoMind chat agent eval runner (Python port).

Single entrypoint for: schema validate, run one case, run a directory of
cases, run the smoke suite, generate the grade card. The agent under test
runs inside a real `neomind serve` subprocess — production chat pipeline,
production tool registry, production system prompts.

Usage:
    python3 eval/run_eval.py validate-all --root eval/cases
    python3 eval/run_eval.py run-case --case eval/smoke/good-002.json
    python3 eval/run_eval.py smoke
    python3 eval/run_eval.py run \
        --root eval/cases \
        --lang both \
        --workflow device,rule \
        --judge
    python3 eval/run_eval.py report --scores eval/runs/<ts>/scores.jsonl

Env:
    AGENT_LLM_API_KEY, AGENT_LLM_ENDPOINT, AGENT_LLM_MODEL  (powers the chat agent)
    AGENT_LLM_BACKEND_TYPE (default "openai" — works for most OpenAI-compatible)
    AGENT_LLM_THINKING (default "false"; per commit c6385169)
    ANTHROPIC_API_KEY  (powers the Claude judge)
    EVAL_JUDGE_MODEL (default claude-opus-4-6)
    NEOMIND_TEST_BIN (default <cwd>/target/release/neomind)
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

# Make lib/ importable.
sys.path.insert(0, str(Path(__file__).parent / "lib"))

import fallback  # noqa: E402
import hard_signal  # noqa: E402
import judge  # noqa: E402
import report  # noqa: E402
import seed  # noqa: E402
import server  # noqa: E402
import simulator  # noqa: E402
import state_query  # noqa: E402
import validate  # noqa: E402


def _load_case(path: Path) -> dict:
    return json.loads(path.read_text())


def _truncate(s, n: int = 800) -> str:
    """Truncate a string for trace display."""
    if s is None:
        return ""
    if not isinstance(s, str):
        s = json.dumps(s, ensure_ascii=False)
    return s if len(s) <= n else s[:n] + f"... (+{len(s) - n} chars)"


def _build_turn_record(user_msg: str, resp: dict, pt_ms: int) -> dict:
    """Build a turn record enriched with tool args/results/thinking.

    Prefers live-streamed WS events (`tool_calls_stream` / `thinking_stream`)
    when present — these come straight from the production streaming pipeline
    and capture multi-round ReAct loops correctly. Falls back to parsing the
    `new_messages` history delta (legacy HTTP path) when stream data is absent.
    """
    # === Preferred: live-streamed events from WebSocket ===
    stream_tc = resp.get("tool_calls_stream") or []
    stream_thinking = resp.get("thinking_stream") or None
    if stream_tc:
        return {
            "user": user_msg,
            "assistant_message": resp.get("response", ""),
            "tool_calls": stream_tc,
            "thinking": stream_thinking,
            "round_contents": None,  # not exposed via WS events
            "round_thinking": None,
            "processing_time_ms": pt_ms,
            "raw_messages": resp.get("new_messages") or [],
            "transport": "websocket",
            "ws_error": resp.get("error"),
            "transient_stall_retry_count": resp.get("transient_stall_retry_count", 0),
        }

    # === Fallback: parse history delta (HTTP path or WS without stream data) ===
    new_messages = resp.get("new_messages") or []

    # Collect assistant messages (final + intermediate tool-calling rounds).
    # The LAST assistant message is the final reply; earlier ones carry
    # tool_calls with args/result populated by the tool loop.
    assistant_msgs = [m for m in new_messages if m.get("role") == "assistant"]
    tool_result_msgs = {
        m.get("tool_call_id"): m
        for m in new_messages
        if m.get("role") == "tool" and m.get("tool_call_id")
    }

    # Build enriched tool_calls list (preserves call order across rounds).
    enriched_tc = []
    for m in assistant_msgs:
        tcs = m.get("tool_calls") or []
        for tc in tcs:
            # ToolCall is flat in our backend, but tolerate OpenAI-nested
            # shape just in case a future refactor changes serialization.
            if "function" in tc and isinstance(tc["function"], dict):
                name = tc["function"].get("name", "?")
                args = tc["function"].get("arguments")
            else:
                name = tc.get("name", "?")
                args = tc.get("arguments")
            # arguments may arrive as a JSON string — parse for readability.
            if isinstance(args, str):
                try:
                    args = json.loads(args)
                except Exception:
                    pass
            # Prefer the inline `result` field; fall back to the matching
            # tool-role message's content.
            result = tc.get("result")
            if result is None:
                tm = tool_result_msgs.get(tc.get("id"))
                if tm:
                    result = tm.get("content")
            enriched_tc.append({
                "name": name,
                "arguments": args,
                "result": result,
                "round": tc.get("round"),
                "tool_call_id": tc.get("id"),
            })

    # Pull thinking + round_contents from whichever assistant message has them.
    thinking = stream_thinking
    round_contents = None
    round_thinking = None
    for m in assistant_msgs:
        if not thinking and m.get("thinking"):
            thinking = m["thinking"]
        if not round_contents and m.get("round_contents"):
            round_contents = m["round_contents"]
        if not round_thinking and m.get("round_thinking"):
            round_thinking = m["round_thinking"]

    return {
        "user": user_msg,
        "assistant_message": resp.get("response", ""),
        "tool_calls": enriched_tc if enriched_tc else [
            # Fallback to the thin shape if history delta was unavailable.
            {"name": t, "arguments": None, "result": None}
            for t in resp.get("tools_used", [])
        ],
        "thinking": thinking,
        "round_contents": round_contents,
        "round_thinking": round_thinking,
        "processing_time_ms": pt_ms,
        "raw_messages": new_messages,
        "transport": "websocket" if resp.get("tool_calls_stream") is not None else "http",
        "ws_error": resp.get("error") if resp.get("tool_calls_stream") is not None else None,
        "transient_stall_retry_count": resp.get("transient_stall_retry_count", 0),
    }


def _load_fixture(name: str) -> dict:
    p = Path(__file__).parent / "fixtures" / f"{name}.json"
    return json.loads(p.read_text())


def _walk_json_files(root: Path) -> list[Path]:
    out = []
    for p in sorted(root.rglob("*.json")):
        out.append(p)
    return out


def run_case(case_path: str) -> dict:
    """Run one case end-to-end. Returns a CaseRecord dict."""
    case = _load_case(Path(case_path))

    # Validate schema; hard-fail on shape errors.
    errors = validate.validate_case(case)
    if errors:
        return _error_record(case, "schema_error", "; ".join(errors))

    srv = server.TestServer()
    try:
        try:
            srv.spawn(case_id=case.get("id"))
        except Exception as e:
            return _error_record(case, "seed_failure", f"spawn failed: {e}")

        try:
            srv.configure_llm_backend()
        except Exception as e:
            return _error_record(case, "llm_config_error", str(e))

        # Seed fixture + case extras.
        try:
            fix = _load_fixture(case["setup"]["fixture"])
            seed.seed_fixture(srv, fix)
            seed.seed_extras(srv, case["setup"].get("extras", {}) or {})
        except Exception as e:
            return _error_record(case, "seed_failure", str(e))

        # Create session + run turns via HTTP chat.
        try:
            sid = srv.create_chat_session()
        except Exception as e:
            return _error_record(case, "seed_failure", f"create session: {e}")

        turn_records = []
        for turn in case.get("turns", []):
            t0 = time.monotonic()
            try:
                resp = srv.chat(sid, turn["user"],
                                images=turn.get("images") or case.get("images"))
            except Exception as e:
                # Record the turn we have so far, then bail with timeout-style
                # status so the judge can mark it as agent error.
                return _error_record_at(
                    case,
                    "agent_error",
                    f"turn failed ({turn['user']!r}): {e}",
                    turn_records,
                )
            elapsed_ms = int((time.monotonic() - t0) * 1000)
            # Use server-reported processing_time_ms when present; fall back to
            # wall clock so we always have a number for fallback detection.
            pt = resp.get("processing_time_ms") or elapsed_ms
            turn_records.append(_build_turn_record(turn["user"], resp, pt))

        # Optional post-run delay before state queries — used by cases that
        # trigger async operations (e.g. `agent invoke` returns immediately
        # but updates stats.total_executions only after the background
        # execution lands). Without this, the SQ races the agent runtime.
        delay_ms = int(case.get("post_run_delay_ms") or 0)
        if delay_ms > 0:
            time.sleep(delay_ms / 1000.0)

        # State queries.
        sqs = case.get("state_queries") or []
        state_results = []
        # Final assistant text — lets response_contains assert cross-turn recall.
        final_text = ""
        for tr_ in turn_records:
            if tr_.get("assistant_message"):
                final_text = tr_["assistant_message"]
        for q in sqs:
            try:
                # Optional per-turn assertion: `turn_index` targets a specific
                # turn's assistant message (for "does the model still remember
                # X N turns later"). Default = final turn.
                tidx = q.get("turn_index")
                resp = final_text
                if isinstance(tidx, int) and 0 <= tidx < len(turn_records):
                    resp = turn_records[tidx].get("assistant_message") or ""
                r = state_query.run_query(q, srv.api_base, srv.api_key, resp)
                state_results.append(r)
            except Exception as e:
                state_results.append({
                    "type": q.get("type"),
                    "error": str(e),
                    "passed": False,
                })

        # Runtime verification: inject triggers (telemetry that should FIRE the
        # just-configured rule/transform/agent), wait for the async engine,
        # then run runtime.expect. Appended to state_results so the hard
        # signal covers "did it actually fire" — not just "does it exist".
        runtime = case.get("runtime")
        if runtime:
            # Pre-inject wait: let the server's background init (adapters —
            # webhook takes ~10s, after the slow MQTT adapter) complete before
            # we POST the trigger. Without this, the webhook adapter isn't
            # registered yet → 500 "not initialized".
            pre_wait = int(runtime.get("pre_wait_ms") or 0)
            if pre_wait > 0:
                time.sleep(pre_wait / 1000.0)
            for trig in runtime.get("trigger") or []:
                if trig.get("type") == "telemetry":
                    try:
                        srv.post(
                            f"/devices/{trig['device_id']}/metrics",
                            {"metric": trig.get("metric"), "value": trig.get("value")},
                        )
                    except Exception as e:
                        print(f"  runtime inject error: {e}", file=sys.stderr)
                elif trig.get("type") == "webhook":
                    # Webhook ingestion publishes DeviceMetric events (fires
                    # rules/transforms); HTTP /devices/:id/metrics does NOT.
                    try:
                        body = trig.get("body") or {trig.get("metric"): trig.get("value")}
                        resp = srv.post(f"/devices/{trig['device_id']}/webhook", body)
                        print(
                            f"  [runtime] webhook inject → {resp.status_code}: "
                            f"{resp.text[:200]}",
                            file=sys.stderr,
                        )
                    except Exception as e:
                        print(f"  runtime webhook inject error: {e}", file=sys.stderr)
            # Start continuous-telemetry simulators (run during the wait).
            # Simulators send telemetry via webhook ingestion at `interval`
            # seconds, with optional drift (gradual metric change). Rules fire
            # on CONTINUOUS data — much more realistic than one-shot injection.
            sim_configs = runtime.get("simulators") or []
            sims = []
            if sim_configs:
                sims = simulator.start_simulators(
                    srv.api_base, srv.api_key, sim_configs
                )
                print(
                    f"  [runtime] started {len(sims)} device simulator(s)",
                    file=sys.stderr,
                )

            time.sleep(int(runtime.get("wait_ms") or 2000) / 1000.0)

            # Stop simulators + let last events settle.
            if sims:
                simulator.stop_simulators(sims)
                time.sleep(1.0)
            for q in runtime.get("expect") or []:
                try:
                    r = state_query.run_query(q, srv.api_base, srv.api_key)
                    r["_runtime"] = True
                    state_results.append(r)
                except Exception as e:
                    state_results.append({
                        "type": q.get("type"),
                        "error": str(e),
                        "passed": False,
                        "_runtime": True,
                    })

        suspected = fallback.detect_suspected_fallback(
            turn_records,
            (case.get("expectations") or {}).get("per_turn", []),
        )

        return {
            "case_id": case["id"],
            "lang": case["lang"],
            "turn_records": turn_records,
            "state_queries": state_results,
            "suspected_fallback": suspected,
            "status": None,
            "error_type": None,
            "message": None,
        }
    finally:
        srv.shutdown()


def _error_record(case: dict, status: str, msg: str) -> dict:
    return _error_record_at(case, status, msg, [])


def _error_record_at(case: dict, status: str, msg: str, turn_records: list) -> dict:
    return {
        "case_id": case.get("id", "?"),
        "lang": case.get("lang", "?"),
        "turn_records": turn_records,
        "state_queries": [],
        "suspected_fallback": False,
        "status": status,
        "error_type": status,
        "message": msg,
    }


def cmd_validate_all(args):
    root = Path(args.root)
    if not root.exists():
        print(f"root not found: {root}", file=sys.stderr)
        return 1
    total = failed = 0
    for p in _walk_json_files(root):
        total += 1
        try:
            case = _load_case(p)
        except Exception as e:
            print(f"{p}: parse error: {e}", file=sys.stderr)
            failed += 1
            continue
        errs = validate.validate_case(case)
        if errs:
            failed += 1
            print(f"{p}:", file=sys.stderr)
            for e in errs:
                print(f"  ERROR: {e}", file=sys.stderr)
    print(f"validated {total} cases, {failed} failed")
    return 1 if failed else 0


def cmd_run_case(args):
    rec = run_case(args.case)
    print(json.dumps(rec, ensure_ascii=False))
    return 0


def cmd_smoke(args):
    smoke_dir = Path(args.dir)
    out_dir = Path(args.out_dir) if args.out_dir else None
    if out_dir:
        out_dir.mkdir(parents=True, exist_ok=True)
    cases_jsonl = []
    for p in sorted(smoke_dir.glob("*.json")):
        print(f"--- {p} ---", file=sys.stderr)
        rec = run_case(str(p))
        print(json.dumps(rec, ensure_ascii=False))
        cases_jsonl.append(rec)
    if out_dir:
        (out_dir / "cases.jsonl").write_text(
            "\n".join(json.dumps(r, ensure_ascii=False) for r in cases_jsonl)
        )
        print(f"wrote {out_dir / 'cases.jsonl'}", file=sys.stderr)
    return 0


def _select_cases(root: Path, lang: str, workflows: list[str] | None, case_id: str | None) -> list[Path]:
    out = []
    for p in _walk_json_files(root):
        if case_id:
            try:
                if _load_case(p).get("id") != case_id:
                    continue
            except Exception:
                continue
        else:
            if lang != "both":
                # Path shape: eval/cases/<lang>/<workflow>/<case>.json
                parts = p.relative_to(root).parts
                if not parts or parts[0] != lang:
                    continue
            if workflows:
                parts = p.relative_to(root).parts
                wf = parts[1] if len(parts) > 2 else ""
                if wf not in workflows:
                    continue
        out.append(p)
    return out


def cmd_run(args):
    root = Path(args.root)
    cases = _select_cases(root, args.lang, args.workflow, args.case_id)
    if not cases:
        print("no matching cases", file=sys.stderr)
        return 1

    ts = time.strftime("%Y%m%d-%H%M%S")
    run_dir = Path(args.run_dir or f"eval/runs/{ts}")
    run_dir.mkdir(parents=True, exist_ok=True)

    cases_jsonl_path = run_dir / "cases.jsonl"
    scores_jsonl_path = run_dir / "scores.jsonl"

    with cases_jsonl_path.open("w") as cf, scores_jsonl_path.open("w") as sf:
        for p in cases:
            print(f"--- {p} ---", file=sys.stderr)
            rec = run_case(str(p))
            cf.write(json.dumps(rec, ensure_ascii=False) + "\n")
            cf.flush()

            # Hard signals are ALWAYS computed — no judge, no LLM, no API cost.
            # They are the reliable metric (the judge is soft/inflation-prone)
            # and enable judge-free local-model iteration.
            case = _load_case(p)
            hard = hard_signal.compute(case, rec)
            score = {
                "case_id": case.get("id"),
                "lang": case.get("lang"),
                "category": case.get("category"),
                "hard": hard,
                "suspected_fallback": bool(rec.get("suspected_fallback")),
                "status": rec.get("status"),
                "message": rec.get("message", ""),
            }
            if args.judge:
                try:
                    jscore = judge.judge_case(case, rec)
                except Exception as e:
                    print(f"  JUDGE ERROR: {e}", file=sys.stderr)
                    jscore = {
                        "scores": {},
                        "overall_reasoning": f"judge error: {e}",
                        "judge": "claude-opus-4-6",
                        "duration_ms": 0,
                    }
                score["scores"] = jscore.get("scores", {})
                score["overall_reasoning"] = jscore.get("overall_reasoning", "")
                score["judge"] = jscore.get("judge")
                score["duration_ms"] = jscore.get("duration_ms", 0)
            sf.write(json.dumps(score, ensure_ascii=False) + "\n")
            sf.flush()
            verdict = hard_signal.pass_for_case(hard)
            vtag = {True: "HARD_PASS", False: "HARD_FAIL", None: "n/a"}[verdict]
            tail = ""
            if hard.get("wrong_tool"):
                tail += f" wrong_tool={hard['wrong_tools_used']}"
            if args.judge:
                tail += " (+judge)"
            print(f"  case_id={score.get('case_id')} {vtag}{tail}", file=sys.stderr)

    print(f"\nRun dir: {run_dir}", file=sys.stderr)

    scores_text = scores_jsonl_path.read_text()
    agg = report.aggregate(scores_text)
    hp = report.hard_pass_rate(agg)
    print(
        f"HARD PASS RATE: {hp['passed']}/{hp['denom']} ({hp['pct']:.0f}%)  "
        f"wrong_tool={hp['wrong_tool']}  unasserted={hp['unasserted']}  "
        f"agent_failed={hp['agent_failed']}",
        file=sys.stderr,
    )
    if args.judge:
        grade = report.grade_letter(report.overall(agg))
        print(
            f"soft judge grade: {grade} ({report.overall(agg):.1f}/100), "
            f"{agg['total_cases']} cases, {agg['malformed']} malformed, "
            f"{agg['agent_errors']} agent errors",
            file=sys.stderr,
        )
    else:
        print("(no judge — hard signals only; pass --judge for soft scores)",
              file=sys.stderr)
    report.write_grade_card(agg, run_dir / "grade-card.md")
    print(f"wrote {run_dir / 'grade-card.md'}", file=sys.stderr)

    return 0


def cmd_report(args):
    scores_text = Path(args.scores).read_text()
    agg = report.aggregate(scores_text)
    grade = report.grade_letter(report.overall(agg))
    out = Path(args.out)
    report.write_grade_card(agg, out)
    print(
        f"grade: {grade} ({report.overall(agg):.1f}/100), "
        f"{agg['total_cases']} cases, "
        f"{agg['malformed']} malformed, "
        f"{agg['agent_errors']} agent errors"
    )
    print(f"wrote {out}")
    return 0


def cmd_compare(args):
    """Per-category hard pass-rate delta: new run vs a committed baseline."""
    base_agg = report.aggregate(Path(args.baseline).read_text())
    new_agg = report.aggregate(Path(args.run).read_text())
    cmp = report.compare(base_agg, new_agg)
    bh, nh = cmp["base"], cmp["new"]
    print(f"baseline: hard {bh['passed']}/{bh['denom']} ({bh['pct']:.0f}%)  "
          f"wrong_tool={bh['wrong_tool']}  unasserted={bh['unasserted']}")
    print(f"new:      hard {nh['passed']}/{nh['denom']} ({nh['pct']:.0f}%)  "
          f"wrong_tool={nh['wrong_tool']}  unasserted={nh['unasserted']}")
    print(f"overall hard delta: {nh['pct'] - bh['pct']:+.0f} pct pts")
    print()
    print(f"{'category':<14}{'base':>12}{'new':>12}{'delta':>8}")
    print("-" * 46)
    for r in cmp["rows"]:
        bp = f"{r['base_pct']:.0f}%/{r['base_n']}" if r["base_pct"] is not None else "—"
        np_ = f"{r['new_pct']:.0f}%/{r['new_n']}" if r["new_pct"] is not None else "—"
        if r["base_pct"] is not None and r["new_pct"] is not None:
            d = f"{r['new_pct'] - r['base_pct']:+.0f}"
        else:
            d = "—"
        print(f"{r['category']:<14}{bp:>12}{np_:>12}{d:>8}")
    return 0


def main():
    ap = argparse.ArgumentParser(prog="run_eval")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("validate-all", help="validate every case under root")
    p.add_argument("--root", default="eval/cases")
    p.set_defaults(func=cmd_validate_all)

    p = sub.add_parser("run-case", help="run one case, print CaseRecord JSON")
    p.add_argument("--case", required=True)
    p.set_defaults(func=cmd_run_case)

    p = sub.add_parser("smoke", help="run all smoke cases")
    p.add_argument("--dir", default="eval/smoke")
    p.add_argument("--out-dir", default=None)
    p.set_defaults(func=cmd_smoke)

    p = sub.add_parser("run", help="run selected cases + optional judge")
    p.add_argument("--root", default="eval/cases")
    p.add_argument("--lang", choices=["zh", "en", "both"], default="both")
    p.add_argument("--workflow", help="comma-separated workflow names", default=None)
    p.add_argument("--case-id", default=None)
    p.add_argument("--judge", action="store_true", help="invoke Claude judge")
    p.add_argument("--run-dir", default=None)
    p.set_defaults(func=cmd_run)

    p = sub.add_parser("report", help="aggregate scores.jsonl → grade-card.md")
    p.add_argument("--scores", required=True)
    p.add_argument("--out", default="grade-card.md")
    p.set_defaults(func=cmd_report)

    p = sub.add_parser("compare",
                       help="per-category hard pass-rate delta vs a baseline")
    p.add_argument("--baseline", required=True,
                   help="baseline scores.jsonl (e.g. eval/baselines/glm-5.2/scores.jsonl)")
    p.add_argument("--run", required=True, help="new run scores.jsonl")
    p.set_defaults(func=cmd_compare)

    args = ap.parse_args()
    sys.exit(args.func(args))


if __name__ == "__main__":
    main()

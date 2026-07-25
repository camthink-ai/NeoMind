"""Claude-as-judge over a CaseRecord.

Uses the anthropic SDK. The judge sees:
- The case's `applies[]` dimensions
- The case's `expectations.per_turn[]` and `expectations.overall`
- The CaseRecord's `turn_records`, `state_queries`, `suspected_fallback`

And returns integer scores 0-10 per dimension plus a reasoning string.
"""
from __future__ import annotations

import json
import os
import time

try:
    import anthropic
except ImportError:  # pragma: no cover
    anthropic = None


JUDGE_MODEL_ENV = "EVAL_JUDGE_MODEL"
# Fall back to ANTHROPIC_MODEL (used by BigModel's Anthropic-compatible
# endpoint), then to a Claude default.
DEFAULT_JUDGE_MODEL = os.environ.get("ANTHROPIC_MODEL", "claude-opus-4-6")

RUBRIC = """You are scoring a NeoMind chat agent. Score each dimension in the
case's `applies[]` list from 0-10 using these anchors:

| Dimension | Weight | Anchor 10 | Anchor 8 | Anchor 5 | Anchor 2 | Anchor 0 |
|-----------|--------|-----------|----------|----------|----------|----------|
| tool_accuracy | 25% | exact right tool + args | right tool, minor arg slip | wrong tool but recovered | wrong tool, no recovery | no tool call |
| task_completion | 25% | all expectations + state queries pass | main goal met, minor miss | partial | barely any | nothing done |
| response_quality | 20% | clear, structured, on-brand | clear but verbose | unclear in places | confusing | incoherent |
| context_retention | 15% | perfect recall across turns | minor lapse | one missed reference | multiple lapses | lost the thread (multi_turn only) |
| error_recovery | 15% | no errors, OR recovered gracefully | recovered with effort | recovery attempted | recovery failed | gave up |
| language_adherence | 5% | native-quality zh/en | fluent | minor cross-language leak | frequent leaks | wrong language |

If `applies[]` is a subset, only score those dimensions. If `suspected_fallback`
is true, the agent likely never called the LLM — cap all scores at 2.

**CRITICAL — read ALL fields in tool results before scoring:**
Tool results are JSON objects that often contain MULTIPLE data fields. For
example, a `neomind widget list` result has both `total` (count of installed
widgets) AND `builtin_types` (a catalogue of built-in widget types). A
`total: 0` in one field does NOT mean the tool returned nothing — you MUST
scan every field in the result (including ones further down like
`builtin_types`, `builtin_count`, `summary`, `meta`, etc.) before deciding
whether the agent's response matches the data. Do NOT claim the agent
"hallucinated" or "contradicted the tool output" unless you have checked
every field in the truncated result shown to you.

Return STRICT JSON only, no prose, matching this exact shape:
{
  "scores": {"tool_accuracy": 9, "task_completion": 10, ...},
  "overall_reasoning": "<one paragraph>"
}
"""


def _truncate_for_judge(s, n: int = 600) -> str:
    """Compact a string/value for the judge prompt."""
    if s is None:
        return ""
    if not isinstance(s, str):
        s = json.dumps(s, ensure_ascii=False)
    return s if len(s) <= n else s[:n] + f"... (+{len(s) - n} chars)"


def _format_case_record(rec: dict, blind: bool = True) -> str:
    """Render CaseRecord compactly for the judge prompt.

    Includes the LLM's thinking, tool arguments, and tool results (truncated)
    — without these the judge cannot distinguish "system lost the args" from
    "model decided not to act".

    ``blind=True`` (default) hides the state_query pass/fail results so the
    judge can't anchor its task_completion score on a hard pass/fail flag —
    it must judge from the trace. This removes the known anchoring bias.
    """
    out = []
    for i, t in enumerate(rec.get("turn_records", []), 1):
        out.append(f"--- Turn {i} ---")
        out.append(f"User: {t.get('user', '')}")
        out.append(f"Assistant reply: {t.get('assistant_message', '')}")
        thinking = t.get("thinking")
        if thinking:
            out.append(f"LLM thinking: {_truncate_for_judge(thinking, 400)}")
        tools = t.get("tool_calls", [])
        if tools:
            out.append(f"Tools called ({len(tools)}):")
            for j, tc in enumerate(tools, 1):
                name = tc.get("name", "?")
                args = tc.get("arguments")
                result = tc.get("result")
                rnd = tc.get("round")
                rnd_tag = f" round={rnd}" if rnd else ""
                args_s = _truncate_for_judge(args, 200)
                res_s = _truncate_for_judge(result, 4000)
                out.append(
                    f"  [{j}] {name}{rnd_tag} args={args_s} result={res_s}"
                )
        else:
            out.append("Tools called: (none)")
        out.append(f"Processing time: {t.get('processing_time_ms', 0)} ms")
    sqs = rec.get("state_queries", [])
    if sqs and not blind:
        # Only shown when blind=False. Hiding pass/fail prevents the judge from
        # anchoring task_completion on the hard signal (it must read the trace).
        out.append("\n--- State Queries ---")
        for sq in sqs:
            out.append(
                f"  {sq.get('type')}: expected={sq.get('expected')!r} "
                f"actual={sq.get('actual')!r} passed={sq.get('passed')}"
            )
    if rec.get("suspected_fallback"):
        out.append("\n⚠️ suspected_fallback=true (LLM likely never ran)")
    if rec.get("status"):
        out.append(
            f"\n❌ Agent failed: status={rec.get('status')} "
            f"message={rec.get('message', '')}"
        )
    return "\n".join(out)


def judge_case(case: dict, case_record: dict, blind: bool = True) -> dict:
    """Score one case. Returns a ScoreLine dict.

    ``blind=True`` (default) hides state_query results from the judge prompt
    to remove the pass/fail anchoring bias.
    """
    if anthropic is None:
        raise RuntimeError(
            "anthropic package not installed — pip install -r eval/requirements.txt"
        )

    # If the agent failed outright, short-circuit: no scores, just record why.
    if case_record.get("status"):
        return {
            "case_id": case.get("id"),
            "lang": case.get("lang"),
            "scores": {},
            "overall_reasoning": (
                f"agent {case_record.get('status')}: "
                f"{case_record.get('message', '')}"
            ),
            "judge": "claude-opus-4-6",
            "duration_ms": 0,
            "suspected_fallback": bool(case_record.get("suspected_fallback")),
            "status": case_record.get("status"),
            "error_type": case_record.get("error_type") or case_record.get("status"),
            "message": case_record.get("message", ""),
        }

    applies = case.get("applies", [])
    per_turn = (case.get("expectations") or {}).get("per_turn", [])
    overall = (case.get("expectations") or {}).get("overall", "")

    user_prompt = (
        f"## Case\n"
        f"id: {case.get('id')}\n"
        f"lang: {case.get('lang')}\n"
        f"scenario_type: {case.get('scenario_type')}\n"
        f"description: {case.get('description', '')}\n\n"
        f"## Dimensions to score\n{', '.join(applies)}\n\n"
        f"## Expectations (per_turn)\n"
        + "\n".join(f"- {e}" for e in per_turn)
        + f"\n\n## Expectations (overall)\n{overall}\n\n"
        f"## Agent Trace\n\n{_format_case_record(case_record, blind=blind)}\n\n"
        f"## Your Task\n{RUBRIC}\n"
    )

    # Anthropic SDK reads ANTHROPIC_API_KEY by default. The BigModel
    # Anthropic-compatible endpoint uses ANTHROPIC_AUTH_TOKEN instead, so
    # fall back to it. ANTHROPIC_BASE_URL is auto-read by the SDK.
    api_key = (
        os.environ.get("ANTHROPIC_API_KEY")
        or os.environ.get("ANTHROPIC_AUTH_TOKEN")
    )
    if not api_key:
        raise RuntimeError(
            "ANTHROPIC_API_KEY (or ANTHROPIC_AUTH_TOKEN for BigModel) required"
        )
    client = anthropic.Anthropic(api_key=api_key)
    model = os.environ.get(JUDGE_MODEL_ENV, DEFAULT_JUDGE_MODEL)

    t0 = time.monotonic()
    resp = client.messages.create(
        model=model,
        max_tokens=1024,
        messages=[{"role": "user", "content": user_prompt}],
    )
    duration_ms = int((time.monotonic() - t0) * 1000)

    # Pull the first text block.
    text = ""
    for block in resp.content:
        if getattr(block, "type", None) == "text":
            text = block.text
            break
    if not text:
        text = resp.content[0].text if resp.content else ""

    # Parse JSON — tolerate ```json fenced blocks.
    body = text.strip()
    if body.startswith("```"):
        # strip first line (```json or ```) and trailing ```
        lines = body.splitlines()
        body = "\n".join(lines[1:-1] if lines[-1].startswith("```") else lines[1:])
    try:
        parsed = json.loads(body)
    except json.JSONDecodeError:
        # Last-ditch: find first { ... } span.
        first = body.find("{")
        last = body.rfind("}")
        if first >= 0 and last > first:
            parsed = json.loads(body[first : last + 1])
        else:
            raise

    return {
        "case_id": case.get("id"),
        "lang": case.get("lang"),
        "scores": parsed.get("scores", {}),
        "overall_reasoning": parsed.get("overall_reasoning", ""),
        "judge": model,
        "duration_ms": duration_ms,
        "suspected_fallback": bool(case_record.get("suspected_fallback")),
    }

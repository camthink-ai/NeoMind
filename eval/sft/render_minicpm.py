"""Render NeoMind eval traces into MiniCPM5 SFT training data.

The SFT data factory joins two capture sources (see memory:
minicpm5-neomind-baseline):

  CaseRecord (eval result, `run-case`/`run` output)
    - structured conversation: ``raw_messages[].tool_calls`` carry the exact
      ``name`` + ``arguments`` the teacher invoked (the golden actions we
      distill). Tool results are ``role:"tool"`` messages.
    - ``state_queries`` give the pass/fail label used to filter golden traces.
    - DOES NOT contain the system prompt (session history excludes it).

  anthropic_trace.jsonl (Rust hook in ``openai.rs::build_anthropic_request``,
  gated on ``NEOMIND_TRACE_DIR``)
    - the exact per-round system prompt the teacher received (it GROWS per
      round as skills are loaded into the prompt — NOT constant), plus the
      tool definitions.
    - DOES NOT contain structured assistant tool calls (``to_core()`` keeps
      only prose / a lossy summary in ``content``); that's why we need the
      CaseRecord above.

Output: JSONL of ``{"messages": [...], "tools": [...]}`` in OpenAI chat format
(``messages[0]`` = system; assistant turns carry ``tool_calls`` with dict
``arguments``). The MiniCPM5 tokenizer's own ``chat_template.jinja`` (vendored
at ``templates/minicpm5_chat_template.jinja``) renders this into the exact
``<function>/<param>/<tool_response>`` ChatML the model trains/infers on — so
feeding ``messages``+``tools`` to any SFT framework that applies the MiniCPM5
template reproduces inference byte-for-byte (prompt-freezing contract).
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Iterable

TEMPLATE_PATH = Path(__file__).with_name("templates") / "minicpm5_chat_template.jinja"


# ---------------------------------------------------------------------------
# Conversions
# ---------------------------------------------------------------------------

def anthropic_tools_to_openai(tools: list[dict] | None) -> list[dict]:
    """Trace tools are Anthropic-shaped ``{name, description, input_schema}``.

    The student (MiniCPM5 via llama.cpp ``/v1``) is an OpenAI backend, so at
    inference NeoMind sends OpenAI-shaped tools. The SFT data must match the
    student's inference format, not the teacher's trace format — convert here.
    """
    out: list[dict] = []
    for t in tools or []:
        out.append({
            "type": "function",
            "function": {
                "name": t.get("name"),
                "description": t.get("description", ""),
                "parameters": t.get("input_schema") or {"type": "object", "properties": {}},
            },
        })
    return out


def raw_messages_to_chat(raw_messages: list[dict]) -> list[dict]:
    """CaseRecord ``raw_messages`` -> OpenAI chat messages (no system message).

    - assistant turns keep their structured ``tool_calls`` (name + dict args)
    - ``role:"tool"`` results pass through verbatim

    NB: ``arguments`` stays a **dict**, not the OpenAI JSON-string form. The
    MiniCPM5 template iterates ``arguments.items()`` directly, so a string
    would render wrong. This makes the output MiniCPM5-specific.
    """
    msgs: list[dict] = []
    for m in raw_messages:
        role = m.get("role") or "user"
        content = m.get("content") or ""
        if role == "assistant":
            am: dict[str, Any] = {"role": "assistant", "content": content}
            tcs = m.get("tool_calls") or []
            if tcs:
                am["tool_calls"] = [
                    {
                        "type": "function",
                        "function": {
                            "name": c.get("name"),
                            "arguments": c.get("arguments", {}),
                        },
                    }
                    for c in tcs
                ]
            msgs.append(am)
        elif role == "tool":
            msgs.append({"role": "tool", "content": content})
        else:
            msgs.append({"role": role, "content": content})
    return msgs


def fullest_system(trace_lines: Iterable[dict]) -> str:
    """Pick the system prompt to train against.

    The system prompt grows each round as skill guides are injected (verified
    empirically: 9.4k -> 24k -> 24k chars across 3 rounds). The fullest line
    (longest) is the one with all loaded skills — the most complete context
    for the conversation. Using it for the whole conversation is a small
    approximation (early rounds had a shorter prompt) but keeps the loaded
    skill guides visible to every assistant turn.
    """
    systems = [(l.get("system") or "") for l in trace_lines]
    systems = [s for s in systems if s]
    if not systems:
        return ""
    return max(systems, key=len)


def case_to_sft(
    case_record: dict,
    trace_lines: list[dict],
    *,
    golden_only: bool = True,
) -> dict | None:
    """Join one CaseRecord with its trace lines into a single SFT example.

    Returns ``None`` when filtered out (non-golden, or no trace). The example
    is one conversation = one training row (let the SFT framework mask
    non-assistant tokens).
    """
    sqs = case_record.get("state_queries") or []
    if golden_only and sqs and not all(q.get("passed") for q in sqs):
        return None
    if golden_only and not sqs:
        # No assertion -> can't confirm the action landed; skip for golden set.
        return None

    system = fullest_system(trace_lines)
    if not system:
        # Without the system prompt the prompt-freezing contract is violated.
        return None

    tools = anthropic_tools_to_openai(trace_lines[-1].get("tools") if trace_lines else None)

    # A CaseRecord has one turn_record per top-level user message; multi-message
    # cases produce several. Concatenate their raw_messages into one conversation.
    chat: list[dict] = [{"role": "system", "content": system}]
    for tr in case_record.get("turn_records", []):
        chat.extend(raw_messages_to_chat(tr.get("raw_messages", [])))

    return {
        "messages": chat,
        "tools": tools,
        "case_id": case_record.get("case_id"),
        "label": {q.get("type"): bool(q.get("passed")) for q in sqs},
    }


# ---------------------------------------------------------------------------
# Template rendering (verification / explicit-text output)
# ---------------------------------------------------------------------------

def _env():
    from jinja2 import Environment, BaseLoader

    env = Environment(loader=BaseLoader())
    # HF chat templates call `tojson(ensure_ascii=False)`; stock jinja2's filter
    # rejects that kwarg. Register an HF-compatible one.
    def _tojson(value, ensure_ascii=True, **_kw):
        return json.dumps(value, ensure_ascii=ensure_ascii)

    env.filters["tojson"] = _tojson
    return env


def render_minicpm_text(
    messages: list[dict],
    tools: list[dict] | None = None,
    *,
    add_generation_prompt: bool = False,
    enable_thinking: Any = None,
) -> str:
    """Apply the vendored MiniCPM5 chat template -> exact ChatML text.

    Use this to eyeball/verify what the model will see. For actual training,
    prefer emitting ``{"messages", "tools"}`` and letting the framework apply
    the MiniCPM5 tokenizer template (same template, guaranteed match).

    ``enable_thinking`` is left unset by default to mirror the documented
    llama.cpp ``--jinja`` deployment (no ``enable_thinking`` passed -> the
    template emits no ``<think>`` block). Pass ``False`` to force the empty
    ``<think>\\n\\n</think>`` block.
    """
    tpl = _env().from_string(TEMPLATE_PATH.read_text())
    ctx = {
        "messages": messages,
        "tools": tools or [],
        "add_generation_prompt": add_generation_prompt,
        "bos_token": "",
        "has_tool_sep": False,
    }
    if enable_thinking is not None:
        ctx["enable_thinking"] = enable_thinking
    return tpl.render(**ctx)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def _load_trace(trace_dir: Path) -> list[dict]:
    p = trace_dir / "anthropic_trace.jsonl"
    if not p.exists():
        return []
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Render NeoMind eval traces -> MiniCPM5 SFT JSONL")
    ap.add_argument("case_record", type=Path, help="CaseRecord JSON (run-case output)")
    ap.add_argument("--trace-dir", type=Path, default=Path("/tmp/neomind-trace"),
                    help="dir containing anthropic_trace.jsonl (NEOMIND_TRACE_DIR)")
    ap.add_argument("-o", "--out", type=Path, default=None, help="output JSONL (default stdout)")
    ap.add_argument("--include-non-golden", action="store_true",
                    help="emit even failed state_query cases (useful for debugging format)")
    ap.add_argument("--show-text", action="store_true",
                    help="also print the rendered MiniCPM ChatML text for the first example")
    args = ap.parse_args(argv)

    cr = json.loads(args.case_record.read_text())
    trace = _load_trace(args.trace_dir)
    if not trace:
        print(f"warning: no anthropic_trace.jsonl in {args.trace_dir} "
              f"(set NEOMIND_TRACE_DIR when running the eval)", file=sys.stderr)

    example = case_to_sft(cr, trace, golden_only=not args.include_non_golden)
    if example is None:
        reason = "non-golden (state_query failed) — rerun with --include-non-golden to emit anyway" \
            if not args.include_non_golden else "no system prompt in trace"
        print(f"filtered: {reason}", file=sys.stderr)
        return 1

    lines = [json.dumps(example, ensure_ascii=False)]
    if args.show_text:
        text = render_minicpm_text(example["messages"], example["tools"])
        lines.append(json.dumps({"_rendered_text": text}, ensure_ascii=False))

    out = "\n".join(lines) + "\n"
    if args.out:
        args.out.write_text(out)
        print(f"wrote {len(lines)} example(s) to {args.out}", file=sys.stderr)
    else:
        sys.stdout.write(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

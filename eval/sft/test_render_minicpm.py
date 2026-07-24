"""Tests for eval.sft.render_minicpm.

Run standalone:  .venv/bin/python eval/sft/test_render_minicpm.py
Or with pytest:  .venv/bin/python -m pytest eval/sft/test_render_minicpm.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# allow running from repo root without install
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from eval.sft import render_minicpm as R  # noqa: E402


# --- conversions -----------------------------------------------------------

def test_anthropic_tools_to_openai_shape():
    tools = [{"name": "shell", "description": "run cmd", "input_schema": {"type": "object"}}]
    out = R.anthropic_tools_to_openai(tools)
    assert out == [{
        "type": "function",
        "function": {
            "name": "shell",
            "description": "run cmd",
            "parameters": {"type": "object"},
        },
    }]


def test_anthropic_tools_to_openai_handles_missing_schema():
    # input_schema absent -> default empty object schema (never null/missing)
    out = R.anthropic_tools_to_openai([{"name": "x"}])
    assert out[0]["function"]["parameters"] == {"type": "object", "properties": {}}
    assert out[0]["function"]["description"] == ""


def test_raw_messages_to_chat_keeps_dict_arguments():
    # The MiniCPM template iterates arguments.items() -> MUST stay a dict.
    raw = [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": "", "tool_calls": [
            {"name": "skill", "arguments": {"action": "load", "id": "device-onboarding"}}]},
        {"role": "tool", "content": '{"ok": true}'},
    ]
    chat = R.raw_messages_to_chat(raw)
    assert chat[0] == {"role": "user", "content": "hi"}
    assert chat[1]["role"] == "assistant"
    tc = chat[1]["tool_calls"][0]
    assert tc["function"]["name"] == "skill"
    # dict, not JSON-string — MiniCPM5-specific
    assert tc["function"]["arguments"] == {"action": "load", "id": "device-onboarding"}
    assert isinstance(tc["function"]["arguments"], dict)
    assert chat[2] == {"role": "tool", "content": '{"ok": true}'}


def test_raw_messages_to_chat_assistant_without_tool_calls():
    chat = R.raw_messages_to_chat([{"role": "assistant", "content": "done"}])
    assert chat == [{"role": "assistant", "content": "done"}]
    assert "tool_calls" not in chat[0]


def test_fullest_system_picks_longest():
    lines = [{"system": "short"}, {"system": "x" * 500}, {"system": "medium text"}]
    assert R.fullest_system(lines) == "x" * 500
    assert R.fullest_system([]) == ""
    assert R.fullest_system([{"system": None}]) == ""


# --- case_to_sft join + filter --------------------------------------------

def _cr(passed, with_trace=True):
    return {
        "case_id": "c1",
        "state_queries": [{"type": "device_exists", "passed": passed}],
        "turn_records": [{
            "raw_messages": [
                {"role": "user", "content": "do it"},
                {"role": "assistant", "content": "", "tool_calls": [
                    {"name": "shell", "arguments": {"command": "neomind device create"}}]},
            ],
        }],
    }, ([{"system": "SYS", "tools": [{"name": "shell"}]}] if with_trace else [])


def test_case_to_sft_filters_non_golden():
    cr, trace = _cr(passed=False)
    assert R.case_to_sft(cr, trace, golden_only=True) is None


def test_case_to_sft_emits_golden():
    cr, trace = _cr(passed=True)
    ex = R.case_to_sft(cr, trace, golden_only=True)
    assert ex is not None
    assert ex["messages"][0] == {"role": "system", "content": "SYS"}
    # [0]=system, [1]=user, [2]=assistant(tool_calls)
    assert ex["messages"][2]["tool_calls"][0]["function"]["name"] == "shell"
    assert ex["label"] == {"device_exists": True}
    assert ex["tools"][0]["function"]["name"] == "shell"


def test_case_to_sft_non_golden_emitted_when_requested():
    cr, trace = _cr(passed=False)
    assert R.case_to_sft(cr, trace, golden_only=False) is not None


def test_case_to_sft_requires_system_prompt():
    cr, trace = _cr(passed=True, with_trace=False)
    assert R.case_to_sft(cr, trace, golden_only=True) is None  # no system -> skip


def test_case_to_sft_skips_when_no_state_query():
    # Can't confirm the action landed -> not golden.
    cr = {"case_id": "c", "state_queries": [], "turn_records": [{"raw_messages": []}]}
    assert R.case_to_sft(cr, [{"system": "S"}], golden_only=True) is None


# --- template rendering ----------------------------------------------------

def test_render_minicpm_text_has_all_markers():
    msgs = [
        {"role": "system", "content": "You are NeoMind."},
        {"role": "user", "content": "create a device"},
        {"role": "assistant", "content": "", "tool_calls": [{
            "type": "function",
            "function": {"name": "shell", "arguments": {"command": "neomind device create"}}}]},
        {"role": "tool", "content": '{"success": true}'},
        {"role": "assistant", "content": "Done."},
    ]
    tools = [{"type": "function", "function": {
        "name": "shell", "description": "d", "parameters": {"type": "object"}}}]
    text = R.render_minicpm_text(msgs, tools)
    assert "<|im_start|>system" in text
    assert "<function name=\"shell\">" in text
    assert "<param name=\"command\">" in text
    assert "neomind device create" in text
    assert "<tool_response>" in text and "</tool_response>" in text
    assert text.rstrip().endswith("<|im_end|>")


def test_render_minicpm_text_cdata_for_multiline_param():
    # Template rule: param value with '<', '&', or '\n' -> CDATA-wrapped.
    msgs = [
        {"role": "assistant", "content": "", "tool_calls": [{
            "type": "function",
            "function": {"name": "shell", "arguments": {"command": "echo a\necho b"}}}]},
    ]
    text = R.render_minicpm_text(msgs, None)
    assert "<![CDATA[echo a\necho b]]>" in text


def test_render_minicpm_text_generation_prompt():
    msgs = [{"role": "user", "content": "hi"}]
    text = R.render_minicpm_text(msgs, None, add_generation_prompt=True)
    # Template emits the bare generation prompt (no <think> block when
    # enable_thinking is unset, matching the llama.cpp --jinja deployment).
    assert text.endswith("<|im_start|>assistant\n")
    # default (enable_thinking unset) -> no <think> block, matching the
    # documented llama.cpp --jinja deployment.
    assert "<think>" not in text


# --- real-data smoke (skips if the trace dir isn't present) ----------------

REAL = Path("/tmp/neomind-trace")


def test_real_data_smoke():
    cr_path = REAL / "caserecord.json"
    tr_path = REAL / "anthropic_trace.jsonl"
    if not (cr_path.exists() and tr_path.exists()):
        import pytest
        pytest.skip("no /tmp/neomind-trace teacher data; run a teacher run-case first")
    cr = json.loads(cr_path.read_text())
    trace = [json.loads(l) for l in tr_path.read_text().splitlines() if l.strip()]
    ex = R.case_to_sft(cr, trace, golden_only=False)
    assert ex is not None, "should emit with golden_only=False"
    assert ex["messages"][0]["role"] == "system"
    assert len(ex["messages"][0]["content"]) > 1000  # real system prompt is large
    text = R.render_minicpm_text(ex["messages"], ex["tools"])
    assert "<function name=" in text and "<tool_response>" in text


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"  PASS  {fn.__name__}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"  FAIL  {fn.__name__}: {type(e).__name__}: {e}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(_run_all())

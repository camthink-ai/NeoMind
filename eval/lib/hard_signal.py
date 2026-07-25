"""Hard signals — deterministic pass/fail metrics computed from a CaseRecord.

These are the RELIABLE eval signals. The LLM judge (judge.py) is soft and prone
to score inflation; per project calibration (2026-07-20) the trustworthy signals
are hard assertion/substring matches + manual testing, NOT the judge's weighted
grade.

Two complementary hard signals (each covers a gap the other can't):
  - state_query pass : did the world change correctly?   (mutation cases)
  - tool_match       : did the agent call the right tool/command? (all, esp. reads)
Plus wrong_tool : did the agent grab a clearly-wrong tool — the MiniCPM5-1B
failure mode (file_write/web_fetch instead of shell→neomind)?

compute() is PURE: given a case dict + a CaseRecord dict it returns a `hard`
block. No server, no LLM. So it works retroactively on existing runs'
cases.jsonl and runs judge-free (which means free local-model iteration — the
local model costs nothing to run; only the judge costs money).

Case schema addition (optional, backward-compatible):
    "expect": {"tools": ["shell"], "commands": ["neomind device types list"]}
When absent, expected tools/commands are auto-derived from backtick
`neomind ...` commands and action verbs in the case text (flagged `derived`).
"""
from __future__ import annotations

import json
import re

# Tools almost never correct for platform CRUD (device/rule/agent/dashboard
# create-update-delete). Presence when a neomind op was expected = strong
# "wrong tool" signal — exactly the MiniCPM5-1B failure mode.
# `skill`/`memory` are intentionally NOT here: they can be legitimate (skill
# load before a complex op; memory read at session start). They're surfaced as
# soft "distractor" counts instead.
WRONG_TOOLS = {"file_write", "file_edit", "web_fetch", "image_edit"}
DISTRACTOR_TOOLS = {"skill", "memory"}

# Non-shell tools whose unambiguous name in case text means the case is ABOUT
# that tool (not a neomind op). derive_expected uses this to avoid wrongly
# defaulting expected tool to `shell` — which would both false-fail tool_match
# AND false-flag the legit tool as wrong_tool (the tools-file-write/memory/
# web-fetch cases regressed exactly this way before this guard). `memory` and
# `skill` are detected via a tool-context cue (too ambiguous as bare words).
_NON_SHELL_TOOLS = ("file_write", "file_edit", "web_fetch", "image_edit", "vision")

# `neomind <sub> [<entity>] ...` inside backticks — high-precision extraction.
_BACKTICK_CMD = re.compile(r"`neomind\s+([^`]+)`", re.IGNORECASE)

# Action verbs (en + zh) implying a neomind platform op → expected tool = shell.
_ACTION_VERBS = (
    "create", "add", "new", "delete", "remove", "update", "modify", "enable",
    "disable", "list", "show", "get", "query", "execute", "run", "send",
    "install", "uninstall", "config", "control", "approve", "reject", "reload",
    "新增", "创建", "添加", "删除", "修改", "更新", "启用", "停用", "列出",
    "查询", "获取", "执行", "发送", "安装", "卸载", "配置", "控制", "重载",
)


def normalize_command(cmd: str) -> str:
    """Normalize a neomind command to its 3 leading meaningful tokens.

    Lowercase → cut at shell pipe/redirect → drop flags (tokens starting with
    '-') → take first 3 tokens (``neomind <sub> [<entity>]``).

    >>> normalize_command('neomind device types list --limit 5')
    'neomind device types'
    >>> normalize_command('neomind device create --id cam-lobby')
    'neomind device create'
    >>> normalize_command('neomind agent --help 2>&1 | head -60')
    'neomind agent'
    """
    if not cmd:
        return ""
    s = cmd.strip().lower()
    # Cut at shell pipe/sequence — the neomind command ends there.
    for sep in ("|", ";"):
        if sep in s:
            s = s.split(sep, 1)[0]
    tokens = []
    for t in s.split():
        if not t or t.startswith("-"):
            continue  # flag
        if any(c in t for c in "&<>|"):
            continue  # redirect artifact (2>&1, >file, <in, …)
        tokens.append(t)
    return " ".join(tokens[:3])


def _dedup_norm(commands: list[str]) -> list[str]:
    """Normalize + de-duplicate a list of neomind commands."""
    seen: set[str] = set()
    out: list[str] = []
    for c in commands:
        n = normalize_command(c)
        if n and n not in seen:
            seen.add(n)
            out.append(n)
    return out


def _text_parts(case: dict) -> list[str]:
    """Human-text fields mined for backtick commands / action verbs."""
    parts = [case.get("description") or "", case.get("id") or ""]
    exp = case.get("expectations") or {}
    parts.append(exp.get("overall") or "")
    for t in (exp.get("per_turn") or []):
        if isinstance(t, str):
            parts.append(t)
    for turn in (case.get("turns") or []):
        if isinstance(turn, dict):
            parts.append(turn.get("user") or "")
    return [p for p in parts if p]


def derive_expected(case: dict) -> dict:
    """Resolve expected tools/commands for a case.

    Priority:
      1. Authored field ``expect: {tools, commands}`` (normalized).
      2. Auto-derived from backtick `` `neomind ...` `` commands in the case text.
      3. Action verbs → expected tool = ``shell``.

    Returns ``{tools, commands, derived}``. ``derived=True`` means heuristics,
    not authoring.
    """
    expect = case.get("expect") or {}
    tools = list(expect.get("tools") or [])
    norm_cmds = _dedup_norm(list(expect.get("commands") or []))
    if tools or norm_cmds:
        return {"tools": tools, "commands": norm_cmds, "derived": False}

    blob = " ".join(_text_parts(case))
    blob_l = blob.lower()
    found = [f"neomind {m.group(1).strip()}" for m in _BACKTICK_CMD.finditer(blob)]
    derived_cmds = _dedup_norm(found)

    # Detect explicit non-shell tool mentions. A case about file_write / memory
    # / web_fetch must NOT default expected tool to shell (else the legit tool
    # use false-fails tool_match and gets flagged as wrong_tool).
    mentioned = [t for t in _NON_SHELL_TOOLS if t in blob_l]
    if re.search(r"memory\s*(?:工具|tool|action)", blob_l) and "memory" not in mentioned:
        mentioned.append("memory")
    if re.search(r"skill\s*(?:工具|tool|action)", blob_l) and "skill" not in mentioned:
        mentioned.append("skill")

    derived_tools: list[str] = []
    if mentioned:
        derived_tools = mentioned
    elif derived_cmds or any(v in blob_l for v in _ACTION_VERBS):
        derived_tools = ["shell"]

    return {"tools": derived_tools, "commands": derived_cmds, "derived": True}


def _actual_from_record(record: dict) -> tuple[set[str], list[str]]:
    """Extract actual tool names + normalized shell commands from a CaseRecord."""
    tools: set[str] = set()
    commands: list[str] = []
    for turn in record.get("turn_records") or []:
        for tc in (turn.get("tool_calls") or []):
            name = tc.get("name") or ""
            if not name:
                continue
            tools.add(name)
            if name == "shell":
                args = tc.get("arguments")
                cmd = None
                if isinstance(args, dict):
                    cmd = args.get("command")
                elif isinstance(args, str):
                    try:
                        cmd = json.loads(args).get("command")
                    except Exception:
                        cmd = args
                if isinstance(cmd, str):
                    n = normalize_command(cmd)
                    if n:
                        commands.append(n)
    return tools, commands


def _cmd_ok(expected_cmds: list[str], actual_cmds: list[str]):
    """Every expected normalized command is matched by some actual command.

    Match: expected ``e`` matches actual ``a`` iff ``a == e`` or ``a`` is more
    specific (``a.startswith(e + " ")``). Returns ``None`` if no expected
    commands, ``False`` if any expected command is unmatched.
    """
    if not expected_cmds:
        return None
    if not actual_cmds:
        return False
    for e in expected_cmds:
        if not any(a == e or a.startswith(e + " ") for a in actual_cmds):
            return False
    return True


def compute(case: dict, record: dict) -> dict:
    """Compute the hard-signal block for one case. Pure — no I/O, no LLM.

    ``record`` is a CaseRecord dict (``turn_records``, ``state_queries``,
    ``status`` ...). ``case`` is the case definition (``expect``,
    ``expectations``, ``state_queries`` ...).
    """
    # --- state_query rollup (already evaluated server-side in the record) ---
    sqs = record.get("state_queries") or []
    sq_total = len(sqs)
    sq_pass = sum(1 for q in sqs if q.get("passed"))
    sq_all_pass = (sq_pass == sq_total) if sq_total else None

    expected = derive_expected(case)
    exp_tools = expected["tools"]
    exp_cmds = expected["commands"]

    actual_tools, actual_cmds = _actual_from_record(record)

    cmd_ok = _cmd_ok(exp_cmds, actual_cmds)
    tool_ok = (all(t in actual_tools for t in exp_tools) if exp_tools else None)

    neomind_expected = bool(exp_cmds) or "shell" in exp_tools
    wrong_used = sorted(actual_tools & WRONG_TOOLS)
    distractor_used = sorted(actual_tools & DISTRACTOR_TOOLS)
    wrong_tool = bool(wrong_used) and neomind_expected

    # tool_match_pass: expected tools present AND expected commands matched.
    if exp_tools or exp_cmds:
        parts = []
        if exp_tools:
            parts.append(tool_ok is not False)
        if exp_cmds:
            parts.append(cmd_ok is not False)
        tool_match_pass = all(parts) if parts else None
    else:
        tool_match_pass = None

    has_hard_signal = (sq_total > 0) or bool(exp_tools or exp_cmds)

    return {
        "sq_total": sq_total,
        "sq_pass": sq_pass,
        "sq_all_pass": sq_all_pass,
        "expected_tools": exp_tools,
        "expected_commands": exp_cmds,
        "derived": expected["derived"],
        "actual_tools": sorted(actual_tools),
        "cmd_ok": cmd_ok,
        "tool_ok": tool_ok,
        "wrong_tool": wrong_tool,
        "wrong_tools_used": wrong_used,
        "distractor_tools": distractor_used,
        "tool_match_pass": tool_match_pass,
        "has_hard_signal": has_hard_signal,
        "unasserted": not has_hard_signal,
        "agent_failed": bool(record.get("status")),
    }


def pass_for_case(hard: dict) -> bool | None:
    """Convenience: did this case pass its hard signal?

    - Mutation case (has state_query): pass iff sq_all_pass.
    - Tool-match-only case: pass iff tool_match_pass.
    - Case with both: pass iff both.
    - No hard signal: None (unasserted — reported separately, not counted).
    """
    if hard.get("agent_failed"):
        # Runtime failure (timeout/error), not a model-competence verdict.
        # Excluded from the pass-rate denominator; reported separately.
        return None
    if not hard.get("has_hard_signal"):
        return None
    results = []
    if hard.get("sq_total", 0) > 0:
        results.append(hard.get("sq_all_pass") is True)
    if hard.get("tool_match_pass") is not None:
        results.append(hard.get("tool_match_pass") is True)
    return all(results) if results else None

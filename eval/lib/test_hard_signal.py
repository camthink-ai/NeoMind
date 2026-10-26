"""Tests for hard_signal.py — run standalone: `python3 eval/lib/test_hard_signal.py`.

Covers the four behaviors that matter for the small-model eval loop:
  - command normalization (flag/pipe stripping, 3-token truncation)
  - derive_expected (authored > backtick > verbs; derived flag)
  - compute(): mutation-pass, wrong-tool failure, read-case, agent-failed
  - the cmd_ok specificity direction (actual more specific matches)
"""
from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
import hard_signal as hs  # noqa: E402


def _shell_call(command: str) -> dict:
    return {"name": "shell", "arguments": {"command": command}, "result": "{}"}


def _turn(*tool_calls) -> dict:
    return {"user": "", "assistant_message": "", "tool_calls": list(tool_calls),
            "processing_time_ms": 1000}


# ---------------------------------------------------------------- normalize ----

def test_normalize_strips_flags_and_pipes():
    assert hs.normalize_command("neomind device types list --limit 5") == "neomind device types"
    assert hs.normalize_command("neomind device create --id cam-lobby") == "neomind device create"
    # pipe + redirect cut
    assert hs.normalize_command("neomind agent --help 2>&1 | head -60") == "neomind agent"
    # 4-token command truncates to the 3 leading tokens
    assert hs.normalize_command("neomind device types list") == "neomind device types"
    assert hs.normalize_command("") == ""


# -------------------------------------------------------------- derive_expected

def test_derive_authored_field_wins():
    case = {"expect": {"tools": ["shell"], "commands": ["neomind device create"]},
            "description": "whatever `neomind rule create` says"}
    e = hs.derive_expected(case)
    assert e["derived"] is False
    assert e["tools"] == ["shell"]
    assert e["commands"] == ["neomind device create"]


def test_derive_authored_commands_normalized():
    case = {"expect": {"commands": ["neomind device types list --x"]}}
    e = hs.derive_expected(case)
    assert e["commands"] == ["neomind device types"]


def test_derive_backtick_command_and_shell_tool():
    case = {
        "description": "列出系统支持的设备类型（覆盖 `neomind device types list`）",
        "expectations": {"overall": "Agent 调用 `neomind device types list`"},
    }
    e = hs.derive_expected(case)
    assert e["derived"] is True
    assert e["commands"] == ["neomind device types"]
    assert e["tools"] == ["shell"]  # action verb / neomind mention → shell


def test_derive_unasserted_when_nothing_to_match():
    case = {"description": "纯寒暄", "expectations": {"overall": "打招呼"}}
    e = hs.derive_expected(case)
    assert e["tools"] == []
    assert e["commands"] == []
    assert e["derived"] is True


def test_derive_action_verb_implies_shell_even_without_backtick():
    case = {"description": "新增一个设备", "expectations": {"overall": "create a device"}}
    e = hs.derive_expected(case)
    assert e["tools"] == ["shell"]
    assert e["commands"] == []  # no backtick command → no cmd assertion


# -------------------------------------------------------------------- compute

def test_compute_mutation_pass():
    case = {
        "id": "device-create",
        "description": "新增设备",
        "state_queries": [{"type": "device_exists", "params": {"id": "cam-lobby"},
                           "expected": True}],
    }
    record = {
        "turn_records": [_turn(_shell_call("neomind device create --id cam-lobby --name X"))],
        "state_queries": [{"type": "device_exists", "params": {"id": "cam-lobby"},
                           "expected": True, "actual": True, "passed": True}],
    }
    h = hs.compute(case, record)
    assert h["sq_total"] == 1 and h["sq_pass"] == 1
    assert h["sq_all_pass"] is True
    assert h["cmd_ok"] is None  # no backtick command expected → no cmd assertion
    assert h["tool_ok"] is True
    assert h["tool_match_pass"] is True
    assert h["wrong_tool"] is False
    assert h["has_hard_signal"] is True
    assert hs.pass_for_case(h) is True


def test_compute_wrong_tool_failure_minicpm_style():
    """The MiniCPM5 failure mode: expected shell→neomind, grabbed file_write/web_fetch."""
    case = {"description": "新增设备", "expect": {"tools": ["shell"],
            "commands": ["neomind device create"]}}
    record = {
        "turn_records": [_turn(
            {"name": "file_write", "arguments": {"path": "x", "content": "y"}, "result": ""},
            {"name": "web_fetch", "arguments": {"url": "http://x"}, "result": ""},
        )],
        "state_queries": [],
    }
    h = hs.compute(case, record)
    assert h["cmd_ok"] is False          # no shell ran the expected command
    assert h["tool_ok"] is False         # shell not called
    assert h["tool_match_pass"] is False
    assert h["wrong_tool"] is True
    assert set(h["wrong_tools_used"]) == {"file_write", "web_fetch"}
    assert hs.pass_for_case(h) is False


def test_compute_read_case_no_state_query():
    case = {"description": "覆盖 `neomind device types list`",
            "expectations": {"overall": "调用 `neomind device types list`"}}
    record = {"turn_records": [_turn(_shell_call("neomind device types list"))],
              "state_queries": []}
    h = hs.compute(case, record)
    assert h["sq_all_pass"] is None      # no mutation to assert
    assert h["tool_match_pass"] is True  # but tool/command matched
    assert h["has_hard_signal"] is True
    assert hs.pass_for_case(h) is True


def test_compute_cmd_specificity_actual_more_specific_matches():
    """Agent running a MORE specific command than expected should still match."""
    case = {"expect": {"commands": ["neomind device get"]}}  # expected prefix
    record = {"turn_records": [_turn(_shell_call("neomind device get cam-lobby --metric temp"))]}
    h = hs.compute(case, record)
    assert h["cmd_ok"] is True  # 'neomind device get cam-lobby' startswith 'neomind device get '


def test_compute_cmd_wrong_subcommand_does_not_match():
    case = {"expect": {"commands": ["neomind device delete"]}}
    record = {"turn_records": [_turn(_shell_call("neomind device list"))]}
    h = hs.compute(case, record)
    assert h["cmd_ok"] is False


def test_compute_agent_failed_is_marked():
    case = {"description": "新增设备"}
    record = {"turn_records": [], "state_queries": [], "status": "agent_error",
              "message": "timeout"}
    h = hs.compute(case, record)
    assert h["agent_failed"] is True
    assert h["actual_tools"] == []
    # No tools ran, no sq → unasserted, not a hard pass.
    assert hs.pass_for_case(h) is None


def test_compute_unasserted_case():
    case = {"description": "你好", "expectations": {"overall": "打招呼"}}
    record = {"turn_records": [_turn()], "state_queries": []}
    h = hs.compute(case, record)
    assert h["unasserted"] is True
    assert h["has_hard_signal"] is False
    assert hs.pass_for_case(h) is None


def test_compute_skill_is_distractor_not_wrong_tool():
    """skill load before a real op is legitimate — distractor, not wrong_tool."""
    case = {"expect": {"tools": ["shell"], "commands": ["neomind rule create"]}}
    record = {"turn_records": [_turn(
        {"name": "skill", "arguments": {"action": "load", "id": "rule-management"}, "result": ""},
        _shell_call("neomind rule create --name x"),
    )]}
    h = hs.compute(case, record)
    assert h["wrong_tool"] is False
    assert "skill" in h["distractor_tools"]
    assert h["tool_match_pass"] is True


# --------------------------------------------------------------------- runner

def test_derive_non_shell_tool_not_forced_to_shell():
    """A case about file_write must expect file_write, not shell."""
    case = {"description": "通过 file_write 工具落盘一个 JSON 配置文件"}
    e = hs.derive_expected(case)
    assert e["tools"] == ["file_write"]
    assert e["commands"] == []


def test_derive_memory_tool_with_context_cue():
    case = {"description": "通过 memory 工具持久化一条用户偏好"}
    e = hs.derive_expected(case)
    assert e["tools"] == ["memory"]


def test_compute_tools_case_passes_when_correct():
    """tools-file-write case: agent used file_write correctly → PASS, not wrong_tool."""
    case = {"description": "通过 file_write 工具落盘一个 JSON 配置文件并确认写入成功"}
    record = {"turn_records": [_turn(
        {"name": "file_write", "arguments": {"path": "/tmp/x.json", "content": "{}"}, "result": ""},
    )], "state_queries": []}
    h = hs.compute(case, record)
    assert h["wrong_tool"] is False   # file_write is the EXPECTED tool here
    assert h["tool_ok"] is True
    assert h["tool_match_pass"] is True
    assert hs.pass_for_case(h) is True


# ------------------------------------------------- exit-code (P0-1, 2026-10-26)

def test_failed_exit_code_does_not_satisfy_expectation():
    """A command the CLI rejected (clap error, exit 2) must not count.

    Regression shape: settings set-retention --enabled true was rejected by
    clap but still matched `neomind settings set-retention` and PASSED.
    """
    case = {"expect": {"tools": ["shell"],
                       "commands": ["neomind settings set-retention"]}}
    call = {"name": "shell",
            "arguments": {"command": "neomind settings set-retention --interval-hours 1 --enabled true"},
            "result": '{"exit_code": 2, "stderr": "unexpected argument \'true\'"}'}
    rec = {"turn_records": [_turn(call)], "status": None}
    h = hs.compute(case, rec)
    assert h["cmd_ok"] is False, "rejected command must not satisfy expectation"
    assert h["failed_cmds"] == ["neomind settings set-retention"]
    assert h["tool_match_pass"] is False


def test_zero_exit_code_and_legacy_records_still_pass():
    case = {"expect": {"commands": ["neomind device list"]}}
    ok = {"name": "shell", "arguments": {"command": "neomind device list"},
          "result": '{"exit_code": 0, "stdout": "[]"}'}
    legacy = _shell_call("neomind device list")  # result has no exit_code
    for call in (ok, legacy):
        h = hs.compute(case, {"turn_records": [_turn(call)], "status": None})
        assert h["cmd_ok"] is True
        assert h["failed_cmds"] == []


# ---------------------------------------------------- substrings (P1-7) ----

def test_substrings_assert_arguments():
    case = {"expect": {"commands": ["neomind settings set-retention"],
                       "substrings": ["--interval-hours", "168"]}}
    good = {"name": "shell",
            "arguments": {"command": "neomind settings set-retention --interval-hours 1 --default-retention 168"},
            "result": '{"exit_code": 0}'}
    bad = {"name": "shell",
           "arguments": {"command": "neomind settings set-retention --interval-hours 24"},
           "result": '{"exit_code": 0}'}
    h = hs.compute(case, {"turn_records": [_turn(good)], "status": None})
    assert h["substrings_ok"] is True and h["tool_match_pass"] is True
    h2 = hs.compute(case, {"turn_records": [_turn(bad)], "status": None})
    assert h2["substrings_ok"] is False and h2["tool_match_pass"] is False


def test_scoped_substrings_bind_flags_to_their_verb():
    """Dict shape: a flag must appear on a command matching ITS scope, not on
    any sibling command of the case."""
    case = {"expect": {
        "commands": ["neomind settings retention", "neomind settings set-retention"],
        "substrings": {"neomind settings set-retention": ["--interval-hours", "168"]}}}
    right = {"name": "shell",
             "arguments": {"command": "neomind settings set-retention --interval-hours 1 --default-retention 168"},
             "result": '{"exit_code": 0}'}
    # flag present in the case but attached to the WRONG verb -> fail
    wrong_verb = [
        {"name": "shell", "arguments": {"command": "neomind settings retention --format json"},
         "result": '{"exit_code": 0}'},
        {"name": "shell", "arguments": {"command": "neomind settings cleanup"},
         "result": '{"exit_code": 0}'},
    ]
    h = hs.compute(case, {"turn_records": [_turn(right)], "status": None})
    assert h["substrings_ok"] is True
    h2 = hs.compute(case, {"turn_records": [_turn(*wrong_verb)], "status": None})
    assert h2["substrings_ok"] is False


# ------------------------------------------------ product view (P0-2) ----

def test_product_view_counts_agent_failed_as_fail():
    h = {"agent_failed": True, "has_hard_signal": True,
         "sq_total": 0, "tool_match_pass": True}
    assert hs.pass_for_case(h) is None        # diagnostic: excluded
    assert hs.pass_for_case_product(h) is False  # product: a hung turn failed
    h2 = {"agent_failed": False, "has_hard_signal": True,
          "sq_total": 1, "sq_all_pass": True, "tool_match_pass": True}
    assert hs.pass_for_case_product(h2) is True


# ------------------------------------------------ any-of groups (2026-10-26)

def test_any_of_group_matches_any_sibling():
    """A list entry in expect.commands passes when ANY member matches.

    Read-type asks admit several valid verbs; pinning one sibling false-failed
    models choosing another (audit found 9 such cases).
    """
    case = {"expect": {"commands": [["neomind system info", "neomind device list"]]}}
    for cmd in ("neomind system info", "neomind device list",
                "neomind device list --format json"):
        h = hs.compute(case, {"turn_records": [_turn(_shell_call(cmd))], "status": None})
        assert h["cmd_ok"] is True, cmd
    # a sibling NOT in the group still fails
    h = hs.compute(case, {"turn_records": [_turn(_shell_call("neomind device types"))],
                          "status": None})
    assert h["cmd_ok"] is False


def test_mixed_required_and_any_of():
    case = {"expect": {"commands": [
        "neomind rule create",                       # required
        ["neomind device get", "neomind device latest"],  # any-of
    ]}}
    good = [
        _shell_call("neomind rule create --name r1"),
        _shell_call("neomind device latest sensor-001"),
    ]
    h = hs.compute(case, {"turn_records": [_turn(*good)], "status": None})
    assert h["cmd_ok"] is True
    # required command missing -> fail even though any-of matched
    h2 = hs.compute(case, {"turn_records": [_turn(_shell_call("neomind device latest"))],
                           "status": None})
    assert h2["cmd_ok"] is False


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"  PASS  {fn.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"  FAIL  {fn.__name__}: {e!r}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"  ERROR {fn.__name__}: {e!r}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(_run_all())

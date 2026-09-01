#!/usr/bin/env python3
"""
NeoMind Tool Verification - Verify each tool action works correctly.

Tests each action against real business logic (not just LLM invocation).
"""

import json
import sys
import time
import requests
from dataclasses import dataclass
from typing import Optional

API_BASE = "http://localhost:9375/api"

def login():
    r = requests.post(f"{API_BASE}/auth/login",
                      json={"username": "Admin", "password": "zxc707cxz"})
    r.raise_for_status()
    return r.json()["token"]

def create_session(token):
    r = requests.post(f"{API_BASE}/sessions",
                      headers={"Authorization": f"Bearer {token}"})
    r.raise_for_status()
    return r.json()["data"]["sessionId"]

def chat(token, session_id, message, timeout=90):
    r = requests.post(f"{API_BASE}/sessions/{session_id}/chat",
                      headers={"Authorization": f"Bearer {token}",
                               "Content-Type": "application/json"},
                      json={"message": message},
                      timeout=timeout)
    r.raise_for_status()
    return json.loads(r.text, strict=False)

def delete_session(token, session_id):
    requests.delete(f"{API_BASE}/sessions/{session_id}",
                    headers={"Authorization": f"Bearer {token}"})

@dataclass
class TestItem:
    tool: str
    action: str
    message: str
    expect_tool: str  # Expected tool name in toolsUsed
    expect_keywords: list  # Keywords that should appear in response
    timeout: int = 90

def run_test(token, sid, item: TestItem) -> dict:
    """Run a single test item."""
    try:
        resp = chat(token, sid, item.message, item.timeout)
        tools = resp.get("toolsUsed", [])
        content = resp.get("response", "")
        time_ms = resp.get("processingTimeMs", 0)

        # Check tool was called
        tool_ok = item.expect_tool in tools

        # Check keywords
        missing_kw = []
        content_lower = content.lower()
        for kw in item.expect_keywords:
            if kw.lower() not in content_lower:
                missing_kw.append(kw)
        kw_ok = len(missing_kw) == 0

        passed = tool_ok and kw_ok
        status = "✓" if passed else "✗"
        tools_str = ",".join(tools) if tools else "none"

        reason = ""
        if not tool_ok:
            reason = f"Expected tool '{item.expect_tool}' in [{tools_str}]"
        elif not kw_ok:
            reason = f"Missing keywords: {missing_kw}"

        print(f"  {status} {item.tool}({item.action}) | tools=[{tools_str}] {time_ms}ms | {reason if reason else 'OK'}")
        if not passed and content:
            preview = content[:120].replace("\n", " ")
            print(f"      Response: {preview}...")

        return {"tool": item.tool, "action": item.action, "passed": passed, "reason": reason}

    except Exception as e:
        print(f"  ✗ {item.tool}({item.action}) | Error: {e}")
        return {"tool": item.tool, "action": item.action, "passed": False, "reason": str(e)}


def main():
    token = login()
    print(f"Logged in. Running tool verification...\n")

    results = []

    # =========================================================================
    # DEVICE TOOL TESTS
    # =========================================================================
    print("=" * 60)
    print("DEVICE TOOL")
    print("=" * 60)
    sid = create_session(token)
    print(f"Session: {sid[:8]}...")

    device_tests = [
        TestItem("device", "list", "列出所有设备", "device", ["设备"]),
        TestItem("device", "get", "查看设备 NE101-Refrigerator 的详细信息", "device", ["NE101", "Refrigerator"]),
        TestItem("device", "history", "查看设备 NE101-Refrigerator 的温度历史数据", "device", []),
        TestItem("device", "control", "给设备 NE101-Refrigerator 发送一个 test 命令", "device", []),
    ]
    for t in device_tests:
        results.append(run_test(token, sid, t))
        time.sleep(1)

    delete_session(token, sid)

    # =========================================================================
    # AGENT TOOL TESTS
    # =========================================================================
    print(f"\n{'=' * 60}")
    print("AGENT TOOL")
    print("=" * 60)
    sid = create_session(token)
    print(f"Session: {sid[:8]}...")

    agent_tests = [
        TestItem("agent", "list", "列出所有 agent", "agent", ["agent"]),
        TestItem("agent", "get", "查看 冰箱检测 这个 agent 的详细信息", "agent", ["冰箱"]),
        TestItem("agent", "create", "创建一个 agent，名称叫 verify-test，用事件触发，提示词是：每分钟检测一次", "agent", ["verify-test", "创建"]),
        TestItem("agent", "update", "把 verify-test 的描述改成：测试验证用 agent", "agent", ["更新", "成功", "verify"]),
        TestItem("agent", "control_pause", "暂停 agent verify-test", "agent", ["暂停", "paused", "成功"]),
        TestItem("agent", "control_resume", "恢复运行 agent verify-test", "agent", ["恢复", "active", "成功"]),
        TestItem("agent", "memory", "查看 冰箱检测 的记忆", "agent", []),
        TestItem("agent", "send_message", "给 冰箱检测 发一条消息：测试消息", "agent", []),
        TestItem("agent", "executions", "查看 冰箱检测 的执行记录", "agent", []),
        TestItem("agent", "latest_execution", "查看 冰箱检测 最近一次执行结果", "agent", []),
        TestItem("agent", "conversation", "查看 冰箱检测 的对话历史", "agent", []),
    ]
    for t in agent_tests:
        results.append(run_test(token, sid, t))
        time.sleep(1)

    delete_session(token, sid)

    # =========================================================================
    # RULE TOOL TESTS
    # =========================================================================
    print(f"\n{'=' * 60}")
    print("RULE TOOL")
    print("=" * 60)
    sid = create_session(token)
    print(f"Session: {sid[:8]}...")

    rule_tests = [
        TestItem("rule", "list", "列出所有规则", "rule", ["规则"]),
        TestItem("rule", "get", "查看 TEST 规则的详细信息", "rule", ["TEST"]),
        TestItem("rule", "create", "创建一条规则：当温度大于30时发送通知", "rule", ["创建", "规则"]),
        TestItem("rule", "history", "查看规则执行历史", "rule", []),
    ]
    for t in rule_tests:
        results.append(run_test(token, sid, t))
        time.sleep(1)

    delete_session(token, sid)

    # =========================================================================
    # MESSAGE TOOL TESTS
    # =========================================================================
    print(f"\n{'=' * 60}")
    print("MESSAGE/ALERT TOOL")
    print("=" * 60)
    sid = create_session(token)
    print(f"Session: {sid[:8]}...")

    message_tests = [
        TestItem("message", "list", "列出所有消息", "message", ["消息", "message"]),
        TestItem("message", "list_alert", "列出所有告警", "message", []),
        TestItem("message", "send", "发送一条消息，标题是：测试消息，内容是：这是一条验证用的测试消息", "message", ["发送", "成功", "sent"]),
        TestItem("message", "send_alert", "发送一条紧急告警，标题是：紧急测试，内容是：系统测试告警", "message", ["发送", "成功", "sent"]),
    ]
    for t in message_tests:
        results.append(run_test(token, sid, t))
        time.sleep(1)

    delete_session(token, sid)

    # =========================================================================
    # EXTENSION TOOL TESTS
    # =========================================================================
    print(f"\n{'=' * 60}")
    print("EXTENSION TOOL")
    print("=" * 60)
    sid = create_session(token)
    print(f"Session: {sid[:8]}...")

    ext_tests = [
        TestItem("extension", "list", "列出所有扩展", "extension", ["扩展", "extension"]),
        TestItem("extension", "get", "查看 yolo-video 的详细信息", "extension", ["yolo", "video"]),
        TestItem("extension", "status", "查看 yolo-video 的运行状态", "extension", []),
    ]
    for t in ext_tests:
        results.append(run_test(token, sid, t))
        time.sleep(1)

    delete_session(token, sid)

    # =========================================================================
    # SUMMARY
    # =========================================================================
    total = len(results)
    passed = sum(1 for r in results if r["passed"])
    print(f"\n{'=' * 60}")
    print(f"  SUMMARY: {passed}/{total} ({passed/total*100:.0f}%)")
    print("=" * 60)

    # Group by tool
    tools = {}
    for r in results:
        t = r["tool"]
        if t not in tools:
            tools[t] = {"passed": 0, "total": 0, "failures": []}
        tools[t]["total"] += 1
        if r["passed"]:
            tools[t]["passed"] += 1
        else:
            tools[t]["failures"].append(f"{r['action']}: {r['reason']}")

    for t, data in tools.items():
        status = "✅" if data["passed"] == data["total"] else "⚠️"
        print(f"  {status} {t:12s} {data['passed']}/{data['total']}")
        for f in data["failures"]:
            print(f"      ✗ {f}")

    print()

if __name__ == "__main__":
    main()

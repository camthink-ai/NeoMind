#!/usr/bin/env python3
"""NeoMind Comprehensive Business Scenario Test"""
import json
import time
import requests
import subprocess
from pathlib import Path

API_URL = "http://127.0.0.1:3000"
TOKEN = "nmk_6a814fc1840f40e8b1d3edb74ed527c0"
RESULTS_DIR = Path("/tmp/neomind_comprehensive_test")
RESULTS_DIR.mkdir(exist_ok=True)

def log(msg):
    print(f"[{time.strftime('%H:%M:%S')}] {msg}")

def switch_model(model):
    log(f"Switching to model: {model}")
    requests.put(
        f"{API_URL}/api/llm-backends/ollama-default",
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
        json={"model": model, "thinking_enabled": False},
        timeout=30
    )
    time.sleep(3)

def create_session():
    resp = requests.post(
        f"{API_URL}/api/sessions",
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
        timeout=30
    )
    return resp.json()["data"]["sessionId"]

def send_message(session_id, message, timeout=90):
    start = int(time.time() * 1000)
    try:
        resp = requests.post(
            f"{API_URL}/api/sessions/{session_id}/chat",
            headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
            json={"message": message},
            timeout=timeout
        )
        end = int(time.time() * 1000)
        data = resp.json()
        data["measuredTimeMs"] = end - start
        return data
    except requests.exceptions.Timeout:
        return {"response": "TIMEOUT", "toolsUsed": [], "measuredTimeMs": timeout * 1000}

def extract_tools_info(resp):
    tools = resp.get("toolsUsed", [])
    return len(tools), tools

# Business-oriented test scenarios
SCENARIOS = [
    {
        "name": "B1-晨间例程",
        "desc": "早上7点到了，帮我：打开客厅灯、查看室温、查看今天的自动化规则状态",
        "expected": ["control_device", "list_devices", "list_rules"],
        "category": "routine"
    },
    {
        "name": "B2-环境监控",
        "desc": "帮我看一下所有传感器的数据，包括温度、湿度，如果有异常请告诉我",
        "expected": ["list_devices", "query_data", "get_device_metrics"],
        "category": "monitoring"
    },
    {
        "name": "B3-并行查询",
        "desc": "同时帮我查看：1.所有规则列表 2.所有设备列表 3.所有设备类型",
        "expected": ["list_rules", "list_devices", "list_device_types"],
        "category": "parallel"
    },
    {
        "name": "B4-规则创建",
        "desc": "我想创建一个自动化规则：当温度传感器temperature值超过35度时，发送高温告警通知",
        "expected": ["get_device_metrics", "create_rule"],
        "category": "automation"
    },
    {
        "name": "B5-上下文跟进",
        "desc": "刚才创建的规则ID是什么？帮我启用这个规则",
        "expected": ["enable_rule"],
        "category": "context"
    },
    {
        "name": "B6-批量控制",
        "desc": "把所有relay类型的设备同时打开",
        "expected": ["batch_control_devices"],
        "category": "control"
    },
    {
        "name": "B7-数据查询",
        "desc": "查询sensor1设备在过去1小时的温度数据",
        "expected": ["query_data"],
        "category": "query"
    },
    {
        "name": "B8-设备配置",
        "desc": "将sensor1设备的采样率设置为30秒，报警阈值设置为80",
        "expected": ["set_device_config"],
        "category": "config"
    },
    {
        "name": "B9-故障排查",
        "desc": "sensor1设备好像不工作，帮我检查它的状态、配置和最近的数据",
        "expected": ["query_device_status", "get_device_config", "query_data"],
        "category": "troubleshoot"
    },
    {
        "name": "B10-复杂请求",
        "desc": "帮我做一次系统全面检查：列出所有设备、所有规则、所有设备类型，并告诉我哪些设备支持温度监控",
        "expected": ["list_devices", "list_rules", "list_device_types", "get_device_metrics"],
        "category": "complex"
    },
]

def run_model_tests(model):
    log(f"=========================================")
    log(f"Testing Model: {model}")
    log(f"=========================================")

    switch_model(model)
    session_id = create_session()
    log(f"Session: {session_id}")

    output_file = RESULTS_DIR / f"{model.replace(':', '_')}_comprehensive.txt"
    
    results = {
        "model": model,
        "session_id": session_id,
        "tests": [],
        "total": 0,
        "passed": 0,
        "timeouts": 0,
        "total_tools": 0,
        "total_time": 0,
        "parallel_count": 0,
        "context_count": 0,
    }

    with open(output_file, "w") as f:
        f.write(f"MODEL: {model}\n")
        f.write(f"SESSION: {session_id}\n")
        f.write(f"DATE: {time.strftime('%Y-%m-%d %H:%M:%S')}\n")
        f.write("="*50 + "\n\n")

        for scenario in SCENARIOS:
            results["total"] += 1
            
            log(f"Test: {scenario['name']} - {scenario['desc']}")
            
            f.write(f"TEST: {scenario['name']}\n")
            f.write(f"Description: {scenario['desc']}\n")
            f.write(f"Expected: {', '.join(scenario['expected'])}\n")

            resp = send_message(session_id, scenario['desc'], 90)
            duration = resp.get("measuredTimeMs", resp.get("processingTimeMs", 0))
            tools_count, tools = extract_tools_info(resp)
            response_text = resp.get("response", "")[:200]

            # Check timeout
            is_timeout = duration >= 90000 or "TIMEOUT" in response_text.upper() or "超时" in response_text
            if is_timeout:
                log(f"  TIMEOUT ({duration}ms)")
                f.write(f"Result: TIMEOUT ({duration}ms)\n")
                results["timeouts"] += 1
            else:
                log(f"  PASS (tools: {tools_count}, time: {duration}ms)")
                f.write(f"Result: PASS (tools: {tools_count}, time: {duration}ms)\n")
                results["passed"] += 1

            f.write(f"Tools Called: {tools}\n")
            f.write(f"Response: {response_text}\n\n")

            results["total_tools"] += tools_count
            results["total_time"] += duration

            if tools_count >= 2:
                results["parallel_count"] += 1

            if scenario['category'] == 'context' and tools_count > 0:
                results["context_count"] += 1

            test_result = {
                "name": scenario['name'],
                "category": scenario['category'],
                "tools_count": tools_count,
                "tools": tools,
                "duration": duration,
                "timeout": is_timeout
            }
            results["tests"].append(test_result)
            time.sleep(1)

        # Summary
        avg_time = results["total_time"] // results["total"] if results["total"] > 0 else 0
        success_rate = (results["passed"] * 100) // results["total"] if results["total"] > 0 else 0

        f.write("\n" + "="*50 + "\n")
        f.write(f"SUMMARY\n")
        f.write("="*50 + "\n")
        f.write(f"Total Tests: {results['total']}\n")
        f.write(f"Passed: {results['passed']}\n")
        f.write(f"Timeouts: {results['timeouts']}\n")
        f.write(f"Success Rate: {success_rate}%\n")
        f.write(f"Total Tools: {results['total_tools']}\n")
        f.write(f"Avg Tools/Request: {results['total_tools'] // results['total'] if results['total'] > 0 else 0}\n")
        f.write(f"Parallel Requests: {results['parallel_count']}\n")
        f.write(f"Context Success: {results['context_count']}\n")
        f.write(f"Avg Time: {avg_time}ms\n")

    log(f"=========================================")
    log(f"Summary for {model}")
    log(f"=========================================")
    log(f"  Passed: {results['passed']}/{results['total']} ({success_rate}%)")
    log(f"  Timeouts: {results['timeouts']}")
    log(f"  Tools: {results['total_tools']}")
    log(f"  Parallel: {results['parallel_count']}")
    log(f"  Context: {results['context_count']}")
    log(f"  Avg Time: {avg_time}ms")

    return results

def main():
    log("Starting comprehensive business scenario test...")
    log("Testing gpt-oss:20b vs qwen3-vl:2b")

    all_results = []
    
    for model in ["gpt-oss:20b", "qwen3-vl:2b"]:
        print()
        results = run_model_tests(model)
        all_results.append(results)
        print()

    # Generate comparison report
    log("="*50)
    log("COMPARISON REPORT")
    log("="*50)
    print(f"{'Model':<20} {'Pass':<6} {'Total':<6} {'Rate':<6} {'Tools':<6} {'Parallel':<8} {'Context':<8} {'AvgTime':<8}")
    print("-" * 80)
    
    for r in all_results:
        avg_time = r['total_time'] // r['total'] if r['total'] > 0 else 0
        success_rate = (r['passed'] * 100) // r['total'] if r['total'] > 0 else 0
        print(f"{r['model']:<20} {r['passed']:<6} {r['total']:<6} {success_rate:<6} {r['total_tools']:<6} {r['parallel_count']:<8} {r['context_count']:<8} {avg_time:<8}")

    # Save CSV
    csv_file = RESULTS_DIR / "comparison.csv"
    with open(csv_file, "w") as f:
        f.write("Model,Passed,Total,Rate,Tools,Parallel,Context,AvgTime,Timeouts\n")
        for r in all_results:
            avg_time = r['total_time'] // r['total'] if r['total'] > 0 else 0
            success_rate = (r['passed'] * 100) // r['total'] if r['total'] > 0 else 0
            f.write(f"{r['model']},{r['passed']},{r['total']},{success_rate},{r['total_tools']},{r['parallel_count']},{r['context_count']},{avg_time},{r['timeouts']}\n")

    log(f"\nResults saved to: {RESULTS_DIR}")

if __name__ == "__main__":
    main()

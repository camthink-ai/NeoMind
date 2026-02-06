#!/bin/bash
# Comprehensive Model Comparison Test for NeoMind

set -e

API_URL="http://127.0.0.1:3000"
API_TOKEN="nmk_6a814fc1840f40e8b1d3edb74ed527c0"
RESULTS_DIR="/tmp/neomind_model_tests"
mkdir -p "$RESULTS_DIR"

log() { echo "[$(date '+%H:%M:%S')] $1"; }
success() { echo "✓ $1"; }
error() { echo "✗ $1"; }
warn() { echo "⚠ $1"; }

# Switch to a specific model
switch_model() {
    local model=$1
    log "Switching to model: $model"

    local response=$(curl -s -X PUT "$API_URL/api/llm-backends/ollama-default" \
        -H "Authorization: Bearer $API_TOKEN" \
        -H "Content-Type: application/json" \
        -d "{\"model\": \"$model\", \"thinking_enabled\": false, \"temperature\": 0.4, \"top_p\": 0.7}")

    if echo "$response" | jq -e '.success' > /dev/null 2>&1; then
        success "Model switched to $model"
        sleep 2
        return 0
    else
        error "Failed to switch model"
        return 1
    fi
}

# Create a new session
create_session() {
    curl -s -X POST "$API_URL/api/sessions" \
        -H "Authorization: Bearer $API_TOKEN" \
        -H "Content-Type: application/json" | jq -r '.data.sessionId // .id // .session_id // ""'
}

# Send message and get response
send_message() {
    local session_id=$1
    local message=$2
    local timeout=${3:-60}

    # Use Python for milliseconds on macOS (date +%s%3N doesn't work on macOS)
    local start=$(python3 -c 'import time; print(int(time.time() * 1000))')
    local response=$(curl -s -X POST "$API_URL/api/sessions/$session_id/chat" \
        -H "Authorization: Bearer $API_TOKEN" \
        -H "Content-Type: application/json" \
        --max-time "$timeout" \
        -d "{\"message\": \"$message\"}")
    local end=$(python3 -c 'import time; print(int(time.time() * 1000))')

    local duration=$((end - start))
    echo "$response" | jq --argjson dur "$duration" '. + {processingTimeMs: $dur}' 2>/dev/null || echo "$response"
}

# Extract metrics
extract_metrics() {
    local response=$1
    local tools=$(echo "$response" | jq -r '.toolsUsed // .tools_used // [] | length' 2>/dev/null || echo "0")
    local time=$(echo "$response" | jq -r '.processingTimeMs // .processingTime // 0' 2>/dev/null || echo "0")
    local resp_text=$(echo "$response" | jq -r '.response // .message.text // ""' 2>/dev/null || echo "")

    if echo "$resp_text" | grep -qi "超时\|timeout"; then
        echo "TIMEOUT|$tools|$time"
    else
        echo "SUCCESS|$tools|$time"
    fi
}

# Test messages
declare -a TEST_NAMES=(
    "Single_tool_list_devices"
    "Single_tool_list_rules"
    "Create_rule"
    "Multi_tools_devices_rules"
    "Context_reference"
    "Complex_planning"
    "Control_device"
    "Query_data"
    "List_device_types"
    "Multi_tools_rules_types"
)

declare -a TEST_MESSAGES=(
    "请列出所有设备"
    "显示所有自动化规则"
    "创建一个规则：当温度大于30度时发送通知"
    "同时列出所有设备和所有规则"
    "刚才创建的规则ID是什么？"
    "帮我分析当前系统状态，列出所有设备、规则和告警"
    "控制开关设备switch1打开"
    "查询传感器sensor1的温度数据"
    "列出所有设备类型"
    "列出所有规则，同时列出所有设备类型"
)

# Run tests for a model
run_model_tests() {
    local model=$1
    local out="$RESULTS_DIR/${model//:/_}_results.txt"

    log "========================================="
    log "Testing Model: $model"
    log "========================================="

    switch_model "$model" || return 1

    local session_id=$(create_session)
    [ -z "$session_id" ] && { error "Failed to create session"; return 1; }
    success "Session: $session_id"

    local total=${#TEST_MESSAGES[@]}
    local passed=0
    local timeouts=0
    local total_tools=0
    local total_time=0

    echo "Model: $model | Session: $session_id" > "$out"
    echo "-----------------------------------------" >> "$out"

    for i in $(seq 0 $((total - 1))); do
        local name="${TEST_NAMES[$i]}"
        local msg="${TEST_MESSAGES[$i]}"
        local num=$((i + 1))

        log "Test $num/$total: $name"

        local resp=$(send_message "$session_id" "$msg" 90)
        local metrics=$(extract_metrics "$resp")

        IFS='|' read -r status tools time <<< "$metrics"

        # Handle empty time value
        [ -z "$time" ] && time=0

        if [ "$status" = "TIMEOUT" ] || [ "$time" -ge 90000 ]; then
            error "Test $num: TIMEOUT (${time}ms)"
            timeouts=$((timeouts + 1))
        else
            success "Test $num: PASS (tools: $tools, time: ${time}ms)"
            passed=$((passed + 1))
        fi

        total_tools=$((total_tools + tools))
        total_time=$((total_time + time))

        echo "Test $num: $name | Status: $status | Tools: $tools | Time: ${time}ms" >> "$out"

        # Save response for analysis
        echo "$resp" | jq -r '.message.text // .response // ""' >> "$out"
        echo "" >> "$out"
    done

    local avg_time=$((total_time / total))
    local rate=$((passed * 100 / total))

    log "========================================="
    log "Summary for $model"
    log "========================================="
    echo "  Passed: $passed/$total (${rate}%)"
    echo "  Timeouts: $timeouts"
    echo "  Tools Called: $total_tools"
    echo "  Avg Time: ${avg_time}ms"

    echo "$model|$passed|$total|$rate|$total_tools|$avg_time|$timeouts"
}

# Main
main() {
    log "Starting model comparison tests..."

    echo "Model|Passed|Total|Rate|Tools|AvgTime|Timeouts" > "$RESULTS_DIR/summary.csv"

    for model in "qwen2.5:3b" "qwen3:1.7b" "deepseek-r1:1.5b" "qwen3-vl:2b"; do
        echo ""
        local metrics=$(run_model_tests "$model")
        echo "$metrics" >> "$RESULTS_DIR/summary.csv"
    done

    log "========================================="
    log "COMPARISON RESULTS"
    log "========================================="
    column -t -s '|' "$RESULTS_DIR/summary.csv"

    log "Results saved to: $RESULTS_DIR"

    # Check simultaneous tool calling
    log "========================================="
    log "SIMULTANEOUS TOOL CALLING ANALYSIS"
    log "========================================="

    for f in "$RESULTS_DIR"/*_results.txt; do
        [ -f "$f" ] || continue
        local model=$(basename "$f" | sed 's/_results.txt//')
        echo -n "$model: "

        # Check Multi_tools_devices_rules test
        local tools=$(grep -A2 "Multi_tools_devices_rules" "$f" | grep "Tools:" | head -1 | sed 's/.*Tools: \([0-9]*\).*/\1/')
        echo "Multi-tools test called $tools tools"
    done
}

main "$@"

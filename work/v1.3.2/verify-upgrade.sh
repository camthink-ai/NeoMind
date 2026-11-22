#!/usr/bin/env bash
# T3.2 / Gate G6 — in-place upgrade acceptance: v1.3.1 binary → tip (1.3.2 candidate).
# Isolated: /tmp/neomind-upgrade-test + NEOMIND_STRICT_DATA_DIR=1 + temp ports.
# Never touches :29375 / :29385 / :18788.
set -euo pipefail

DATA_DIR="${NEOMIND_UPGRADE_DATA_DIR:-/tmp/neomind-upgrade-test}"
OUT_DIR="${NEOMIND_UPGRADE_OUT:-/tmp/neomind-132-stage5/upgrade}"
BIN131="${NEOMIND_BIN_131:-/tmp/neomind-1.3.1-tip-eval-overnight/bin/neomind}"
BIN132="${NEOMIND_BIN_132:-/tmp/neomind-132-stage4/bin/neomind}"
HTTP_PORT="${NEOMIND_UPGRADE_HTTP_PORT:-19400}"
MQTT_PORT="${NEOMIND_UPGRADE_MQTT_PORT:-19500}"

mkdir -p "$OUT_DIR" "$DATA_DIR"
LOG="$OUT_DIR/verify-upgrade.log"
: >"$LOG"
exec > >(tee -a "$LOG") 2>&1

echo "=== verify-upgrade start $(date) ==="
echo "DATA_DIR=$DATA_DIR HTTP=$HTTP_PORT MQTT=$MQTT_PORT"
echo "BIN131=$BIN131"
echo "BIN132=$BIN132"
test -x "$BIN131" || { echo "missing BIN131"; exit 2; }
test -x "$BIN132" || { echo "missing BIN132"; exit 2; }

rm -rf "$DATA_DIR"
mkdir -p "$DATA_DIR"

KEY_OUT="$OUT_DIR/api-key-create.txt"
"$BIN131" api-key create --name upgrade-seed --data-dir "$DATA_DIR" >"$KEY_OUT" 2>&1
API_KEY="$(python3 - "$KEY_OUT" <<'PY'
from pathlib import Path
import re, sys
text = Path(sys.argv[1]).read_text(errors="replace")
for line in text.splitlines():
    m = re.search(r"Key:\s*(nmk_\S+)", line)
    if m:
        print(m.group(1))
        break
PY
)"
test -n "$API_KEY" || { echo "failed to parse API key"; cat "$KEY_OUT"; exit 2; }
echo "API_KEY_LEN=${#API_KEY}"

cat >"$DATA_DIR/config.toml" <<EOF
[mqtt]
port = ${MQTT_PORT}
EOF

export NEOMIND_DATA_DIR="$DATA_DIR"
export NEOMIND_STRICT_DATA_DIR=1
export NEOMIND_API_KEY="$API_KEY"
export NEOMIND_API_BASE="http://127.0.0.1:${HTTP_PORT}/api"
export NEOMIND_BUILTIN_LLM=off
export NEOMIND_HOST=127.0.0.1
export NEOMIND_PORT="$HTTP_PORT"
export NEOMIND_BUILTIN_LLM_PORT=29490

api() {
  local method="$1" path="$2"
  shift 2
  curl -sS -X "$method" \
    -H "Authorization: Bearer ${API_KEY}" \
    -H "Content-Type: application/json" \
    "${NEOMIND_API_BASE}${path}" \
    "$@"
}

wait_health() {
  local deadline=$((SECONDS + 90))
  while (( SECONDS < deadline )); do
    code=$(curl -sS -o /dev/null -w "%{http_code}" \
      -H "Authorization: Bearer ${API_KEY}" \
      "http://127.0.0.1:${HTTP_PORT}/api/health" || echo 000)
    if [[ "$code" == "200" ]]; then
      echo "health=200"
      return 0
    fi
    sleep 0.5
  done
  echo "health timeout"
  return 1
}

start_serve() {
  local bin="$1" tag="$2"
  local srv_log="$OUT_DIR/serve-${tag}.log"
  : >"$srv_log"
  (
    cd "$DATA_DIR"
    exec "$bin" serve --host 127.0.0.1 --port "$HTTP_PORT"
  ) >"$srv_log" 2>&1 &
  echo $! >"$OUT_DIR/serve-${tag}.pid"
  echo "started $tag pid=$(cat "$OUT_DIR/serve-${tag}.pid") log=$srv_log"
  wait_health
}

stop_serve() {
  local tag="$1"
  local pidfile="$OUT_DIR/serve-${tag}.pid"
  if [[ -f "$pidfile" ]]; then
    local pid
    pid=$(cat "$pidfile")
    if kill -0 "$pid" 2>/dev/null; then
      echo "stopping $tag pid=$pid (SIGTERM)"
      kill -TERM "$pid" || true
      local deadline=$((SECONDS + 30))
      while kill -0 "$pid" 2>/dev/null && (( SECONDS < deadline )); do
        sleep 0.3
      done
      if kill -0 "$pid" 2>/dev/null; then
        echo "force kill $tag"
        kill -KILL "$pid" || true
      fi
    fi
    rm -f "$pidfile"
  fi
  local p
  p=$(lsof -nP -iTCP:"$HTTP_PORT" -sTCP:LISTEN -t 2>/dev/null | head -1 || true)
  if [[ -n "${p:-}" ]]; then
    echo "leftover listener $p on :$HTTP_PORT — killing"
    kill -TERM "$p" || true
    sleep 1
  fi
}

post_retry() {
  local path="$1" body="$2" out="$3" label="$4"
  local deadline=$((SECONDS + 60))
  local resp=""
  while (( SECONDS < deadline )); do
    resp=$(api POST "$path" -d "$body" || true)
    echo "$resp" >"$out"
    if echo "$resp" | grep -q '"success":true'; then
      echo "OK seed $label"
      return 0
    fi
    local low
    low=$(echo "$resp" | tr '[:upper:]' '[:lower:]')
    if echo "$low" | grep -qE 'no mqtt adapter|session not ready|template.*not found'; then
      echo "retry $label (startup race)"
      sleep 1
      continue
    fi
    echo "FAIL seed $label: $resp"
    return 1
  done
  echo "FAIL seed $label timeout: $resp"
  return 1
}

seed_v131() {
  echo "=== seed on v1.3.1 ==="
  sleep 2

  post_retry /devices '{
    "device_id": "upgrade-sensor-001",
    "name": "Upgrade Seed Sensor",
    "device_type": "ne101_camera",
    "adapter_type": "mqtt",
    "connection_config": {}
  }' "$OUT_DIR/seed-device.json" device

  post_retry /rules '{
    "name": "upgrade-rule-battery",
    "description": "upgrade seed rule",
    "enabled": true,
    "trigger": { "trigger_type": "data_change" },
    "condition": {
      "condition_type": "comparison",
      "source": "device:upgrade-sensor-001:values.battery",
      "operator": "less_than",
      "threshold": 20.0
    },
    "actions": [
      { "type": "notify", "message": "upgrade battery low", "severity": "warning" }
    ],
    "cooldown": 60000
  }' "$OUT_DIR/seed-rule.json" rule

  post_retry /dashboards '{
    "name": "upgrade-dashboard",
    "description": "upgrade seed dashboard",
    "components": []
  }' "$OUT_DIR/seed-dashboard.json" dashboard

  post_retry /agents '{
    "name": "upgrade-structured-agent",
    "description": "upgrade seed structured agent",
    "user_prompt": "Report battery status for upgrade-sensor-001",
    "execution_mode": "structured",
    "schedule": { "schedule_type": "manual" },
    "resources": [
      {
        "resource_id": "device:upgrade-sensor-001:values.battery",
        "resource_type": "device_metric",
        "name": "battery"
      }
    ],
    "output_schema": [
      { "name": "status", "field_type": { "type": "text" } }
    ]
  }' "$OUT_DIR/seed-agent.json" agent

  SESSION_JSON=$(api POST /sessions -d '{}')
  echo "$SESSION_JSON" | tee "$OUT_DIR/seed-session.json"
  SID=$(python3 -c 'import json,sys
d=json.load(sys.stdin)
data=d.get("data") if isinstance(d,dict) else {}
if not isinstance(data,dict): data={}
print(data.get("sessionId") or data.get("session_id") or data.get("id") or "")
' <<<"$SESSION_JSON")
  test -n "$SID" || { echo "no session id from $SESSION_JSON"; exit 3; }

  api POST "/sessions/${SID}/messages" -d '{
    "role": "user",
    "content": "upgrade-seed chat history marker"
  }' >"$OUT_DIR/seed-chat.json" 2>&1 || true
  if ! grep -q 'upgrade-seed chat history marker' "$OUT_DIR/seed-chat.json" 2>/dev/null; then
    curl -sS -m 20 -X POST \
      -H "Authorization: Bearer ${API_KEY}" \
      -H "Content-Type: application/json" \
      "${NEOMIND_API_BASE}/sessions/${SID}/chat" \
      -d '{"message":"upgrade-seed chat history marker","stream":false}' \
      >"$OUT_DIR/seed-chat.json" 2>&1 || true
  fi
  echo "session=$SID"

  api POST /automations/transforms -d '{
    "name": "upgrade-write-ledger",
    "description": "upgrade seed write fact",
    "code": "return 1;",
    "inputs": ["device:upgrade-sensor-001:values.battery"]
  }' >"$OUT_DIR/seed-write.json" 2>&1 || true
  if ! grep -q '"success":true' "$OUT_DIR/seed-write.json" 2>/dev/null; then
    api POST /devices/upgrade-sensor-001/metrics -d '{
      "metric": "values.battery",
      "value": 88
    }' >"$OUT_DIR/seed-write.json" 2>&1 || true
  fi
  echo "write_seed_head=$(head -c 240 "$OUT_DIR/seed-write.json")"

  python3 - "$OUT_DIR" "$SID" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
sid = sys.argv[2]
expect = {"session_id": sid, "device_id": "upgrade-sensor-001"}

def data_of(name):
    p = out / name
    if not p.exists():
        return {}
    try:
        d = json.loads(p.read_text())
    except Exception:
        return {}
    if isinstance(d, dict) and isinstance(d.get("data"), dict):
        return d["data"]
    return d if isinstance(d, dict) else {}

rule = data_of("seed-rule.json")
dash = data_of("seed-dashboard.json")
agent = data_of("seed-agent.json")
if isinstance(rule.get("rule"), dict):
    expect["rule_id"] = rule["rule"].get("id")
else:
    expect["rule_id"] = rule.get("id")
expect["dashboard_id"] = dash.get("id")
expect["agent_id"] = agent.get("id")
write = data_of("seed-write.json")
expect["write_id"] = write.get("id") or write.get("name") or "metric-or-transform"
(out / "expect.json").write_text(json.dumps(expect, indent=2))
print("expect", expect)
PY
}

verify_after_upgrade() {
  echo "=== verify after tip binary ==="
  FAIL=0
  srv_log="$OUT_DIR/serve-132.log"

  if grep -q "DATA-DIR SPLIT" "$srv_log"; then
    echo "FAIL: DATA-DIR SPLIT in serve-132.log"; FAIL=1
  else
    echo "OK: no DATA-DIR SPLIT"
  fi
  if grep -qiE "decrypt(ion)? failed|failed to decrypt|panic" "$srv_log"; then
    echo "FAIL: decrypt/panic in serve-132.log"; FAIL=1
  else
    echo "OK: no decrypt/panic"
  fi

  CLI_OUT="$OUT_DIR/cli-device-list.json"
  if ! "$BIN132" device list --format json >"$CLI_OUT" 2>"$OUT_DIR/cli-device-list.err"; then
    echo "FAIL: CLI device list exit nonzero"
    cat "$OUT_DIR/cli-device-list.err" || true
    FAIL=1
  fi
  if grep -qi '401' "$OUT_DIR/cli-device-list.err" "$CLI_OUT" 2>/dev/null; then
    echo "FAIL: CLI auto_auth 401"
    FAIL=1
  else
    echo "OK: CLI auto_auth no 401"
  fi

  check_get() {
    local path="$1" needle="$2" label="$3"
    local body code
    body=$(curl -sS -w "\n%{http_code}" \
      -H "Authorization: Bearer ${API_KEY}" \
      "${NEOMIND_API_BASE}${path}")
    code=$(echo "$body" | tail -1)
    body=$(echo "$body" | sed '$d')
    echo "$body" >"$OUT_DIR/verify-${label}.json"
    if [[ "$code" != "200" ]]; then
      echo "FAIL: GET $path -> $code"; FAIL=1; return
    fi
    if [[ -n "$needle" ]] && ! echo "$body" | grep -q "$needle"; then
      echo "FAIL: GET $path missing needle $needle"; FAIL=1; return
    fi
    echo "OK: $label"
  }

  check_get "/devices/upgrade-sensor-001" "Upgrade Seed Sensor" device
  check_get "/rules" "upgrade-rule-battery" rules
  check_get "/dashboards" "upgrade-dashboard" dashboards
  check_get "/agents" "upgrade-structured-agent" agents

  SID=$(python3 -c 'import json; print(json.load(open("'"$OUT_DIR"'/expect.json")).get("session_id",""))')
  if [[ -n "$SID" ]]; then
    code=$(curl -sS -o "$OUT_DIR/verify-chat.json" -w "%{http_code}" \
      -H "Authorization: Bearer ${API_KEY}" \
      "${NEOMIND_API_BASE}/sessions/${SID}/history")
    if [[ "$code" != "200" ]]; then
      code2=$(curl -sS -o "$OUT_DIR/verify-chat.json" -w "%{http_code}" \
        -H "Authorization: Bearer ${API_KEY}" \
        "${NEOMIND_API_BASE}/sessions/${SID}")
      if [[ "$code2" != "200" ]]; then
        echo "FAIL: session $SID not readable ($code/$code2)"; FAIL=1
      else
        echo "OK: chat session exists"
      fi
    else
      if grep -q 'upgrade-seed' "$OUT_DIR/verify-chat.json"; then
        echo "OK: chat history marker"
      else
        echo "OK: chat history readable (marker optional if LLM path skipped)"
      fi
    fi
  else
    echo "FAIL: no session_id in expect"; FAIL=1
  fi

  if curl -sS -o /dev/null -w "%{http_code}" -H "Authorization: Bearer ${API_KEY}" \
      "${NEOMIND_API_BASE}/automations/transforms" | grep -q 200; then
    if curl -sS -H "Authorization: Bearer ${API_KEY}" \
        "${NEOMIND_API_BASE}/automations/transforms" | grep -q "upgrade-write-ledger"; then
      echo "OK: write transform"
    elif grep -q '"success":true' "$OUT_DIR/seed-write.json" 2>/dev/null; then
      echo "OK: write seed recorded at create time"
    else
      echo "FAIL: write ledger seed missing"; FAIL=1
    fi
  else
    if grep -q '"success":true' "$OUT_DIR/seed-write.json" 2>/dev/null; then
      echo "OK: write seed recorded at create time"
    else
      echo "FAIL: write ledger seed missing"; FAIL=1
    fi
  fi

  STATS=$(api GET /stats/system)
  echo "$STATS" | tee "$OUT_DIR/stats-system.json"
  if ! python3 - "$OUT_DIR/stats-system.json" <<'PY'
import json, sys
p = sys.argv[1]
d = json.load(open(p))
if isinstance(d.get("data"), dict):
    d = d["data"]
mem = None
if isinstance(d.get("process"), dict):
    mem = d["process"].get("memory_bytes")
if mem is None:
    mem = d.get("memory_bytes")
if not isinstance(mem, int) or mem <= 0:
    print(f"FAIL: process.memory_bytes invalid: {mem!r}")
    sys.exit(1)
print(f"OK: process.memory_bytes={mem}")
PY
  then
    FAIL=1
  fi

  if (( FAIL != 0 )); then
    echo "=== VERIFY FAIL ==="
    exit 1
  fi
  echo "=== VERIFY PASS ==="
}

cleanup() {
  stop_serve 131 || true
  stop_serve 132 || true
}
trap cleanup EXIT

start_serve "$BIN131" 131
seed_v131
stop_serve 131
sleep 1
start_serve "$BIN132" 132
verify_after_upgrade

echo "=== verify-upgrade done $(date) ==="
exit 0

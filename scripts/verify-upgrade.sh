#!/usr/bin/env bash
# T3.2 / Gate G6 — in-place upgrade acceptance: v1.3.1 binary → tip (1.3.2 RC).
#
# Lives under scripts/ (tracked) rather than gitignored work/ so Gate G6 cannot
# silently drift. Pass binaries via env — no hard-coded stage artifact paths.
#
# Required:
#   NEOMIND_BIN_131  path to a 1.3.1 neomind binary
#   NEOMIND_BIN_132  path to the 1.3.2 RC neomind binary under test
#
# Optional:
#   NEOMIND_UPGRADE_DATA_DIR  default /tmp/neomind-upgrade-test
#   NEOMIND_UPGRADE_OUT       default /tmp/neomind-upgrade-verify
#   NEOMIND_UPGRADE_HTTP_PORT default 19400
#   NEOMIND_UPGRADE_MQTT_PORT default 19500
#   NEOMIND_EXPECT_VERSION    default 1.3.2
#
# Isolated: STRICT_DATA_DIR + private ports. Never touches :29375 / :29385 / :18788.
set -euo pipefail

DATA_DIR="${NEOMIND_UPGRADE_DATA_DIR:-/tmp/neomind-upgrade-test}"
OUT_DIR="${NEOMIND_UPGRADE_OUT:-/tmp/neomind-upgrade-verify}"
BIN131="${NEOMIND_BIN_131:-}"
BIN132="${NEOMIND_BIN_132:-}"
HTTP_PORT="${NEOMIND_UPGRADE_HTTP_PORT:-19400}"
MQTT_PORT="${NEOMIND_UPGRADE_MQTT_PORT:-19500}"
EXPECT_VERSION="${NEOMIND_EXPECT_VERSION:-1.3.2}"

mkdir -p "$OUT_DIR" "$DATA_DIR"
LOG="$OUT_DIR/verify-upgrade.log"
: >"$LOG"
exec > >(tee -a "$LOG") 2>&1

echo "=== verify-upgrade start $(date) ==="
echo "DATA_DIR=$DATA_DIR HTTP=$HTTP_PORT MQTT=$MQTT_PORT EXPECT_VERSION=$EXPECT_VERSION"
echo "BIN131=$BIN131"
echo "BIN132=$BIN132"

if [[ -z "$BIN131" || ! -x "$BIN131" ]]; then
  echo "missing executable NEOMIND_BIN_131 (1.3.1 binary)"; exit 2
fi
if [[ -z "$BIN132" || ! -x "$BIN132" ]]; then
  echo "missing executable NEOMIND_BIN_132 (1.3.2 RC binary)"; exit 2
fi

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
# Keep API_KEY for curl helpers only. CLI auto_auth checks must unset it.
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

  # Prefer a durable metric write (seed-6). Transform create is best-effort.
  api POST /devices/upgrade-sensor-001/metrics -d '{
    "metric": "values.battery",
    "value": 88
  }' >"$OUT_DIR/seed-write.json" 2>&1 || true
  if ! grep -q '"success":true' "$OUT_DIR/seed-write.json" 2>/dev/null; then
    api POST /automations/transforms -d '{
      "name": "upgrade-write-ledger",
      "description": "upgrade seed write fact",
      "code": "return 1;",
      "inputs": ["device:upgrade-sensor-001:values.battery"]
    }' >"$OUT_DIR/seed-write.json" 2>&1 || true
  fi
  echo "write_seed_head=$(head -c 240 "$OUT_DIR/seed-write.json")"

  python3 - "$OUT_DIR" "$SID" <<'PY'
import json, sys
from pathlib import Path
out = Path(sys.argv[1])
sid = sys.argv[2]
expect = {"session_id": sid, "device_id": "upgrade-sensor-001", "battery": 88}

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

  # Real auto_auth: CLI must NOT see NEOMIND_API_KEY (else it shadows redb/file auth).
  CLI_OUT="$OUT_DIR/cli-device-list.json"
  if ! env -u NEOMIND_API_KEY \
      NEOMIND_API_BASE="$NEOMIND_API_BASE" \
      NEOMIND_DATA_DIR="$DATA_DIR" \
      NEOMIND_STRICT_DATA_DIR=1 \
      "$BIN132" device list --format json \
      >"$CLI_OUT" 2>"$OUT_DIR/cli-device-list.err"; then
    echo "FAIL: CLI device list exit nonzero (auto_auth path)"
    cat "$OUT_DIR/cli-device-list.err" || true
    FAIL=1
  fi
  if grep -qi '401' "$OUT_DIR/cli-device-list.err" "$CLI_OUT" 2>/dev/null; then
    echo "FAIL: CLI auto_auth 401"
    FAIL=1
  else
    echo "OK: CLI auto_auth no 401 (NEOMIND_API_KEY unset)"
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

  # Seed-6 post-upgrade re-read: device current_values / telemetry / transform.
  WRITE_OK=0
  if python3 - "$OUT_DIR/verify-device.json" <<'PY'
import json, sys
p = sys.argv[1]
d = json.load(open(p))
if isinstance(d.get("data"), dict):
    d = d["data"]
cv = d.get("current_values") or {}
val = cv.get("values.battery")
try:
    if float(val) == 88.0:
        print(f"OK: seed-6 battery re-read current_values={val}")
        sys.exit(0)
except (TypeError, ValueError):
    pass
print(f"device current_values missing battery==88: {cv!r}")
sys.exit(1)
PY
  then
    WRITE_OK=1
  fi

  if (( WRITE_OK == 0 )); then
    TELE=$(api GET "/devices/upgrade-sensor-001/telemetry?metric=values.battery&hours=720&limit=5" || true)
    echo "$TELE" >"$OUT_DIR/verify-seed6-telemetry.json"
    if python3 - "$OUT_DIR/verify-seed6-telemetry.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
if isinstance(d.get("data"), dict):
    d = d["data"]
points = d.get("points") or d.get("data") or d.get("series") or []
if isinstance(d.get("metrics"), dict):
    points = d["metrics"].get("values.battery") or points
cands = []
if isinstance(points, list):
    for pt in points:
        if isinstance(pt, dict):
            cands.append(pt.get("value"))
        elif isinstance(pt, (int, float)):
            cands.append(pt)
elif isinstance(points, dict):
    cands.append(points.get("value"))
for c in cands:
    try:
        if float(c) == 88.0:
            print(f"OK: seed-6 battery re-read telemetry value={c}")
            sys.exit(0)
    except (TypeError, ValueError):
        pass
print("telemetry missing battery==88")
sys.exit(1)
PY
    then
      WRITE_OK=1
    fi
  fi

  if (( WRITE_OK == 0 )); then
    TX=$(api GET /automations/transforms || true)
    echo "$TX" >"$OUT_DIR/verify-seed6-transforms.json"
    if echo "$TX" | grep -q "upgrade-write-ledger"; then
      echo "OK: seed-6 transform re-read"
      WRITE_OK=1
    fi
  fi

  if (( WRITE_OK == 0 )); then
    echo "FAIL: seed-6 write not re-readable after upgrade (device/telemetry/transform)"
    FAIL=1
  fi

  STATS=$(api GET /stats/system)
  echo "$STATS" | tee "$OUT_DIR/stats-system.json"
  if ! python3 - "$OUT_DIR/stats-system.json" "$EXPECT_VERSION" <<'PY'
import json, sys
p, expect = sys.argv[1], sys.argv[2]
d = json.load(open(p))
if isinstance(d.get("data"), dict):
    d = d["data"]
ver = d.get("version") or (d.get("process") or {}).get("version")
if ver != expect:
    print(f"FAIL: version={ver!r} expected {expect!r}")
    sys.exit(1)
print(f"OK: version={ver}")
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

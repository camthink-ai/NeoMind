# NeoMind System-Level Test Layer (`eval/system/`)

Drives **real server paths** the agent eval (`eval/`) doesn't: a real MQTT
device → telemetry store → live dashboard WS event. Uses `TestServer` from
`eval/lib/server.py` to spawn a real `neomind serve` in a temp data dir, so
these are end-to-end against the actual embedded broker, storage, and EventBus.

The agent eval measures *agent competence via chat*; this layer measures the
*system's body* — the data spine. See CLAUDE.md / project memory for the gap
analysis (behavior-tree coverage).

## Run

```bash
pip install -r eval/requirements.txt   # adds paho-mqtt
cargo build --release -p neomind-cli   # server.py warns on stale binaries
python3 eval/system/run_system.py list
python3 eval/system/run_system.py run-scenario \
    --scenario eval/system/scenarios/mqtt-telemetry-flow.json
python3 eval/system/run_system.py run-all
```

Exit code 0 = all assertions passed; nonzero otherwise. Each scenario prints
a `ScenarioRecord` with per-assert `{type, expected, actual, passed}`.

## Adding a scenario ("持续补充")

1. Drop a JSON file in `eval/system/scenarios/`. **No runner change needed.**
2. Schema (see `mqtt-telemetry-flow.json` for a complete example):

```json
{
  "id": "unique-kebab-id",
  "description": "...",
  "setup": { "extras": {
    "device_types": [{"device_type":"env_sensor","name":"Env Sensor","mode":"Simple",
      "metrics":[{"name":"temperature","data_type":"Float"}]}],
    "devices": [{"device_type":"env_sensor","device_id":"mqtt-temp-001","name":"...",
      "adapter_type":"mqtt",
      "connection_config":{"telemetry_topic":"device/env_sensor/mqtt-temp-001/uplink"}}]
  }},
  "action": { "type": "mqtt_publish", "device_id":"mqtt-temp-001",
              "device_type":"env_sensor", "payload": {"temperature": 25.5} },
  "asserts": [ { "type": "latest_telemetry",
                 "params": {"device_id":"mqtt-temp-001","metric":"temperature"},
                 "expected": 25.5 } ],
  "ws_asserts": [ { "type": "DeviceMetric",
                    "match": {"data.metric":"temperature","data.value":25.5},
                    "timeout": 10 } ]
}
```

- `asserts[]` reuse `eval/lib/state_query.py` query types (any of them, e.g.
  `rule_exists`, `message_count`, `dashboard_component_bound`, `latest_telemetry`).
- `ws_asserts[]` wait for a live WS event matching `{type, match(dotted-path)}`.

## Constraints / gotchas

- **Port 1883 is fixed** (embedded broker). Scenarios run **strictly
  sequentially** — one server/broker at a time. `run-all` shuts down fully
  between scenarios. No parallel execution.
- **Broker lags `/health`**: `MqttDeviceSimulator.connect()` blocks on the
  MQTT CONNACK (retrying the socket connect) — it is the readiness gate.
- **WS `DeviceMetric` is batched** (not in `immediate_events`): the subscriber
  handles both single `{...}` and `{"batch":true,"events":[...]}` frames.
- **WS heartbeat**: the subscriber auto-replies `pong` to the 30s server ping.
- **Float fidelity**: values are stored/returned verbatim, so exact equality
  on e.g. `25.5` is safe. Avoid near-equality assertions.

## Future scenario ideas

- `dashboard-live-binding` — widget bound to `device:{id}:{field}`, assert live
  update via the same WS (this MVP already proves the spine).
- `mqtt-downlink-command` — device subscribes via `subscribe_for_commands`,
  a rule/agent sends a command, assert delivery.
- `offline-detection` — device connects then disconnects, assert
  `DeviceTransportOffline` / the 4-state UI model.
- `rule-on-real-mqtt` — real MQTT telemetry fires a rule (bridges the agent-eval
  runtime cases to the real MQTT path).

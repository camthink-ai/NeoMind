#!/usr/bin/env python3
"""End-to-end business-chain smoke: the path a real deployment lives on.

The case-based eval exercises the *agent* over HTTP. This exercises the
*platform* with no LLM in the loop, through the same services a production
box runs:

    MQTT publish -> MQTT adapter -> metric stored -> rule engine
                 -> notification (all enabled channels, incl. webhook)
                 -> data-push forwarding (webhook target)

Every step is asserted against real state (HTTP API / a live webhook
receiver), so a green run means "a device's data reaches an operator", which
no number of agent-gate passes can establish.

Usage:
    python3 eval/e2e_business.py                 # all steps
    python3 eval/e2e_business.py --keep-server   # leave the server up for poking
    python3 eval/e2e_business.py -v              # echo server logs

Requires: `cargo build -p neomind-cli --release` and `paho-mqtt` in the venv.
Exit code 0 = every step passed.
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import requests

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib.server import TestServer  # noqa: E402

DEVICE_ID = "e2e-smoke-001"
DEVICE_TYPE = "e2e_smoke_sensor"
# Standard uplink shape: device/{device_type}/{device_id}/uplink. The MQTT
# handler only takes the JSON-payload extraction path for this shape; any
# other topic falls back to "last topic segment is the metric name", which
# silently drops JSON payloads.
TELEMETRY_TOPIC = f"device/{DEVICE_TYPE}/{DEVICE_ID}/uplink"
METRIC = "temperature"
ALERT_THRESHOLD = 30.0


# --------------------------------------------------------------------------
# webhook receiver
# --------------------------------------------------------------------------
class WebhookCollector:
    """HTTP server that records every POST — stands in for an operator's
    alert endpoint / external SIEM / data-push consumer."""

    def __init__(self) -> None:
        self.events: list[dict] = []
        self.lock = threading.Lock()
        collector = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self) -> None:  # noqa: N802 (http.server API)
                length = int(self.headers.get("content-length") or 0)
                raw = self.rfile.read(length) if length else b""
                try:
                    payload = json.loads(raw) if raw else {}
                except json.JSONDecodeError:
                    payload = {"_raw": raw.decode("utf-8", "replace")}
                with collector.lock:
                    collector.events.append(
                        {"path": self.path, "payload": payload, "at": time.time()}
                    )
                self.send_response(200)
                self.send_header("content-type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"ok":true}')

            def log_message(self, *_args) -> None:
                pass

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.httpd.server_address[1]
        self.url = f"http://127.0.0.1:{self.port}"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def path_url(self, path: str) -> str:
        return f"{self.url}{path}"

    def wait_for(self, path_prefix: str, needle: str, timeout: float) -> dict | None:
        """Wait for a POST to `path_prefix` whose JSON text contains `needle`."""
        deadline = time.time() + timeout
        while time.time() < deadline:
            with self.lock:
                for ev in self.events:
                    if not ev["path"].startswith(path_prefix):
                        continue
                    if needle.lower() in json.dumps(ev["payload"], ensure_ascii=False).lower():
                        return ev
            time.sleep(0.2)
        return None

    def close(self) -> None:
        self.httpd.shutdown()


# --------------------------------------------------------------------------
# MQTT publisher
# --------------------------------------------------------------------------
def publish_mqtt(port: int, topic: str, value: float, wait: float = 5.0) -> None:
    import paho.mqtt.client as mqtt

    client = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2)
    deadline = time.time() + wait
    last_err: Exception | None = None
    while time.time() < deadline:
        try:
            client.connect("127.0.0.1", port, keepalive=15)
            break
        except Exception as exc:  # broker may still be binding
            last_err = exc
            time.sleep(0.3)
    else:
        raise RuntimeError(f"MQTT broker never accepted a connection on :{port} ({last_err})")
    client.loop_start()
    info = client.publish(topic, json.dumps({METRIC: value}), qos=1)
    info.wait_for_publish(timeout=5)
    client.loop_stop()
    client.disconnect()


def wait_for_internal_adapter(srv, timeout: float = 90.0) -> None:
    """Block until the platform's own MQTT client is subscribed to its broker.

    Measured 2026-10-26 (1.2.3, cold start): the embedded broker is up within
    ~300ms of /health, but `internal-mqtt` connects ~16s later (it is created
    in a post-health background task). Anything published before that is
    silently dropped — there is no subscriber. /mqtt/status exposes the truth
    (`connected`, `subscriptions_count`), so wait on it instead of guessing.
    """
    deadline = time.time() + timeout
    last = {}
    while time.time() < deadline:
        r = srv.get("/mqtt/status")
        if r.ok:
            last = (r.json().get("data") or {}).get("status") or {}
            if last.get("connected") and (last.get("subscriptions_count") or 0) > 0:
                return
        time.sleep(1.0)
    raise RuntimeError(
        f"internal MQTT adapter never subscribed within {timeout}s "
        f"(last status: {json.dumps(last, ensure_ascii=False)[:300]})"
    )


# --------------------------------------------------------------------------
# step runner
# --------------------------------------------------------------------------
class Runner:
    def __init__(self) -> None:
        self.results: list[tuple[str, bool, str, float]] = []

    def step(self, name: str, fn) -> bool:
        started = time.monotonic()
        try:
            fn()
        except Exception as exc:  # noqa: BLE001 - a failed step reports, never aborts
            self.results.append((name, False, str(exc), time.monotonic() - started))
            print(f"  FAIL  {name}: {exc}", file=sys.stderr)
            return False
        self.results.append((name, True, "", time.monotonic() - started))
        print(f"  PASS  {name}  ({time.monotonic() - started:.1f}s)")
        return True

    def summary(self) -> int:
        passed = sum(1 for r in self.results if r[1])
        total = len(self.results)
        print(f"\n=== business chain: {passed}/{total} steps passed ===")
        for name, ok, err, dur in self.results:
            print(f"  {'PASS' if ok else 'FAIL':4s}  {name:52s} {dur:6.1f}s {err}")
        return 0 if passed == total else 1


def expect(cond: bool, msg: str) -> None:
    if not cond:
        raise AssertionError(msg)


def poll(fn, predicate, timeout: float, interval: float = 0.5):
    """Return fn()'s first result satisfying predicate, else None."""
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        last = fn()
        if predicate(last):
            return last
        time.sleep(interval)
    return last


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--keep-server", action="store_true")
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--timeout", type=float, default=20.0, help="per-assertion wait (s)")
    args = ap.parse_args()

    if not args.verbose:
        # TestServer echoes child output to stderr; silence unless asked.
        import lib.server as server_mod

        server_mod.sys_stderr_write = lambda *_a, **_k: None  # type: ignore[assignment]

    mqtt_port = _free_port()
    os.environ["NEOMIND_EVAL_MQTT_PORT"] = str(mqtt_port)

    collector = WebhookCollector()
    runner = Runner()
    srv = TestServer()
    print(f"spawning neomind serve (api :{srv.port or 'auto'}, mqtt :{mqtt_port}) ...")
    srv.spawn(startup_timeout=45)
    print(f"  server up at {srv.api_base}; webhook receiver at {collector.url}")

    def api_get(path: str) -> requests.Response:
        return srv.get(path)

    def api_post(path: str, body) -> requests.Response:
        return srv.post(path, body)

    def ok_post(path: str, body, what: str) -> dict:
        r = api_post(path, body)
        expect(r.ok, f"{what} -> {r.status_code}: {r.text[:300]}")
        data = r.json()
        return data.get("data", data)

    try:
        # 1. device type + device (MQTT, explicit telemetry topic)
        def seed_device_type():
            ok_post(
                "/device-types",
                {
                    "device_type": DEVICE_TYPE,
                    "name": "E2E Smoke Sensor",
                    "metrics": [{"name": METRIC, "data_type": "Float"}],
                },
                "create device type",
            )

        def seed_device():
            ok_post(
                "/devices",
                {
                    "device_id": DEVICE_ID,
                    "name": "E2E Smoke Sensor",
                    "device_type": DEVICE_TYPE,
                    "adapter_type": "mqtt",
                    "connection_config": {"telemetry_topic": TELEMETRY_TOPIC},
                },
                "create device",
            )

        # 2. notification channel -> our receiver
        def seed_channel():
            ok_post(
                "/messages/channels",
                {
                    "name": "e2e-webhook",
                    "channel_type": "webhook",
                    "url": collector.path_url("/alert"),
                },
                "create webhook channel",
            )

        # 3. rule: metric above threshold -> notify (broadcasts to enabled channels)
        def seed_rule():
            ok_post(
                "/rules",
                {
                    "name": "e2e-high-temp",
                    "description": f"{DEVICE_ID} {METRIC} > {ALERT_THRESHOLD}",
                    "enabled": True,
                    "trigger": {"trigger_type": "data_change"},
                    "condition": {
                        "condition_type": "comparison",
                        "source": f"device:{DEVICE_ID}:{METRIC}",
                        "operator": "greater_than",
                        "threshold": ALERT_THRESHOLD,
                    },
                    "actions": [
                        {
                            "type": "notify",
                            "message": f"{DEVICE_ID} hot: {{value}}",
                            "severity": "critical",
                        }
                    ],
                    "cooldown": 1000,
                },
                "create rule",
            )

        # 4. data-push target -> our receiver (event-driven, this device only)
        push_id: dict[str, str] = {}

        def seed_push():
            data = ok_post(
                "/data-push",
                {
                    "name": "e2e-push",
                    "target_type": "webhook",
                    "config": {"url": collector.path_url("/push")},
                    "schedule": {"type": "event_driven", "event_types": ["device_metric"]},
                    "data_filter": {"source_patterns": [f"device:{DEVICE_ID}:*"]},
                    "enabled": True,
                },
                "create data-push target",
            )
            push_id["id"] = data.get("id") or data.get("target", {}).get("id", "")

        def start_push():
            expect(bool(push_id.get("id")), "data-push target id missing from create response")
            r = api_post(f"/data-push/{push_id['id']}/start", {})
            expect(r.ok, f"start push -> {r.status_code}: {r.text[:200]}")

        # 5. the chain itself
        def adapter_ready():
            wait_for_internal_adapter(srv)

        def publish_and_assert_metric():
            """Publish until the reading is queryable.

            Even after /mqtt/status says connected, the broker subscription can
            take another beat to propagate, so the first datagram may be lost.
            Retrying the publish is honest here: the assertion being made is
            "a published reading becomes queryable", not "the very first
            datagram survives".
            """
            target = 35.5
            deadline = time.time() + args.timeout

            def stored_values():
                r = api_get(f"/devices/{DEVICE_ID}/telemetry?metric={METRIC}&hours=1")
                if not r.ok:
                    return []
                data = (r.json().get("data") or {}).get("data") or {}
                return data.get(METRIC) or []

            while time.time() < deadline:
                publish_mqtt(mqtt_port, TELEMETRY_TOPIC, target)
                got = poll(stored_values, lambda pts: len(pts) > 0, timeout=4)
                if got:
                    values = [p.get("value") for p in got]
                    expect(any(abs(float(v) - target) < 1e-6 for v in values if v is not None),
                           f"stored point(s) {values} do not match published {target}")
                    return
            raise AssertionError(
                f"published reading never became queryable within {args.timeout}s")

        def assert_rule_fired():
            def read():
                r = api_get("/messages")
                return r.json() if r.ok else {}

            got = poll(read, lambda v: DEVICE_ID in json.dumps(v), args.timeout)
            expect(DEVICE_ID in json.dumps(got or {}),
                   f"no rule-triggered message within {args.timeout}s")

        def assert_alert_webhook():
            ev = collector.wait_for("/alert", DEVICE_ID, args.timeout)
            expect(ev is not None, f"alert webhook not delivered within {args.timeout}s")

        def publish_and_assert_push():
            publish_mqtt(mqtt_port, TELEMETRY_TOPIC, 41.0)
            ev = collector.wait_for("/push", METRIC, args.timeout)
            expect(ev is not None, f"data-push webhook not delivered within {args.timeout}s")
            text = json.dumps(ev["payload"], ensure_ascii=False)
            expect("41" in text or DEVICE_ID in text,
                   f"push payload lacks the published reading: {text[:300]}")

        def wait_ingestion_ready(timeout: float = 45.0):
            """Webhook ingestion returns 503 for the first seconds after spawn
            (a startup race — sibling of the MQTT 16s one, but at least loud).
            Poll with a throwaway device payload until it accepts."""
            deadline = time.time() + timeout
            while time.time() < deadline:
                r = api_post("/devices/wh-001/webhook", {"probe": 0})
                if r.status_code != 503:
                    return
                time.sleep(1.0)
            raise AssertionError(f"webhook ingestion never became ready within {timeout}s")

        def seed_webhook_device():
            ok_post("/device-types", {
                "device_type": "e2e_wh_sensor", "name": "E2E Webhook Sensor",
                "metrics": [{"name": "temperature", "data_type": "Float"}]},
                "create webhook device type")
            ok_post("/devices", {
                "device_id": "wh-001", "name": "E2E Webhook Sensor",
                "device_type": "e2e_wh_sensor", "adapter_type": "webhook",
                "connection_config": {}}, "create webhook device")

        def webhook_uplink_stored():
            wait_ingestion_ready()
            r = api_post("/devices/wh-001/webhook", {"temperature": 23.5})
            expect(r.ok, f"webhook uplink -> {r.status_code}: {r.text[:200]}")

            def read():
                rr = api_get("/devices/wh-001/telemetry?metric=temperature&hours=1")
                return rr.json() if rr.ok else {}

            def has_value(v):
                data = (v.get("data") or {}).get("data") or {}
                pts = data.get("temperature") or []
                return any(abs(float(p.get("value", 0)) - 23.5) < 1e-6 for p in pts)

            got = poll(read, has_value, args.timeout)
            expect(got is not None and has_value(got),
                   "webhook-published reading never appeared in telemetry")

        def dead_push_retries_and_logs():
            data = ok_post("/data-push", {
                "name": "e2e-dead-push", "target_type": "webhook",
                "config": {"url": "http://127.0.0.1:9/dead"},
                "schedule": {"type": "event_driven", "event_types": ["device_metric"]},
                "data_filter": {"source_patterns": ["device:wh-001:*"]},
                "enabled": True,
                # 1s backoff keeps the retry ladder observable but quick
                "retry_config": {"max_retries": 2, "backoff_secs": 1, "max_backoff_secs": 5},
            }, "create dead push target")
            dead_id = data.get("id")
            expect(bool(dead_id), "dead push id missing")
            r = api_post(f"/data-push/{dead_id}/start", {})
            expect(r.ok, f"start dead push -> {r.status_code}")
            api_post("/devices/wh-001/webhook", {"temperature": 41.0})

            def read_logs():
                rr = api_get(f"/data-push/{dead_id}/logs?limit=10")
                return (rr.json().get("data") or {}).get("logs") or [] if rr.ok else []

            logs = poll(read_logs, lambda ls: len(ls) > 0, args.timeout)
            expect(logs, f"dead push never logged a delivery attempt within {args.timeout}s")
            entry = logs[0]
            status = str(entry.get("status", "")).lower()
            expect(status in ("retrying", "failed", "error"),
                   f"dead push log status unexpected: {entry}")
            expect(int(entry.get("attempts") or 0) >= 1,
                   f"no retry attempts recorded: {entry}")

        def assert_push_logs():
            r = api_get(f"/data-push/{push_id['id']}/logs?limit=20")
            expect(r.ok, f"push logs -> {r.status_code}")
            expect("success" in r.text.lower() or "200" in r.text,
                   f"push logs show no successful delivery: {r.text[:300]}")

        runner.step("device type registered", seed_device_type)
        runner.step("device registered on MQTT adapter", seed_device)
        runner.step("webhook notification channel created", seed_channel)
        runner.step("threshold rule created", seed_rule)
        runner.step("data-push webhook target created", seed_push)
        runner.step("data-push target started", start_push)
        runner.step("internal MQTT adapter subscribed to broker", adapter_ready)
        runner.step("MQTT publish -> metric stored (GET telemetry)", publish_and_assert_metric)
        runner.step("rule engine fired -> message recorded", assert_rule_fired)
        runner.step("notification delivered to webhook", assert_alert_webhook)
        runner.step("MQTT publish -> data-push webhook delivered", publish_and_assert_push)
        runner.step("data-push delivery log recorded", assert_push_logs)
        runner.step("webhook device type + device registered", seed_webhook_device)
        runner.step("webhook uplink -> metric stored", webhook_uplink_stored)
        runner.step("dead push target retries + logs failure", dead_push_retries_and_logs)
    finally:
        collector.close()
        if args.keep_server:
            print(f"\n--keep-server: leaving {srv.api_base} up (pid {srv.process.pid})",
                  file=sys.stderr)
        else:
            srv.shutdown()

    return runner.summary()


def _free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


if __name__ == "__main__":
    sys.exit(main())

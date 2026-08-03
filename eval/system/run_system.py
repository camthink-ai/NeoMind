#!/usr/bin/env python3
"""NeoMind system-level test runner.

Drives real server paths the agent eval doesn't: a real MQTT device →
telemetry store → live dashboard WS event. Scenarios are JSON files in
``eval/system/scenarios/`` — to add one, drop in a JSON file, no runner change
("持续补充").

Usage:
    python3 eval/system/run_system.py list
    python3 eval/system/run_system.py run-scenario --scenario eval/system/scenarios/mqtt-telemetry-flow.json
    python3 eval/system/run_system.py run-all
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import traceback
from pathlib import Path

_SYSTEM_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(_SYSTEM_DIR.parent / "lib"))

from server import TestServer  # noqa: E402
from seed import seed_extras  # noqa: E402
from state_query import run_query  # noqa: E402
from mqtt_device import MqttDeviceSimulator  # noqa: E402
from ws_events import WSEventSubscriber  # noqa: E402

SCENARIOS_DIR = _SYSTEM_DIR / "scenarios"


def _dot_match(evt: dict, match: dict) -> bool:
    """Dotted-path equality: {"data.metric": "temperature"} vs the event dict."""
    for key, want in (match or {}).items():
        cur: object = evt
        for part in key.split("."):
            if isinstance(cur, dict):
                cur = cur.get(part)
            else:
                cur = None
                break
        if cur != want:
            return False
    return True


def run_scenario(path: str) -> dict:
    sc = json.loads(Path(path).read_text(encoding="utf-8"))
    result: dict = {
        "id": sc.get("id", Path(path).stem),
        "assertions": [],
        "ws_assertions": [],
        "status": "error",
        "error": None,
    }
    srv: TestServer | None = None
    sub: WSEventSubscriber | None = None
    dev: MqttDeviceSimulator | None = None
    try:
        srv = TestServer()
        srv.spawn(case_id=result["id"])
        seed_extras(srv, (sc.get("setup") or {}).get("extras") or {})

        # Adapter-readiness gate: the embedded broker + MQTT adapter subscribe
        # asynchronously and can lag /health. Publishing before the subscription
        # is live silently drops the message. Wait on the server log for the
        # deterministic signal ("... default subscription acknowledged ...").
        if srv.wait_for_log("subscription acknowledged", timeout=30.0) is None:
            raise RuntimeError(
                "MQTT adapter never acknowledged its subscription — broker not ready"
            )

        action = sc["action"]
        if action.get("type") == "mqtt_publish":
            dev = MqttDeviceSimulator(action["device_id"], action["device_type"])
            dev.connect(timeout=15.0)  # broker-readiness gate (CONNACK)
            # Optional downlink capture: subscribe the simulator to a command topic.
            if action.get("subscribe_downlink"):
                dev.subscribe_for_commands(action["subscribe_downlink"], on_command=lambda p: None)
            # Start the WS subscriber BEFORE the publish so it catches the event.
            sub = WSEventSubscriber("127.0.0.1", srv.port, srv.api_key, category="device")
            sub.start(timeout=10.0)
            time.sleep(0.3)
            dev.publish_telemetry(action["payload"], topic=action.get("topic"))
            # Optional offline test: sever the connection so the broker's
            # presence hook fires DeviceTransportOffline.
            if action.get("connect_then_disconnect"):
                dev.disconnect()
                time.sleep(2.0)
        else:
            raise RuntimeError(f"unknown action type: {action.get('type')}")

        # HTTP / simulator asserts (state_query, plus MQTT-specific ones).
        for a in sc.get("asserts", []):
            if a["type"] == "downlink_received":
                want = str(a.get("params", {}).get("command") or a.get("expected") or "")
                timeout = float(a.get("params", {}).get("timeout", 10))
                # The rule → execute → downlink chain is async: poll the
                # simulator's capture until the first message arrives.
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline and not dev.received_downlink:
                    time.sleep(0.1)
                captured = list(getattr(dev, "received_downlink", []) or [])
                passed = len(captured) >= 1
                result["assertions"].append(
                    {"type": a["type"], "params": a.get("params", {}),
                     "expected": f"≥1 downlink (wanted {want})" if want else "≥1 downlink",
                     "actual": captured, "passed": passed}
                )
            elif a["type"] == "device_transport_offline":
                # Poll GET /devices/:id until transport_connected flips false
                # (the broker presence hook fires async after disconnect).
                import requests as _requests
                timeout = float(a.get("params", {}).get("timeout", 20))
                want_id = a["params"]["device_id"]
                seen_false = False
                last_conn = None
                deadline = time.monotonic() + timeout
                while time.monotonic() < deadline:
                    try:
                        r = _requests.get(
                            f"{srv.api_base}/devices/{want_id}",
                            headers={"Authorization": f"Bearer {srv.api_key}"}, timeout=5,
                        )
                        body = r.json()
                        dev_body = body.get("data", body) if isinstance(body, dict) else body
                        last_conn = dev_body.get("transport_connected") if isinstance(dev_body, dict) else None
                        if last_conn is False:
                            seen_false = True
                            break
                    except Exception:
                        pass
                    time.sleep(0.5)
                result["assertions"].append(
                    {"type": a["type"], "params": a.get("params", {}),
                     "expected": True, "actual": seen_false, "passed": seen_false}
                )
            else:
                result["assertions"].append(run_query(a, srv.api_base, srv.api_key))

        # Live WS event asserts.
        for w in sc.get("ws_asserts", []):
            match = w.get("match", {})
            pred = lambda e: e.get("type") == w.get("type") and _dot_match(e, match)  # noqa: E731
            evt = sub.wait_for(pred, timeout=w.get("timeout", 10)) if sub else None
            result["ws_assertions"].append(
                {"type": w.get("type"), "match": match, "passed": evt is not None, "actual": evt}
            )

        ok_asserts = all(a["passed"] for a in result["assertions"])
        ok_ws = all(a["passed"] for a in result["ws_assertions"])
        result["status"] = "passed" if (ok_asserts and ok_ws) else "failed"
    except Exception as e:
        result["error"] = f"{type(e).__name__}: {e}"
        result["status"] = "error"
        traceback.print_exc()
    finally:
        if sub is not None:
            sub.stop()
        if dev is not None:
            dev.disconnect()
        if srv is not None:
            try:
                srv.shutdown()
            except Exception:
                pass
    return result


def cmd_run_scenario(args) -> None:
    r = run_scenario(args.scenario)
    print(json.dumps(r, ensure_ascii=False, indent=2, default=str))
    sys.exit(0 if r["status"] == "passed" else 1)


def cmd_run_all(args) -> None:
    files = sorted(SCENARIOS_DIR.glob("*.json"))
    if not files:
        print(f"no scenarios in {SCENARIOS_DIR}", file=sys.stderr)
        sys.exit(1)
    results = []
    for f in files:
        print(f"--- {f.name} ---", file=sys.stderr)
        r = run_scenario(str(f))
        results.append(r)
        print(json.dumps({"id": r["id"], "status": r["status"]}, ensure_ascii=False))
    npass = sum(1 for r in results if r["status"] == "passed")
    print(f"\n{npass}/{len(results)} passed", file=sys.stderr)
    sys.exit(0 if npass == len(results) else 1)


def cmd_list(args) -> None:
    for f in sorted(SCENARIOS_DIR.glob("*.json")):
        try:
            print(json.load(open(f, encoding="utf-8"))["id"])
        except Exception:
            print(f"? {f.name}")


def main() -> None:
    p = argparse.ArgumentParser(prog="run_system")
    sub = p.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("run-scenario")
    a.add_argument("--scenario", required=True)
    a.set_defaults(func=cmd_run_scenario)
    a = sub.add_parser("run-all")
    a.set_defaults(func=cmd_run_all)
    a = sub.add_parser("list")
    a.set_defaults(func=cmd_list)
    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()

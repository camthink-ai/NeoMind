"""Device simulator — sends continuous telemetry via webhook ingestion.

Runs in a background thread, periodically POSTing telemetry to the device's
webhook endpoint. Supports metric drift (gradual change over time), which
enables realistic testing of rules that fire on CONTINUOUS data (not one-shot
injection).

Usage in eval runtime block:
    "runtime": {
        "simulators": [
            {"device_id": "webhook-sensor", "interval": 2,
             "metrics": {"battery_level": 30.0},
             "drift": {"battery_level": {"rate": -1.0, "min": 0}}}
        ],
        "wait_ms": 15000,
        "expect": [{"type": "message_count", "expected_min": 1}]
    }

The simulator sends telemetry every `interval` seconds. Drift rates are
applied per-send (rate * interval). The webhook ingestion path publishes
DeviceMetric events → rules/transforms fire on real continuous data.

For MQTT-based simulation (bidirectional: telemetry up + commands down), a
future MqttDeviceSimulator would connect to the embedded broker, subscribe
to downlink topics, and support real offline/reconnect.
"""
from __future__ import annotations

import threading
import time

import requests


class DeviceSimulator:
    """Simulates a physical IoT device sending telemetry via webhook ingestion.

    Runs in a daemon thread — automatically stops when the main thread exits.
    """

    def __init__(
        self,
        api_base: str,
        api_key: str,
        device_id: str,
        interval: float = 2.0,
    ):
        self.api_base = api_base
        self.api_key = api_key
        self.device_id = device_id
        self.interval = interval
        self.metrics: dict = {}
        self._drifts: dict[str, dict] = {}  # name -> {rate, min, max}
        self._thread: threading.Thread | None = None
        self._running = False
        self._send_count = 0

    def set_metric(self, name: str, value):
        """Set a metric to a fixed value."""
        self.metrics[name] = value

    def drift_metric(self, name: str, rate: float, min_val=None, max_val=None):
        """Configure gradual drift for a metric (value changes per send).

        rate: change per SECOND (applied as rate * interval per send).
        min_val/max_val: optional clamps.
        """
        self._drifts[name] = {"rate": rate, "min": min_val, "max": max_val}

    def start(self):
        """Start sending telemetry in background."""
        self._running = True
        self._send_count = 0
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def stop(self):
        """Stop sending — simulates the device going offline."""
        self._running = False
        if self._thread:
            self._thread.join(timeout=5)

    @property
    def send_count(self) -> int:
        """How many telemetry sends completed."""
        return self._send_count

    def _run(self):
        """Background loop: apply drifts + send telemetry."""
        url = f"{self.api_base}/devices/{self.device_id}/webhook"
        headers = {"Authorization": f"Bearer {self.api_key}"}

        while self._running:
            # Apply drifts
            for name, cfg in self._drifts.items():
                if name in self.metrics:
                    val = self.metrics[name]
                    val += cfg["rate"] * self.interval
                    if cfg.get("min") is not None:
                        val = max(cfg["min"], val)
                    if cfg.get("max") is not None:
                        val = min(cfg["max"], val)
                    self.metrics[name] = val

            # Send current metrics via webhook ingestion
            try:
                requests.post(url, json=dict(self.metrics), headers=headers, timeout=5)
                self._send_count += 1
            except Exception:
                pass  # silently skip failed sends (device "offline" moment)

            time.sleep(self.interval)


def start_simulators(api_base: str, api_key: str, configs: list) -> list[DeviceSimulator]:
    """Start multiple simulators from runtime config.

    Each config: {device_id, interval, metrics: {name: val}, drift: {name: {rate, min, max}}}
    Returns the list of started simulators (call stop_simulators() to stop).
    """
    sims = []
    for cfg in configs:
        sim = DeviceSimulator(
            api_base=api_base,
            api_key=api_key,
            device_id=cfg["device_id"],
            interval=cfg.get("interval", 2.0),
        )
        for name, val in (cfg.get("metrics") or {}).items():
            sim.set_metric(name, val)
        for name, drift in (cfg.get("drift") or {}).items():
            sim.drift_metric(
                name,
                rate=drift["rate"],
                min_val=drift.get("min"),
                max_val=drift.get("max"),
            )
        sim.start()
        sims.append(sim)
    return sims


def stop_simulators(sims: list[DeviceSimulator]):
    """Stop all simulators."""
    for sim in sims:
        sim.stop()

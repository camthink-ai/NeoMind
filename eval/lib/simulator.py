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
        self._events: list[tuple[float, dict]] = []  # [(at_seconds, {metric: val})]
        self._offline_at: float | None = None
        self._reconnect_at: float | None = None
        self._thread: threading.Thread | None = None
        self._running = False
        self._send_count = 0
        self._offline_count = 0  # sends skipped while "offline"

    def set_metric(self, name: str, value):
        """Set a metric to a fixed value."""
        self.metrics[name] = value

    def drift_metric(self, name: str, rate: float, min_val=None, max_val=None):
        """Configure gradual drift for a metric (value changes per send).

        rate: change per SECOND (applied as rate * interval per send).
        min_val/max_val: optional clamps.
        """
        self._drifts[name] = {"rate": rate, "min": min_val, "max": max_val}

    def add_event(self, at_seconds: float, metrics: dict):
        """Schedule a discrete metric change at a specific time.

        Enables testing rules on DISCRETE conditions (door opens, compressor
        stops) rather than just gradual drift. Events fire once and are removed.
        """
        self._events.append((at_seconds, dict(metrics)))

    def schedule_offline(self, at_seconds: float, reconnect_at: float | None = None):
        """Simulate device going offline at a specific time.

        The simulator stops sending telemetry (device "disappears"). If
        reconnect_at is set, it resumes sending at that time. Tests NeoMind's
        4-state offline detection.
        """
        self._offline_at = at_seconds
        self._reconnect_at = reconnect_at

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
        """Background loop: apply events/drifts + send telemetry (with offline)."""
        import time as _time

        url = f"{self.api_base}/devices/{self.device_id}/webhook"
        headers = {"Authorization": f"Bearer {self.api_key}"}
        start = _time.monotonic()

        while self._running:
            elapsed = _time.monotonic() - start

            # Check offline/reconnect schedule
            is_offline = (
                self._offline_at is not None
                and elapsed >= self._offline_at
                and (
                    self._reconnect_at is None
                    or elapsed < self._reconnect_at
                )
            )
            if is_offline:
                self._offline_count += 1
                _time.sleep(self.interval)
                continue  # skip sending — device is "offline"

            # Apply timed events (fire once at scheduled time)
            remaining = []
            for at, metrics in self._events:
                if elapsed >= at:
                    self.metrics.update(metrics)
                else:
                    remaining.append((at, metrics))
            self._events = remaining

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
                pass

            _time.sleep(self.interval)


def start_simulators(api_base: str, api_key: str, configs: list) -> list[DeviceSimulator]:
    """Start multiple simulators from runtime config.

    Each config supports:
      device_id:           which device to simulate
      interval:            send frequency (seconds, default 2)
      metrics:             {name: value} starting values
      drift:               {name: {rate, min, max}} gradual change per second
      events:              [{at_seconds, set: {name: val}}] timed discrete changes
      offline_at:          seconds to go offline (stop sending)
      reconnect_at:        seconds to reconnect (resume sending)
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
        for event in (cfg.get("events") or []):
            sim.add_event(event["at_seconds"], event.get("set") or {})
        if cfg.get("offline_at") is not None:
            sim.schedule_offline(
                cfg["offline_at"],
                reconnect_at=cfg.get("reconnect_at"),
            )
        sim.start()
        sims.append(sim)
    return sims


def stop_simulators(sims: list[DeviceSimulator]):
    """Stop all simulators."""
    for sim in sims:
        sim.stop()

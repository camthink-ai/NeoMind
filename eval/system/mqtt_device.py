"""MQTT device simulator — publishes telemetry to the embedded broker.

Realizes the ``MqttDeviceSimulator`` TODO named in ``eval/lib/simulator.py``.
Uses paho-mqtt (sync ``Client`` + ``loop_start`` background network thread) so
the same client can BOTH publish telemetry up AND subscribe for downlink
commands (future scenarios). Connects anonymously — the embedded broker
defaults to ``auth_enabled=false`` (``config.rs:435``).

Topic contract: a device publishes JSON to ``device/{device_type}/{device_id}/uplink``
(``crates/neomind-devices/src/adapters/mqtt.rs:1811`` requires
``parts[0]=="device"`` and ``parts[3]=="uplink"``).
"""
from __future__ import annotations

import json
import threading
import time

import paho.mqtt.client as paho


class MqttDeviceSimulator:
    """Simulates a physical IoT device talking to the embedded MQTT broker.

    ``connect()`` is the explicit broker-readiness gate: the embedded broker
    (``0.0.0.0:1883``) starts asynchronously and can lag ``/health`` going
    green, so we retry the socket connect and block until a CONNACK arrives.
    """

    def __init__(
        self,
        device_id: str,
        device_type: str,
        broker_host: str = "127.0.0.1",
        broker_port: int = 1883,
        client_id: str | None = None,
    ):
        self.device_id = device_id
        self.device_type = device_type
        self.broker_host = broker_host
        self.broker_port = broker_port
        self._client_id = client_id or f"neomind-sim-{device_id}-{int(time.time()*1000) % 100000}"
        self._client: paho.Client | None = None
        self._connected = threading.Event()
        self._command_handlers: dict[str, callable] = {}
        self.received_downlink: list[str] = []  # every message on subscribed topics

    def _on_connect(self, client, userdata, flags, reason_code, properties=None):
        # reason_code == 0 (or a non-failure ReasonCode) ⇒ CONNACK success.
        if getattr(reason_code, "is_failure", False) is False:
            self._connected.set()
            for topic in self._command_handlers:
                client.subscribe(topic)

    def _on_message(self, client, userdata, msg):
        payload = msg.payload.decode("utf-8", "replace")
        self.received_downlink.append(payload)  # record all downlink deliveries
        cb = self._command_handlers.get(msg.topic)
        if cb:
            try:
                cb(payload)
            except Exception:
                # A handler error must not kill the network loop.
                pass

    def connect(self, timeout: float = 15.0) -> "MqttDeviceSimulator":
        """Block until the broker accepts the connection (CONNACK).

        Retries the socket connect while the broker is still binding 1883.
        Raises RuntimeError on timeout with an unambiguous message.
        """
        self._client = paho.Client(
            paho.CallbackAPIVersion.VERSION2, client_id=self._client_id
        )
        self._client.on_connect = self._on_connect
        self._client.on_message = self._on_message
        deadline = time.monotonic() + timeout
        last_err = None
        while time.monotonic() < deadline:
            try:
                self._client.connect(self.broker_host, self.broker_port, keepalive=60)
                break
            except (ConnectionRefusedError, OSError) as e:
                last_err = e
                time.sleep(0.3)
        else:
            raise RuntimeError(
                f"MQTT broker {self.broker_host}:{self.broker_port} not ready in "
                f"{timeout}s (last: {last_err})"
            )
        self._client.loop_start()
        if not self._connected.wait(timeout=max(1.0, deadline - time.monotonic())):
            try:
                self._client.loop_stop()
            except Exception:
                pass
            raise RuntimeError(
                f"MQTT connect to {self.broker_host}:{self.broker_port} got no CONNACK"
            )
        return self

    def publish_telemetry(
        self,
        metrics: dict,
        topic: str | None = None,
        qos: int = 1,
        wait_for_publish: bool = True,
        timeout: float = 5.0,
    ) -> str:
        """Publish ``{metric: value, ...}`` JSON to the uplink topic.

        ``topic`` defaults to ``device/{device_type}/{device_id}/uplink``.
        ``wait_for_publish=True`` at QoS>=1 blocks for PUBACK before returning.
        """
        if self._client is None:
            raise RuntimeError("publish_telemetry called before connect()")
        topic = topic or f"device/{self.device_type}/{self.device_id}/uplink"
        payload = json.dumps(metrics)
        info = self._client.publish(topic, payload, qos=qos)
        if wait_for_publish:
            info.wait_for_publish(timeout=timeout)
        return topic

    def subscribe_for_commands(self, command_topic: str, on_command: callable) -> None:
        """Subscribe to a downlink topic; ``on_command(payload: str)`` per message.

        Stub used by future downlink scenarios (agent/rule → device command).
        """
        self._command_handlers[command_topic] = on_command
        if self._client:
            self._client.subscribe(command_topic)

    def disconnect(self) -> None:
        if self._client:
            try:
                self._client.loop_stop()
                self._client.disconnect()
            except Exception:
                pass

"""Live WebSocket event subscriber — proves the dashboard live-update path.

Connects to ``/api/events/ws?api_key=<key>&category=device`` (pre-auth path,
``events.rs:527-536``) and captures ``DeviceMetric`` events into a thread-safe
buffer. This is NOT a ``state_query`` (those are synchronous point-in-time
GETs); it is a live-event capture that must be started BEFORE the action that
should produce the event.

Protocol contract (``crates/neomind-api/src/handlers/events.rs``):
- Server replies ``{"type":"Authenticated"}`` after pre-auth via query param.
- Frames arrive as EITHER a single ``{id,type,timestamp,source,data}`` OR a
  batch ``{"batch":true,"events":[...]}``. ``DeviceMetric`` is NOT in
  ``immediate_events`` (events.rs:43-47) so it is batched (≤10 / 50ms).
- Server pings every 30s; client must reply ``{"type":"pong"}`` or get killed
  after 60s (events.rs:655-663) — auto-replied here for future long scenarios.
"""
from __future__ import annotations

import json
import threading
import time

from websockets.sync.client import connect as ws_connect


class WSEventSubscriber:
    def __init__(
        self,
        host: str,
        port: int,
        api_key: str,
        category: str = "device",
        event_types: list[str] | None = None,
    ):
        self.url = f"ws://{host}:{port}/api/events/ws?api_key={api_key}&category={category}"
        if event_types:
            for t in event_types:
                self.url += f"&event_type={t}"
        self._events: list[dict] = []
        self._lock = threading.Lock()
        self._thread: threading.Thread | None = None
        self._ws = None
        self._stop = threading.Event()
        self._authed = threading.Event()

    def start(self, timeout: float = 10.0) -> "WSEventSubscriber":
        """Open the WS in a daemon thread; block until Authenticated. Raises on timeout."""
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        if not self._authed.wait(timeout):
            self.stop()
            raise RuntimeError(
                f"WS auth timeout on {self.url} (no Authenticated frame in {timeout}s)"
            )
        return self

    def _run(self) -> None:
        try:
            self._ws = ws_connect(self.url)
            for raw in self._ws:
                if self._stop.is_set():
                    break
                try:
                    msg = json.loads(raw)
                except (ValueError, TypeError):
                    continue
                if not isinstance(msg, dict):
                    continue
                mtype = msg.get("type")
                if mtype == "Authenticated":
                    self._authed.set()
                    continue
                if mtype == "ping":
                    try:
                        self._ws.send(json.dumps({"type": "pong"}))
                    except Exception:
                        pass
                    continue
                if mtype == "pong":
                    continue
                if mtype == "Error":
                    with self._lock:
                        self._events.append(msg)
                    continue
                # Flatten single + batch frames.
                if msg.get("batch") is True and isinstance(msg.get("events"), list):
                    for e in msg["events"]:
                        if isinstance(e, dict):
                            with self._lock:
                                self._events.append(e)
                else:
                    with self._lock:
                        self._events.append(msg)
        except Exception:
            pass  # connection closed / stopped — drain what we got

    def wait_for(self, predicate, timeout: float = 10.0) -> dict | None:
        """Block until an event matching ``predicate(evt)`` arrives, else None.

        Non-destructive: scans the full captured history on each poll, so
        multiple ws_asserts can each scan the same events.
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                for e in self._events:
                    if predicate(e):
                        return e
            time.sleep(0.05)
        return None

    def stop(self) -> None:
        self._stop.set()
        try:
            if self._ws is not None:
                self._ws.close()
        except Exception:
            pass

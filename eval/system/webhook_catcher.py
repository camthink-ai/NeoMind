"""Local HTTP receiver for testing outbound webhook delivery (data-push).

Spins up a background HTTP server that records every POST body it receives,
so a system scenario can assert that a data-push target actually delivered a
payload end-to-end (the "does the push reach the outside world" proof that
`push_enabled` alone can't give).
"""
from __future__ import annotations

import threading
import http.server
import socketserver
import json
import time
from urllib.parse import urlparse


class _Handler(http.server.BaseHTTPRequestHandler):
    received: list[dict] = []

    def do_POST(self):
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length).decode("utf-8", "replace")
        type(self).received.append(
            {"path": self.path, "headers": dict(self.headers), "body": body}
        )
        self.send_response(200)
        self.end_headers()

    def log_message(self, *args):  # silence
        pass


class WebhookCatcher:
    """Background HTTP server on an ephemeral port that records POSTs."""

    def __init__(self, host: str = "127.0.0.1"):
        self.host = host
        self.port: int = 0
        self._server: socketserver.TCPServer | None = None
        self._thread: threading.Thread | None = None

    def start(self) -> "WebhookCatcher":
        _Handler.received = []
        self._server = socketserver.TCPServer((self.host, 0), _Handler)
        self.port = self._server.server_address[1]
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        return self

    @property
    def url(self) -> str:
        return f"http://{self.host}:{self.port}/hook"

    def received(self) -> list[dict]:
        return list(_Handler.received)

    def wait_for_post(self, timeout: float = 10.0) -> dict | None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if _Handler.received:
                return _Handler.received[-1]
            time.sleep(0.1)
        return None

    def stop(self) -> None:
        if self._server:
            self._server.shutdown()
            self._server.server_close()

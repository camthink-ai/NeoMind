"""TestServer: spawn `neomind serve` per case with full isolation.

Python port of crates/eval-runner/src/test_server.rs, plus the seed/HTTP-chat
helpers that lived in seed.rs.

Architecture (pivoted 2026-06-29 per user feedback):
- Agent under test runs INSIDE the `neomind serve` subprocess via the
  production chat pipeline (POST /api/sessions/:id/chat). This exercises the
  real system prompts, real tool registry, real multi-round tool-calling
  continuation, real list-only-dead-end detection — everything the chat UI
  exercises. (The previous in-process SessionManager bypassed all of it and
  caused multi-tool calls to silently fail in eval despite working fine in
  the chat UI.)
- LLM backend is configured via the standard API (POST /api/llm-backends +
  /activate) so the eval exercises whatever backend AGENT_LLM_* points at.
- Auth: we run `neomind api-key create` BEFORE spawning the server to
  pre-seed `<tmpdir>/data/api_keys.redb` with a known plaintext key. Then
  the server's `AuthState::new()` (CWD-relative path → resolves to same
  file) loads that key. This avoids needing to read redb from Python.
"""
from __future__ import annotations

import asyncio
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Optional

import requests


def _find_free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _resolve_neomind_bin() -> str:
    """Locate the neomind binary.

    Priority:
    1. $NEOMIND_TEST_BIN override
    2. <cwd>/target/release/neomind (workspace's freshly-built binary —
       preferred over PATH because installed `neomind` is often stale; e.g.
       v0.8.11 on PATH while repo is at v0.9.0 caused silent API drift)
    3. `neomind` on PATH (last resort)
    """
    env = os.environ.get("NEOMIND_TEST_BIN")
    if env:
        return env
    cwd = os.getcwd()
    candidate = os.path.join(cwd, "target", "release", "neomind")
    if os.path.exists(candidate):
        _warn_if_stale(candidate)
        return candidate
    return "neomind"  # fall back to PATH


def _warn_if_stale(bin_path: str) -> None:
    """Warn loudly if the release binary predates any source file.

    Eval results from a stale binary are meaningless — recent fixes don't
    take effect and cases fail for reasons already patched. We compare the
    binary mtime against the newest .rs file under crates/ and print a
    hard-to-miss warning when the binary is older. Set NEOMIND_SKIP_STALE_CHECK=1
    to suppress (e.g. for prod binaries built elsewhere).
    """
    if os.environ.get("NEOMIND_SKIP_STALE_CHECK"):
        return
    try:
        bin_mtime = os.path.getmtime(bin_path)
    except OSError:
        return
    newest_src = 0.0
    newest_path = ""
    for root, _dirs, files in os.walk("crates"):
        for f in files:
            if f.endswith(".rs"):
                p = os.path.join(root, f)
                try:
                    m = os.path.getmtime(p)
                except OSError:
                    continue
                if m > newest_src:
                    newest_src = m
                    newest_path = p
    if newest_src > bin_mtime:
        from datetime import datetime
        bin_t = datetime.fromtimestamp(bin_mtime).strftime("%H:%M:%S")
        src_t = datetime.fromtimestamp(newest_src).strftime("%H:%M:%S")
        print(
            "⚠️  STALE BINARY WARNING: target/release/neomind was built at "
            f"{bin_t} but {newest_path} was modified at {src_t}.\n"
            "    Recent source changes are NOT in the binary the eval will "
            "run against.\n"
            "    Rebuild with `cargo build --release -p neomind-cli` before "
            "trusting eval results,\n"
            "    or set NEOMIND_SKIP_STALE_CHECK=1 to silence this warning.",
            file=__import__("sys").stderr,
            flush=True,
        )


def _precreate_api_key(data_dir: Path) -> str:
    """Run `neomind api-key create` to seed a known plaintext key.

    Returns the plaintext key. The same redb file will be loaded by the
    subprocess because its CWD = tmpdir makes the hardcoded `data/api_keys.redb`
    resolve to the same path.
    """
    data_dir.mkdir(parents=True, exist_ok=True)
    data_dir_str = str(data_dir)
    bin_path = _resolve_neomind_bin()
    result = subprocess.run(
        [bin_path, "api-key", "create", "--name", "eval", "--data-dir", data_dir_str],
        capture_output=True,
        text=True,
        timeout=30,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"api-key create failed (rc={result.returncode}):\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    # Output format (main.rs:1503-1509):
    #   API Key created successfully!
    #   Name: eval
    #   ID:   <uuid>
    #   Key:  <plaintext>
    for line in result.stdout.splitlines():
        stripped = line.strip()
        if stripped.startswith("Key:"):
            return stripped[4:].strip()
    raise RuntimeError(
        f"could not parse Key from api-key create output:\n{result.stdout}"
    )


class TestServer:
    """A per-case `neomind serve` subprocess with a temp data dir.

    Use as a context manager (`async with` not needed — sync) or call
    spawn()/shutdown() directly.
    """

    def __init__(self):
        self.tmpdir: Optional[tempfile.TemporaryDirectory] = None
        self.process: Optional[subprocess.Popen] = None
        self.api_base: str = ""
        self.api_key: str = ""
        self._out_thread = None
        self._err_thread = None
        self.port: int = 0
        import threading
        self._log_lines: list[str] = []
        self._log_lock = threading.Lock()

    def spawn(self, startup_timeout: float = 30.0, case_id: Optional[str] = None) -> "TestServer":
        self.tmpdir = tempfile.TemporaryDirectory(prefix="neomind-eval-")
        tmpdir_path = Path(self.tmpdir.name)
        data_dir = tmpdir_path / "data"

        # 1. Pre-seed a known API key (also creates the data dir + redb file).
        self.api_key = _precreate_api_key(data_dir)

        # 2. Pre-bind a port (avoids parsing the startup banner — the banner
        #    uses `bind.to_string()` which prints `127.0.0.1:0` when
        #    `--port 0` is used, not the actual OS-assigned port).
        self.port = _find_free_port()
        self.api_base = f"http://127.0.0.1:{self.port}/api"

        # 3. Spawn `neomind serve` with CWD=tmpdir so `AuthState::new()`'s
        #    hardcoded `data/api_keys.redb` lands inside tmpdir/data — same
        #    file we just seeded. Pass NEOMIND_API_BASE so in-process shell
        #    dispatch (CLAUDE.md "CLI In-Process Dispatch") inside the agent
        #    talks back to this same server.
        bin_path = _resolve_neomind_bin()
        env = os.environ.copy()
        env["NEOMIND_DATA_DIR"] = str(tmpdir_path)
        env["NEOMIND_API_BASE"] = self.api_base
        env["NEOMIND_API_KEY"] = self.api_key

        # Parallel-shard support: the embedded MQTT broker defaults to :1883
        # and its bind FAILS (logged, server continues with degraded device
        # flows) when another worker's broker already holds it. Serial runs
        # are unaffected; when NEOMIND_EVAL_MQTT_PORT is set, seed a
        # config.toml in the per-case CWD so this worker's broker binds a
        # private port (fresh settings redb inherits from config.toml on
        # first run — see get_embedded_broker_config).
        mqtt_port = os.environ.get("NEOMIND_EVAL_MQTT_PORT")
        if mqtt_port:
            (tmpdir_path / "config.toml").write_text(
                f"[mqtt]\nport = {int(mqtt_port)}\n"
            )

        # Per-case SFT trace isolation: when NEOMIND_TRACE_ROOT is set, route
        # each case's LLM traces into its own subdir so a batch run's traces
        # stay grouped by case. The SFT renderer then joins one case's trace
        # dir with its CaseRecord. Opt-in — unset => unchanged behavior.
        trace_root = os.environ.get("NEOMIND_TRACE_ROOT")
        if trace_root and case_id:
            case_trace_dir = Path(trace_root) / case_id
            case_trace_dir.mkdir(parents=True, exist_ok=True)
            # MUST be absolute: the subprocess runs with CWD=tmpdir, so a
            # relative path would route the trace into the tmpdir (deleted
            # after the case) — silently losing every trace.
            env["NEOMIND_TRACE_DIR"] = str(case_trace_dir.resolve())

        self.process = subprocess.Popen(
            [bin_path, "serve", "--host", "127.0.0.1", "--port", str(self.port)],
            cwd=str(tmpdir_path),
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            # Kill the whole process group on shutdown — neomind spawns
            # child threads / extension subprocesses we don't want leaking.
            start_new_session=True,
        )

        # 4. Drain stdout/stderr on background threads so pipe buffers don't
        #    fill and block the child. Echo to our stderr for debugging.
        import threading

        def _drain(stream, prefix):
            for raw in iter(stream.readline, b""):
                if not raw:
                    break
                try:
                    line = raw.decode("utf-8", errors="replace")
                except Exception:
                    line = repr(raw)
                with self._log_lock:
                    self._log_lines.append(line.rstrip())
                sys_stderr_write(f"{prefix} {line.rstrip()}\n")

        threading.Thread(
            target=_drain, args=(self.process.stdout, "[server:out]"), daemon=True
        ).start()
        threading.Thread(
            target=_drain, args=(self.process.stderr, "[server:err]"), daemon=True
        ).start()

        # 5. Poll /health until ready.
        deadline = time.monotonic() + startup_timeout
        last_err = None
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(
                    f"neomind serve exited early (rc={self.process.returncode})"
                )
            try:
                r = requests.get(f"{self.api_base}/health", timeout=2)
                if r.status_code < 500:
                    return self
            except requests.RequestException as e:
                last_err = e
            time.sleep(0.2)
        raise RuntimeError(
            f"server never became healthy at {self.api_base} within "
            f"{startup_timeout}s (last error: {last_err})"
        )

    def wait_for_log(self, needle: str, timeout: float = 30.0) -> str | None:
        """Block until the server's stdout/stderr contains `needle`.

        The embedded MQTT broker and its adapters start asynchronously and can
        lag ``/health`` going green — publishing telemetry before the adapter
        has subscribed silently drops the message. This is the deterministic
        readiness gate for system tests (vs a fixed sleep).
        """
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._log_lock:
                for line in self._log_lines:
                    if needle in line:
                        return line
            time.sleep(0.2)
        return None

    def shutdown(self):
        if self.process is None:
            return
        try:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass  # leaked; tmpdir cleanup still happens
        finally:
            if self.tmpdir is not None:
                self.tmpdir.cleanup()
                self.tmpdir = None

    # --- HTTP helpers ------------------------------------------------------

    def _headers(self):
        return {"Authorization": f"Bearer {self.api_key}"}

    def post(self, path: str, body) -> requests.Response:
        url = f"{self.api_base}{path}" if path.startswith("/") else f"{self.api_base}/{path}"
        return requests.post(url, json=body, headers=self._headers(), timeout=60)

    def get(self, path: str, timeout: float = 30.0) -> requests.Response:
        url = f"{self.api_base}{path}" if path.startswith("/") else f"{self.api_base}/{path}"
        return requests.get(url, headers=self._headers(), timeout=timeout)

    def configure_llm_backend(self):
        """Configure the agent-under-test's LLM backend via API.

        Reads AGENT_LLM_* env vars (same contract as the previous Rust runner).
        """
        api_key = os.environ.get("AGENT_LLM_API_KEY")
        endpoint = os.environ.get("AGENT_LLM_ENDPOINT")
        model = os.environ.get("AGENT_LLM_MODEL")
        if not (api_key and endpoint and model):
            raise RuntimeError(
                "AGENT_LLM_API_KEY / AGENT_LLM_ENDPOINT / AGENT_LLM_MODEL required"
            )
        backend_type = os.environ.get("AGENT_LLM_BACKEND_TYPE", "openai")
        thinking = os.environ.get("AGENT_LLM_THINKING", "false").lower() in ("1", "true")

        body = {
            "name": "eval-agent",
            "backend_type": backend_type,
            "endpoint": endpoint,
            "model": model,
            "api_key": api_key,
            "temperature": float(os.environ.get("AGENT_LLM_TEMPERATURE", "0.6")),
            "top_p": float(os.environ.get("AGENT_LLM_TOP_P", "0.85")),
            "thinking_enabled": thinking,
        }
        r = self.post("/llm-backends", body)
        if not r.ok:
            raise RuntimeError(f"create llm backend -> {r.status_code}: {r.text}")
        data = r.json().get("data", r.json())
        bid = data.get("id")
        if not bid:
            raise RuntimeError(f"missing backend id in response: {r.text}")

        r2 = self.post(f"/llm-backends/{bid}/activate", {})
        if not r2.ok:
            raise RuntimeError(
                f"activate backend -> {r2.status_code}: {r2.text}"
            )

    def create_chat_session(self) -> str:
        r = self.post("/sessions", {})
        if not r.ok:
            raise RuntimeError(f"create session -> {r.status_code}: {r.text}")
        data = r.json().get("data", r.json())
        sid = data.get("sessionId") or data.get("id")
        if not sid:
            raise RuntimeError(f"missing sessionId in response: {r.text}")
        return sid

    def get_history(self, session_id: str) -> list:
        """Fetch full session message history.

        Each message has: role, content, tool_calls (with name/id/arguments/
        result/round), tool_call_id, tool_call_name, thinking, round_contents,
        round_thinking, timestamp. This is the rich trace the ChatResponse
        doesn't expose — see crates/neomind-agent/src/agent/types.rs:321.
        """
        r = requests.get(
            f"{self.api_base}/sessions/{session_id}/history",
            headers=self._headers(),
            timeout=30,
        )
        if not r.ok:
            raise RuntimeError(f"history -> {r.status_code}: {r.text}")
        body = r.json()
        data = body.get("data", body)
        return data.get("messages", []) or []

    def chat(self, session_id: str, message: str, timeout: float | None = None,
             images: list[str] | None = None) -> dict:
        # timeout: env-tunable (EVAL_CHAT_TIMEOUT, default 1400). Raised 900→1400
        # (2026-08-14) because the chat path's own turn bound is
        # max_stream_duration=1200s — a 900s outer bound killed turns the agent
        # would have completed (empty-error agent_failed). Slow local models
        # (60-90 tok/s) make 20+-round deploy cases legitimately long; set
        # EVAL_CHAT_TIMEOUT=2400 etc. when re-running the heavyweight cases.
        if timeout is None:
            timeout = float(os.environ.get("EVAL_CHAT_TIMEOUT", "1400"))
        """One chat turn via WebSocket (production chat UI path).

        Routes through `ws://.../api/chat?api_key=...` →
        `process_message_events_with_backend_and_skills` →
        `agent::streaming::stream_core::process_stream_events_with_safeguards`,
        which is the SAME multi-round ReAct loop the chat UI uses (up to 30
        LLM rounds with tool-result feedback). The HTTP POST path
        (`process_message` → `process_with_llm`) is single-round only and was
        silently capping agent capabilities — see mod.rs:2153
        "unified ReAct loop — no Phase 2".

        Returns {response, tools_used, processing_time_ms, new_messages,
        tool_calls_stream, thinking_stream} where the *_stream fields are
        accumulated live from WS events (ToolCallStart/ToolCallEnd/Thinking)
        and new_messages is the post-call history delta.

        WS event protocol (handlers/sessions.rs:75-263):
        - Thinking / Content / ToolCallStart / ToolCallEnd / intermediate_end
        - Intent / Plan / Progress / Warning / Heartbeat
        - end (terminal) / Error (terminal)

        RETRY: if an attempt produces ZERO Content/ToolCall events AND the
        failure is a WS gap timeout (no event for 240s), retry up to 2 times
        with exponential backoff (5s, then 15s). This is the signature of a
        transient LLM backend stall (DeepSeek/OpenAI rate-limit windows,
        network blips) — verified non-pathological: the same case passes in
        7-9s on a clean retry. Real bugs produce at least one Thinking/Content
        event before stalling. Up to 3 total attempts; each stalled attempt
        can take up to `timeout` (900s) so a genuinely-down endpoint still
        terminates within ~3×timeout + backoff.
        """
        backoffs = [5.0, 15.0]  # sleeps BEFORE retry attempt #2 and #3
        retry_count = 0
        history_before = self._snapshot_history_len(session_id)
        result = _ws_chat(
            host="127.0.0.1",
            port=self.port,
            api_key=self.api_key,
            session_id=session_id,
            message=message,
            timeout=timeout,
            history_before=history_before,
            history_fetch=lambda sid: self.get_history(sid),
            images=images,
        )
        while _is_transient_stall(result) and retry_count < len(backoffs):
            retry_count += 1
            sys_stderr_write(
                f"[chat] transient stall detected (0 events + gap timeout); "
                f"retry #{retry_count}/{len(backoffs)} after {backoffs[retry_count-1]:.0f}s backoff\n"
            )
            time.sleep(backoffs[retry_count - 1])
            # Re-snapshot history in case the stalled attempt left a partial
            # assistant message — the retry starts fresh from current state.
            history_before = self._snapshot_history_len(session_id)
            result = _ws_chat(
                host="127.0.0.1",
                port=self.port,
                api_key=self.api_key,
                session_id=session_id,
                message=message,
                timeout=timeout,
                history_before=history_before,
                history_fetch=lambda sid: self.get_history(sid),
            )
        if retry_count:
            result["retried_after_transient_stall"] = True
            result["transient_stall_retry_count"] = retry_count
        return result

    def _snapshot_history_len(self, session_id: str) -> int:
        try:
            return len(self.get_history(session_id))
        except Exception:
            return 0


def sys_stderr_write(s: str):
    import sys
    print(s, file=sys.stderr, flush=True)


def _is_transient_stall(result: dict) -> bool:
    """Detect a transient LLM stall worth retrying.

    Criteria (all must hold):
    - 0 streamed Content chunks AND 0 ToolCall events
    - error message indicates a WS recv gap timeout (not an Error event
      from the server, not a connection close)
    - response text is empty

    A genuine production failure typically emits at least Thinking/Content
    before stalling; a transient rate-limit window or network blip produces
    zero events because the LLM HTTP call itself never returns.
    """
    if not result.get("error"):
        return False
    err = result["error"]
    # Transient LLM-call failures worth retrying:
    #   1. WS recv gap timeout (no event for 240s) — the classic transient
    #      stall (rate-limit window / network blip before the LLM call returns).
    #   2. "Network error: error sending request" — the LLM HTTP request itself
    #      failed to send (connection refused/reset to the backend). Both
    #      produce ZERO streamed events with the LLM never really running, so
    #      both deserve a retry. Without this, a burst of backend connection
    #      failures silently records empty turns (suspected_fallback) and
    #      inflates the failure count for whatever model is under test.
    if ("WS recv gap timeout" not in err) and ("Network error: error sending request" not in err):
        return False
    if result.get("response"):
        return False
    if result.get("tool_calls_stream"):
        return False
    return True


# ---------------------------------------------------------------------------
# WebSocket chat helper
# ---------------------------------------------------------------------------

def _ws_chat(
    host: str,
    port: int,
    api_key: str,
    session_id: str,
    message: str,
    timeout: float,
    history_before: int,
    history_fetch,
    images: list[str] | None = None,
) -> dict:
    """Synchronous wrapper around the async WS chat. Runs an event loop just
    for the duration of one chat call."""
    return asyncio.run(
        _ws_chat_async(
            host=host,
            port=port,
            api_key=api_key,
            session_id=session_id,
            message=message,
            timeout=timeout,
            history_before=history_before,
            history_fetch=history_fetch,
            images=images,
        )
    )


async def _ws_chat_async(
    host: str,
    port: int,
    api_key: str,
    session_id: str,
    message: str,
    timeout: float,
    history_before: int,
    history_fetch,
    images: list[str] | None = None,
) -> dict:
    """Connect to /api/chat, send the message, drain events until terminal."""
    import websockets

    url = f"ws://{host}:{port}/api/chat?api_key={api_key}"
    t0 = time.monotonic()

    # Accumulators
    content_chunks: list[str] = []
    thinking_chunks: list[str] = []
    # tool_calls_stream: list of {name, arguments, result, round, success, tool_call_id}
    tool_calls_stream: list[dict] = []
    pending_tool_calls: dict[int, dict] = {}  # round → in-flight ToolCallStart info
    error_msg: str | None = None
    got_end = False  # terminal "end" event received (successful completion)
    server_msg_buffer: list[str] = []  # all raw event JSON, for forensic use

    try:
        async with websockets.connect(
            url,
            open_timeout=30,
            close_timeout=5,
            max_size=None,  # allow large tool results
        ) as ws:
            # Drain welcome + optional history_complete before sending our msg.
            # Welcome = {"type":"system","content":"Connected..."}.
            # If sessionId was passed as query param, history events arrive too;
            # we don't pass sessionId in the URL so we just expect the welcome.
            welcome_deadline = time.monotonic() + 10.0
            while time.monotonic() < welcome_deadline:
                try:
                    raw = await asyncio.wait_for(ws.recv(), timeout=2.0)
                except asyncio.TimeoutError:
                    # No welcome within 2s — proceed to send anyway.
                    break
                try:
                    evt = json.loads(raw)
                except Exception:
                    continue
                if evt.get("type") == "system":
                    break  # welcome received

            # Send the chat message.
            payload_obj = {"message": message, "sessionId": session_id}
            # Multimodal: optional images (data URLs) — ChatImage{data, mimeType?}.
            if images:
                payload_obj["images"] = [{"data": u} for u in images]
            payload = json.dumps(payload_obj)
            await ws.send(payload)

            # Drain events until terminal.
            # Inner gap timeout must exceed the server's Heartbeat interval
            # (HEARTBEAT_INTERVAL_SECS = 30s in sessions.rs) so a thinking
            # pause between tool rounds doesn't kill a long run. Outer
            # `timeout` (900s) bounds total wall clock for very multi-tool
            # runs. Inner gap raised to 240s (8x heartbeat) for thinking
            # models on slow endpoints, then to 600s (2026-08-14): the chat
            # path's own turn bound is max_stream_duration=1200s and long
            # single tool executions (extension build/install, async agent
            # exec waits) are legitimately silent >240s on local models —
            # the stream_core heartbeat only fires between stream chunks,
            # so a blocking tool phase emits nothing. 240s was killing
            # turns the agent would have completed (~13% of 2.6B full-eval
            # cases died with an empty-error gap timeout). 600s stays under
            # the 900s outer deadline so a truly-hung attempt still
            # terminates; the agent's own 1200s turn bound is the real
            # completion mechanism.
            deadline = time.monotonic() + timeout
            inner_gap = 600.0
            while time.monotonic() < deadline:
                remaining = max(0.0, deadline - time.monotonic())
                try:
                    raw = await asyncio.wait_for(ws.recv(), timeout=min(inner_gap, remaining))
                except asyncio.TimeoutError:
                    error_msg = f"WS recv gap timeout (no event for {inner_gap:.0f}s; total elapsed {time.monotonic() - t0:.0f}s)"
                    break
                except websockets.ConnectionClosed as e:
                    error_msg = f"WS connection closed: {e}"
                    break

                server_msg_buffer.append(raw if isinstance(raw, str) else raw.decode("utf-8", errors="replace"))
                try:
                    evt = json.loads(server_msg_buffer[-1])
                except Exception:
                    continue

                etype = evt.get("type")
                if etype == "Content":
                    content_chunks.append(evt.get("content", ""))
                elif etype == "Thinking":
                    thinking_chunks.append(evt.get("content", ""))
                elif etype == "ToolCallStart":
                    rnd = evt.get("round")
                    # round may be missing on the start event; key by len+name for safety.
                    # But prefer round when present since multi-round may call same tool.
                    key = rnd if rnd is not None else len(tool_calls_stream)
                    pending_tool_calls[key] = {
                        "name": evt.get("tool", "?"),
                        "arguments": evt.get("arguments"),
                        "round": rnd,
                    }
                elif etype == "ToolCallEnd":
                    rnd = evt.get("round")
                    key = rnd if rnd is not None else len(tool_calls_stream)
                    start_info = pending_tool_calls.pop(
                        key,
                        {"name": evt.get("tool", "?"), "arguments": None, "round": rnd},
                    )
                    tool_calls_stream.append({
                        "name": start_info.get("name") or evt.get("tool", "?"),
                        "arguments": start_info.get("arguments"),
                        "result": evt.get("result"),
                        "round": rnd,
                        "success": evt.get("success"),
                        "tool_call_id": None,
                    })
                elif etype == "Error":
                    error_msg = evt.get("message", "unknown error")
                    break
                elif etype == "end":
                    got_end = True
                    break
                elif etype in ("intermediate_end", "Intent", "Plan", "Progress",
                               "Warning", "Heartbeat", "system", "session_created",
                               "session_switched", "history", "history_complete",
                               "cancelled"):
                    # Informational / control — ignore for accumulation.
                    pass
                else:
                    # Unknown event — already logged via server_msg_buffer.
                    pass
    except Exception as e:
        error_msg = f"WS chat failed: {e}"

    # Deadline-exit observability: the drain loop can exit because the outer
    # deadline passed (while-condition false) WITHOUT any error path setting
    # error_msg — that left a None/empty error that surfaced upstream as
    # "turn failed (): " with no diagnosis (2026-08-14: 3 extension cases).
    # Distinguish it explicitly so the failure is diagnosable.
    if error_msg is None and not got_end and time.monotonic() >= deadline:
        error_msg = (
            f"chat deadline exceeded ({timeout:.0f}s) — the agent turn did not "
            f"complete within the outer bound (its own max_stream_duration is 1200s)"
        )

    elapsed_ms = int((time.monotonic() - t0) * 1000)

    # Pull session history delta for raw_messages + final assistant_message.
    # The streamed Content chunks are the assistant's final text, but the
    # session-persisted version is the source of truth (especially if the
    # agent did multi-round tool calling — the persisted message consolidates
    # round_contents etc.). Prefer history; fall back to streamed chunks.
    new_messages: list = []
    final_response = "".join(content_chunks)
    try:
        history = history_fetch(session_id)
        new_messages = history[history_before:] if history_before <= len(history) else history
        # Find the last assistant message — that's the final user-facing reply.
        for m in reversed(new_messages):
            if m.get("role") == "assistant" and m.get("content"):
                final_response = m.get("content", final_response)
                break
    except Exception:
        pass

    # Build tools_used (names) for parity with the old HTTP return shape.
    tools_used = [tc["name"] for tc in tool_calls_stream if tc.get("name")]

    return {
        "response": final_response,
        "tools_used": tools_used,
        "processing_time_ms": elapsed_ms,
        "new_messages": new_messages,
        # New: live-streamed tool calls with args + results.
        "tool_calls_stream": tool_calls_stream,
        # New: full thinking chain across rounds.
        "thinking_stream": "".join(thinking_chunks),
        "error": error_msg,
        "server_events": server_msg_buffer,
    }

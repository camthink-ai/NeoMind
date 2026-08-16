# Changelog

All notable changes to NeoMind will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Chat/session streaming hardening
- **One active stream per session**: a second concurrent stream on the same session silently overwrote the first's cancel sender (making it uncancellable) and interleaved history writes into the same session state. A second stream now gets a clear rejection instead.
- **Fixed a latent permanent deadlock** in `remove_subscriber` (re-acquiring a non-reentrant write lock through the `if let` scrutinee's guard) — would have wedged the subscriber map globally the moment the subscriber feature shipped.
- **Tool-call detection recovers past leading data arrays**: the detector anchored on the first `[` forever, so any innocuous JSON array before the real tool call (e.g. `trend: [1,2,3]`) made the call stream as visible text and never execute.
- **Tool-call parsing is string-aware**: `]`/`}` inside string arguments (shell globs like `ls foo[1].txt`, regexes) broke bracket matching and silently swallowed the extracted call.
- **Stored tool results are base64-sanitized** (previously only the display copy): 4–64KB data URLs stopped flowing verbatim into every subsequent LLM round and into session storage.
- **Background summarization no longer mutates the global thinking flag** (per-call override; was: user turns could silently run with thinking off, user toggles clobbered, aborts left thinking disabled).

### Correctness & Safety (fresh subsystem sweep)
- **Chat no longer sends the user message twice per turn**: the text path pushed the current user message into history before streaming AND the LLM layer appended it again — every prompt carried `[…, user(current), user(current)]` (double tokens, back-to-back duplicate user turns). The multimodal path never had this.
- **Rule `TriggerAgent` no longer blocks all rule processing**: the action awaited the full agent run inline, so one rule with an agent action stalled every other rule's evaluation platform-wide for up to the 5-minute cap. The callback now spawns.
- **Multimodal chat is cancellable** (image chats' Stop button was dead — zero interrupt checks in the multimodal stream).
- **MQTT push targets fail honestly**: `send()` discarded the eventloop poll result — deliveries to an unreachable broker were logged Success and never retried, and `client.publish()` awaited channel capacity unboundedly (dead broker → target task wedged → `stop()`/update/delete hung the API). Publish is timeout-bounded and poll errors propagate; all final-flush teardown sites are capped at 30s.
- **`POST /api/automations/transforms/process` is side-effect free** (it published test output to the live bus, which could fire REAL rules from test data; `/test` already behaved correctly).
- **Transform template engine**: `render_template` could loop forever on self-referential device data (`{"a": "{{a}}"}`) — the scan cursor now advances past each replacement plus an iteration cap. The engine's fetch clients (used for device-URL → base64) gained 30s/10s timeouts and a 10MB body cap (was: no timeout, unbounded body, SSRF surface).
- **Automation executions**: the last unbounded-growth table now has 30-day retention; `delete_automation` no longer orphans its execution rows; execution history returns the most recent records instead of a random sample over randomly-ordered keys.
- **JS transform identifiers sanitized**: extension ids containing a dot (the docs' own `weather.ext` example) produced a JS syntax error that failed the whole transform.
- **Data-push virtual-metric dedup key includes source_id** (two distinct metrics publishing the same value in the same second no longer collide).
- **Cron templates are i18n'd** (11 hardcoded Chinese labels showed to English users); dead Chinese-only `getStatusLabel` removed.

### Reliability & Performance
- **Event-triggered executions are cancellable at shutdown**: they spawned fully detached (the scheduler's `stop()` only aborts scheduled tasks), so event agents kept running through shutdown bounded only by their execution timeout. The executor now registers spawned handles and shutdown aborts them.
- **Multimodal chat gets the tool-execution heartbeat** (same `select!` fix `stream_core` received): long tool phases no longer look like a dead stream to WS listeners.
- **The three hottest storage paths no longer block tokio workers**: `write_batch` (every device metric), `query_range` (every dashboard read) and `query_agents` (full scan + sort per call) were `async fn` bodies doing blocking redb I/O with zero awaits — each now runs on the blocking pool via `spawn_blocking` with an `Arc<Database>` clone.

### Platform trust
- **`data/encryption_key` is now written 0600** (was 0644 via `std::fs::write`, the lone world-readable straggler among secrets — this key encrypts every API key and unlocks the default admin key via auto_auth). Existing 0644 files are chmod-hardened on rewrite.
- **JWT-secret persistence failures are logged** (were `let _ =`-swallowed; a failed write silently meant session-invalidating secret rotation on the next restart with no trace).
- Removed the verified-dead `create_capability_services`/`init_capability_providers`.
- **Logins survive server restarts**: the session-revocation allowlist was an in-memory map rebuilt empty on every boot, so `validate_token` rejected every pre-restart token (`SessionRevoked`) — defeating the persisted JWT secret and logging everyone out on every restart. Sessions now persist to a `user_sessions` table in `users.redb` (write-through on login, delete-through on logout; keys are SHA-256(token) so raw tokens are never written at rest; boot load drops and purges expired rows). Logout stays revoked across restarts.
- **`/health/ready` tells the truth**: every dependency was hardcoded `true` and `all_ready()` used `||`. Now: `database` = a real redb open+read; `llm` = an active backend configured; `mqtt` = the embedded broker actually running (absent → `false` plus an explanatory note covering external-broker mode vs failed-to-start); `ready = database && llm`, with `notes[]` explaining every unready gate. A full MQTT outage (stale process squatting port 1883) previously stayed invisible to readiness.
- **Storage `NotFound` maps to 404** (was 500 for every storage variant): the blanket `From<storage::Error>` now maps NotFound → 404 and InvalidInput → 400, fixing paths that returned INTERNAL_ERROR for a plain missing resource.
- **Extension handlers use the shared `ExtensionStore`**: 19 call sites in the extension handlers opened `ExtensionStore::open("data/extensions.redb")` per request, ~15 of them via `if let Ok(store) = …` — an open failure (corruption / disk full / perms) silently degraded to empty lists or no-op writes with no error anywhere. All sites now use the pre-opened shared `state.extensions.store` (the handle `ExtensionState` has held all along); this also removes the per-request redb open and makes read-modify-write sequences consistent on one handle.

### Docs
- **Edge model deployment guide** (`docs/edge-models.md`): the measured LFM2.5 dual-model recipe — 2.6B (text) as the active agent backend + VL-3B as a non-active perception backend, with the load-bearing llama.cpp flags (`--jinja` for LFM function calling, `--repeat-penalty 1.0`, 128k context), the vision tool's automatic dedicated-VLM preference, and the `lfm1.0` licensing constraint.

### Agent (small-model friendliness + reliability)
- **Concise `shell` tool description + on-demand command guidance**: the shell description had grown to 6510 chars of per-domain "Command Choice" rules, which suppressed tool *selection* on models at/below the 3B tool-calling floor (they avoided the huge description and grabbed the shorter `skill` tool instead — LFM2.5-VL-3B scored 0% cmd_ok purely from this). The description is now a ~1400-char skeleton; exact subcommand syntax is delivered contextually instead: on a FAILED `neomind <domain> …` dispatch the domain's `--help` subcommand table is appended to the tool result (+8pp cmd_ok on the 30-case regression), and on the FIRST successful dispatch per domain it is appended as a reference for multi-step flows (deterministic, fires only after the model already chose `shell` — no intent-detection overtrigger). The "COMPLETE THE FULL FLOW" directive (multi-step requests need every step) is restored in the skeleton.
- **`web_fetch` boundary clarified**: external web content (docs, reference, search-result URLs) vs NeoMind platform data which must use `shell` (was grabbed as a wrong tool in 9/118 eval cases).
- **Heartbeat during tool execution**: the keep-alive heartbeat only fired between stream chunks, so a long single tool execution (extension build/install, async agent-exec waits) emitted no events — WS listeners killed turns the agent would have completed (~13% of LFM2.5-2.6B full-eval cases died this way with an empty error string). The tool batch is now wrapped in a `select!` with an independent 10s heartbeat timer.
- **Chat stream bound 1200s → 2400s** (`StreamConfig` default + the synced chat safeguards): slow local models (60–90 tok/s) legitimately need 20+ minutes for multi-round deploy scenarios.

### Fixes
- **Log export no longer ships ANSI color codes**: the CLI and desktop file layers wrote `tracing` SGR escapes (`ESC[2m` / `ESC[32m` / …) into every line of the daily `neomind.log.*` files because `fmt::layer()` defaults `with_ansi` to true (it does no TTY detection, unlike `fmt()`). Both appenders now set `.with_ansi(false)`, and `/api/logs/download` strips residual ANSI sequences from archived files so logs produced by older server builds export as readable plain text too.

### Eval / Test
- **Time budgets retuned for slow local models**: four eval-side/agent-side limits were each tuned for cloud endpoints and collectively killed every legitimately-slow local run (a 20-round deploy case on a 60–90 tok/s model needs 10–20+ min): WS event gap 240s → 600s; chat outer timeout 900s → env-tunable (`EVAL_CHAT_TIMEOUT`, default 1400); per-case `--case-timeout` for heavyweight cases; deadline-exit now sets a real error instead of propagating `None`.
- **Turn-failure messages include the exception class**: bare `asyncio.TimeoutError()`-style exceptions `str()` to an empty string, which left `turn failed (…): ` with no diagnosis; the class name identified the hidden per-case SIGALRM limit in one shot.

## [0.9.16] - 2026-08-11

LLM capability detection consolidated to a single track (registry + name heuristic) + unified thinking-effort control + slim prompt for small local models + built-in local AI (Docker llama.cpp).
Focus: collapse the duplicated capability detection (manual table + CapabilityDetector + audio pipeline) onto the LiteLLM registry as the single source of truth, unify reasoning/effort control across backends, and ship a slim prompt + built-in local AI for edge deployment.

### LLM Capability Detection — Single Track (registry + heuristic)
- **Removed** the hand-curated manual table (`models.rs`, 72 entries / 865 lines) and `CapabilityDetector` with its heuristic suite (~1300 lines net). Vision / reasoning / max-context now come solely from the LiteLLM registry (2988 entries); audio removed entirely (was informational only — the agent pipeline never emitted audio content parts).
- **Unified `supports_tools`**: the 4 copies of the "exclude tiny" name heuristic (`llm_backends.rs` ×3 + `ollama.rs`) collapsed into one `detect_tools_capability` — registry `supports_function_calling` first (a field that previously sat unread), name fallback for local/Ollama models.
- `detect_vision_capability` / `detect_thinking` now call the registry directly (no `CapabilityDetector` wrapper).
- Storage `BackendCapabilities.supports_audio` dropped (`#[serde(default)]` — legacy redb rows deserialize cleanly, no migration).
- Fixed flaky `test_config_env_var_parsing_*` (env-var tests serialized via `ENV_LOCK` mutex — parallel pollution surfaced after the removal shifted test scheduling).

### Thinking-Effort Control (unified across backends)
- Single `ThinkingEffort` enum (none / low / medium / high / xhigh / max) in `GenerationParams`, supersedes the legacy `thinking_enabled` bool.
- Per-backend translation: Ollama (`think` level), OpenAI / Custom / GLM / Google (`reasoning_effort`), DeepSeek / Anthropic (`thinking` boolean), Qwen (`enable_thinking`), llama.cpp (read-only `reasoning_content`).
- `ReasoningCapabilities` declaration drives the frontend dropdown — read-only backends show a badge, level/effort backends show the full selector.
- Frontend: capability panel redesigned as a spec-row layout (was `FormField` rows, read as a "weird half-form"); multimodal switch now syncs to the selected model when editing an existing backend.

### Slim Prompt + Tool Descriptions (for small local models)
- Slim system prompt is now the default (`NEOMIND_FULL_PROMPT=1` opts back into the verbose one); adds Device Onboarding guidance.
- Tool descriptions slimmed (image_edit / memory / skill / file_write / file_edit); shell tool gained command-choice disambiguation hints (device read-vs-write, set-vs-check, enable-activate) and sequence directives (read the entity before a write, complete every step of a multi-step flow).
- ~45% prompt token reduction with no eval regression (verified across the 154-case bilingual sweep using a stable-case yardstick).

### Built-in Local AI (Docker)
- Docker image now ships llama.cpp + Gemma4-E2B and auto-registers an Ollama-style backend on first boot.
- Python3 added to the image (agents + python-sidecar extensions).

### Agent Reliability
- **Local-backend stream idle timeout**: the openai/anthropic streaming read-idle timeout (0.9.15) is extended to llama.cpp and Ollama. A stalled upstream SSE connection (bytes stop, socket open — e.g. llama-server mid-thinking-loop) now force-completes the round instead of blocking `bytes_stream().next()` forever and hanging the turn. Shared `next_bytes_or_end` helper mirrors the agent-layer `next_chunk_or_timeout`.

### Eval / Test
- 23 en capability-dimension cases + 20 en scenario cases (full bilingual parity with the zh set).
- Fixed: data-push `targets` collection key in state_query, `latest_telemetry` None crash, telemetry-history expectation, widget-full-lifecycle negative sq.
- **Regression gate** (`run_eval.py regression`): curated 30-case stable set vs a committed baseline, flagging only confirmed PASS→FAIL regressions; multi-round verdicts absorb the ~7pp run-to-run noise floor; per-case SIGALRM timeout so a wedged agent can't hang the gate.
- **LLM endpoint pre-flight**: `run`/`regression` probe the agent's LLM backend before the first case — dead server / wrong port / doubly-pathed `/v1` / non-JSON proxy fails fast with a clear message instead of reading as a model-capability regression.

### Web
- **Chat UI overhaul**: code blocks gain syntax highlighting (rehype-highlight) + copy button + language label (borderless immersed block, slim header); whole-message copy button (desktop hover / mobile always); scroll-to-bottom button; both chat surfaces unified (Sparkles avatar, `--msg-user-bg` token, fade-in-up); body 14px desktop (13px mobile); heading hierarchy + table booktabs + blockquote background + muted list markers; `--background` 0.985 + `--syn-*` dual-theme syntax tokens; Brain thinking icon; tool expand as full-row hover; `web/docs/MARKDOWN_STYLE.md` spec.
- PWA status-bar color syncs with full-screen dialogs; chat model-select scrollbar hidden; CSS keyframe dedupe + prose consolidation; mostly-hidden aurora background removed.

---

## [0.9.15] - 2026-08-04

Agent streaming reliability (stall hang fixes) + skill matching (description-based) + system-level test layer + eval coverage.
Focus: fix a class of agent stream hangs that could freeze chat/eval for hours, make skill discovery semantic, and add a deterministic real-server test layer.

### Agent Streaming Reliability (hang fixes — three layers)
- **Stream consumer timeout**: `stream_core`/`stream_multimodal` LLM `next()` loops now bound each `next()` with `tokio::time::timeout` (`next_chunk_or_timeout`). A zero-chunk upstream stall force-completes the round instead of hanging forever.
- **Backend read idle timeout**: openai/anthropic streaming `bytes_stream().next()` bounded by a 60s chunk-interval idle timeout — the upstream HTTP read itself can no longer block indefinitely.
- **Backend header-wait bound**: streaming `send()` (waiting for response headers) bounded by a 30s header timeout via `tokio::time` (NOT reqwest's whole-request `.timeout()`, which would kill long healthy generations).
- **Hold-back fragment fix**: the no-tool-call round-end now uses the full `buffer`, not a `content_before_tools` fragment left by the JSON-hold-back heuristic (was corrupting responses to `":`).
- **eval harness**: LLM "Network error: error sending request" now treated as a transient stall and retried (was silently recording empty turns as failures).

### Skill Matching (description-based, agentskills.io)
- 15 builtin skills now carry an intent-based `description`; `SkillMetadata` + parser support it (1024-char cap).
- Matcher scores description intent phrases (quoted synonyms + `Includes`/`e.g.` clauses) — semantically-equivalent phrasing ("turn off the pump", "把泵停掉") triggers without a literal keyword.
- On-demand `skill` tool search is description-driven and surfaces the frontmatter description (was a body-first-line slice). Device-control trigger coverage expanded; preserve user-specified device ID on create.

### System-Level Test Layer (`eval/system/`)
- New deterministic, LLM-free test layer driving a real `neomind serve`: real MQTT device → telemetry store → live WS event; rule-on-real-mqtt; downlink command delivery; dashboard live binding; offline detection; data-push outbound delivery (local HTTP receiver verifies the payload actually arrives).
- `TestServer.wait_for_log()` — deterministic adapter/broker readiness gate.

### Eval Coverage
- Assertion types now include `latest_telemetry` (agent read-back), `push_enabled` (with name fallback), `response_contains` (hard signal for answer content / cross-turn recall), and per-turn `turn_index` assertions.
- New `device-control` probe cases (zh+en), `deep-memory` 10-turn case exposing small-model recall decay (~8-turn limit on Gemma4-E2B).

### Device/Web
- Seed built-in device templates via the registry's own storage handle (fixes intermittent "template not found" boot race).
- Settings dialog mobile header bar aligned to MobilePageHeader.

---

## [0.9.14] - 2026-07-31

IM bridge system (Telegram + Feishu) + OTA release notes + extension log UX.
Focus: first release of two-way IM (chat-platform) integration — invite-gated
access, per-platform bridges, persistence, and real-bot debugging lessons.

### IM Bridges (new subsystem)
- **Two-way IM integration**: NeoMind agents reply on chat platforms.
  Inbound message → ImRouter → agent → reply. Architecture mirrors openclaw
  (Gateway + channel adapter); IM is positioned as a lightweight conversation
  entry + output channel (web chat remains core).
- **Telegram bridge**: getUpdates long-polling (35s client > 30s window) +
  sendMessage (4000-char chunking) + getMe → bot username → deep-link QR.
- **Feishu (飞书) bridge**: hand-written pbbp2 protobuf frame codec (field
  numbers from larksuite/node-sdk, byte-level verified) + WebSocket
  long-connection (endpoint/ping/shard-merge/per-frame ack/reconnect) + REST
  send-message (tenant_access_token single-flight cache). No official Rust
  SDK → ported from Node SDK (MIT). No deep-link → invite via `/start <token>`.
- **Invite-gated access**: admin generates invite → user `/start <token>` →
  atomic consume → allowlist. Bridge boots invite-gated (rejects all until
  bound). `/start` runs before allowlist + dedup.
- **Bridge persistence + restart reload**: `im_bridges` redb table; create/
  delete sync; `start_im_router` reloads persisted bridges on restart (was
  in-memory only).
- **IM sessions**: per `(platform, chat_id)` session mapping (redb) + expiry
  cleanup; Messages → IM Sessions tab.

### IM UX
- **Settings → IM channels**: all-platform card list (Telegram + Feishu) with
  configured/not-configured status; bridge CRUD + invite management (QR for
  Telegram, `/start <token>` hint for Feishu) + allowlist.
- **Messages → IM Sessions**: session table (chat_id / agent / last-active) +
  reset; "Configure IM bridge" CTA when no bridge.
- Per-platform icons (Settings gear for manage; platform-specific tint).

### IM Reliability (real-bot debugging lessons)
- Removed fixed Chinese "🤔 思考中…" ack (inappropriate for multilingual;
  replies go straight to result).
- 10-min agent timeout + English error/timeout reply (was silent hang on
  LLM stall, e.g. thinking-model runaways).
- Telegram: api_base normalize + reply() tracing.
- Feishu: WS endpoint path, domain normalize, REST /open-apis/ prefix,
  event/reply tracing.

### OTA Updates
- **Update notes as markdown**: OTA update dialog renders release notes as
  markdown (was plain text).

### CI
- **CHANGELOG as release notes**: GitHub release / Discord / OTA now surface
  CHANGELOG.md sections. Short mode: clean headlines, no truncation.

### Extensions
- **Stderr capture refactor**: extracted `capture_stderr_loop` (testable) +
  non-UTF8 byte survival test. Extension details dialog log auto-scroll
  sticks to bottom only when already there (scrolling up to read history
  isn't yanked back down on every 3s poll).

## [0.9.13] - 2026-07-30

Skills overhaul + user-configurable system preferences + About resource panel.
Focus: make small/edge models more reliable at tool use, and expose
previously-hardcoded operational knobs as user-facing settings.

### Skills System
- **4 new built-in skills**: `settings-management`, `system-info`,
  `extension-management`, `widget-management`. Previously these CLI domains
  had no skill (settings/system) or were buried in 500-line dev guides
  (extension/widget). Each has a concise command cheat-sheet at the top.
- **Unified command cheat-sheets** across all 13 management skills — every
  CLI domain now has a top-of-file table with exact subcommands, so small
  models see the precise command first instead of prose.
- Eval impact: settings 0% → 66%; tool-selection wrong-cmd failures reduced
  across device/rule/agent/message/push/transform/dashboard/connector/llm.

### Agent Loop
- **Hallucinated tool-name redirect** (chat path): when a weak model emits
  a full `neomind <command>` as the tool name (e.g. "neomind device list"),
  the chat path now appends a "use `shell(command=...)`" hint so the model
  recovers next round instead of looping. Guidance only — no early-stop.

### System Preferences (new)
- **Agent defaults** (`GET/PUT /api/settings/agent`): `max_rounds` (was
  const 30), `execution_timeout_secs` (was 300), `tool_concurrency` (was 6),
  `default_temperature` (was 0.3), `default_top_p` (was 0.7),
  `default_thinking_enabled`. All configurable via Preferences UI with
  range-clamped Selects. Priority: env > UI > hardcoded.
- **Device defaults** (`GET/PUT /api/settings/device`):
  `default_offline_timeout_secs` (was 300, now **live** via
  `effective_offline_timeout`) and `auto_onboard_enabled` (was true,
  applies on restart).

### About Resource Panel
- **Live system metrics** in the About tab: CPU usage gauge + memory gauge
  + per-disk usage gauges (deduped macOS APFS duplicates) + network
  interfaces (IPv4, loopback/virtual filtered) with rx/tx bytes.
- Polls `/api/stats/system` every 5s (aligned with backend cache).
- Threshold colors use `info` (blue) mid-tier instead of `warning` (orange).
- Reusable `UsageGauge` component (parameterized icon/label/sub/right).

### Backend Stats Expansion
- `/api/stats/system` now returns `cpu_usage` (sysinfo 2-sample),
  `disks` (per-disk total/used/available), and `networks` (per-interface
  name/IP/MAC/rx/tx). Flattened to top-level `data.*` for frontend access.

### UI Fixes
- **Portal dialog click bubbling**: UnifiedFormDialog now stops click
  propagation on portal content + overlay, preventing dialog taps (✕, Close)
  from reaching ancestor handlers like table row onRowClick.
- Card style consistency: Agent + Device preference cards match the standard
  pattern (icon, space-y-4, SelectTrigger width).
- About tab: removed redundant instance-manager entry (duplicated TopNav's
  InstanceSelector).

---

## [0.9.12] - 2026-07-27

Agent hardening + security fixes + data integrity + test infrastructure.
50+ commits across all subsystems, informed by a systematic audit of
NeoMind against mainstream agent frameworks (OpenHands, LangGraph,
smolagents, Letta, Mem0, OpenClaw, Hermes) and a full codebase scan
(devices, rules, core, API, storage, frontend).

### Agent Loop
- **StopReason enum** — every loop exit now carries a typed reason
  (NaturalCompletion / MaxRounds / Stuck / AllDuplicate / LlmError /
  Cancelled). The journal records it so the agent learns why it stopped.
- **StuckDetector** (OpenHands-inspired, 5 patterns) — catches repeated
  action+observation, repeated errors, A-B-A-B ping-pong, monologue loops,
  and context-overflow loops. Wired into both Loop A (scheduled) and Loop B
  (chat).
- **Graceful exit** — removed the `max_rounds += 10` continuation hack;
  the post-loop Phase 2 summary now synthesizes a final answer at the real
  round cap.
- **AllDuplicate break** — when all tool calls are cross-round duplicates
  and results already exist, breaks to Phase 2 instead of burning rounds.
- **MockLlmRuntime** (`test-utils` feature) — scripted per-call responses,
  call recording, usable from integration tests. Closes the gap that forced
  pure-logic-only testing.
- **Behavior tests** — deterministic end-to-end loop tests (natural
  completion, AllDuplicate, max-rounds graceful exit) without a real LLM.

### Security
- **Logout actually invalidates sessions** — was a no-op (returned 200 but
  left the token valid for 7 days). Now extracts the bearer token from the
  Authorization header and calls `logout()`.
- **Password changes + user deletions persist to redb** — were in-memory
  only; silently reverted on restart. Deleted admins resurrected from disk.
- **JWT secret persisted** — was regenerated on every restart (all users
  logged out). Now: env var > file (`data/.jwt_secret`) > generate + persist.
- **Extension manifest id path-traversal** — `id: "../../etc/cron.d/x"`
  could write outside the install dir. Now rejects `..`, `/`, `\`, NUL.
- **Extension zip-bomb defense** — caps per-file (200MB), total (500MB),
  and file count (10K) on `.nep` extraction.
- **Shell policy deny-list** (both loops) — `rm -rf /`, `dd of=/dev/`,
  `mkfs`, `curl|sh`, fork bomb, destructive `neomind` CLI commands blocked
  before execution. Code-enforced (not just prompt advisory).
- **SSRF protection** — instance test endpoint blocks loopback / cloud
  metadata IPs.

### Data Integrity
- **Transform non-numeric outputs** — string values (OCR text, labels,
  decoded payloads) were hashed to a bogus float (`chars().sum() % 10000`)
  and stored as `MetricValue::Float`, indistinguishable from real readings.
  Now stored as `MetricValue::String`.
- **Stubbed Pipeline/Fork/If** — returned `Ok(0.0)` placeholder metrics,
  silently reporting success. Now return `AutomationError::TransformError`.
- **Rules `condition_since` persisted** — `for_duration` elapsed accumulation
  was lost on restart, causing premature/missed triggering. Now persists on
  every state transition.
- **Rules cooldown refund** — if ALL actions fail, the cooldown is refunded
  so the rule retries on the next match instead of waiting the full window.

### Reliability
- **EventBus HOL blocking fixed** — extension event dispatch used
  `sender.send().await` sequentially; one slow subscriber stalled all.
  Now `try_send` (non-blocking, drops on full with warn).
- **block_in_place panic on Tauri** — extension registration used
  `block_in_place(Handle::current().block_on(...))` which panics on
  current-thread runtimes. Now `tokio::spawn` (fire-and-forget).
- **MemorySnapshot mutable** — agent memory writes were invisible until next
  session (OnceLock frozen). Now re-reads on each user message.
- **Scheduler priority** — agent priority field (0-255) was stored but
  ignored (FIFO scheduling). Now sorted by priority at the concurrency limit.
- **Webhook rate-limit lock** — write lock + full `retain()` scan on every
  request. Now: retain only when map >1000 entries; per-entry staleness
  check for expiry.
- **Device telemetry routing** — `find_device_by_telemetry_topic` was O(N)
  linear scan on every MQTT publish. Now O(1) via `topic_index` DashMap
  with fallback scan (auto-repairs stale entries).
- **metric_cache sweep** — unbounded growth from phantom devices on
  long-running edge boxes. Periodic sweep every 5 min drops entries >30 min.
- **client_id_cache cleanup** — rotating MQTT client_ids leaked forever.
  Now removed on ClientDisconnected.
- **Mutex poison recovery** — `device_status_emitter` used
  `.lock().expect("poisoned")`; now `unwrap_or_else(|e| e.into_inner())`.
- **Sessions reaper** — expired JWT sessions accumulated in the in-memory
  HashMap. Now piggyback `retain()` sweep on each login.
- **Device storage write failures** — `let _ = storage.save_device(...)`
  silently swallowed errors. Now logs at `error!` level.

### Performance
- **Regex cached** — `ComparisonOperator::evaluate_str` recompiled regex on
  every condition evaluation. Now cached via `OnceLock<RwLock<HashMap>>`.
- **Dashboard `.shallow`** — two large object selectors in VisualDashboard
  were missing the `shallow` equality fn, causing re-renders on every store
  mutation.
- **Prompt cache-friendly** — `{{CURRENT_TIME}}` moved from prompt top to
  end, so the entire stable prefix is reusable by prefix-caching backends.

### Frontend
- **Stale API_BASE** — `useExtensionLifecycle` captured `getApiBase()` at
  module load; remote-instance switch still hit the old backend. Now dynamic.
- **WS reconnect jitter** — pure `2^n` backoff → multi-tab thundering herd.
  Now `× (0.5 + Math.random() × 0.5)`.
- **sessionSlice race** — `loadSessions` didn't set `sessionsLoading: true`
  at start; `loadMoreSessions` could fire concurrently.
- **JWT error handling** — removed aggressive `window.location.reload()`;
  aligned with `events.ts` (disconnect, let 401 interceptor handle redirect).
- **dataPushSlice dedup** — no loading guard → concurrent double-fetches.
- **frontendComponentSlice fetching flag** — `shouldFetch` ignored in-flight
  state → concurrent `fetchInstalled` calls.

### Tool Configuration
- **`allowed_tools` exposed end-to-end** — `AgentToolConfig` existed in
  storage and `filter_tools` honored it, but the API hardcoded `None` and
  the UI didn't expose it → every agent saw all ~12 tools. Now wired
  through create/update handlers + `AgentDetailDto`.
- **`enabled` defaults to true** — was a required field (400 on partial
  payloads); now `#[serde(default = "default_true")]` + honored in
  `filter_tools` (`enabled: false` → no tools).

### Documentation
- **CLAUDE.md gotcha #8 corrected** — read-side truncation was 300+800, not
  "600 total".
- **Stale MemoryScheduler comment removed** — claimed "periodic extraction"
  that doesn't exist.

### Pre-release regression sweep
A full `v0.9.11..HEAD` regression audit (fan-out review + manual verification)
surfaced and fixed eight issues before tagging:
- **BMP/TIFF image decode** — the base64 magic-prefix fast-reject whitelisted
  only JPEG/PNG/GIF/WebP, so BMP/TIFF images were rejected before decode and
  stored as raw base64 (never rendered). Prefix list now matches
  `detect_extension`.
- **Stale telemetry-topic routing** — the `topic_index` reverse-index wasn't
  invalidated when a device's `telemetry_topic` changed, so orphan messages on
  an old topic were routed to a device that no longer subscribed. The read path
  now verifies the device's current topic still matches.
- **Image retention disk leak** — CLI-materialized images named by content hash
  had no parseable timestamp, so retention skipped them forever. Falls back to
  mtime for unparseable filenames.
- **JWT secret file permissions** — persisted `data/.jwt_secret` was world-
  readable (0644); now 0600.
- **delete_user atomicity** — mutated memory before the DB write; on DB failure
  the user vanished from the list only to resurrect on restart. Now persists DB
  first (mirrors `change_password`).
- **Sessions reaper grace** — evicted sessions at `expires_at` with no grace,
  while JWT validation grants 30s skew; a login-triggered reap could revoke a
  still-valid token. Now honors the same skew.
- **Cooldown retry flood** — the all-actions-failed cooldown refund had no
  backoff, so an Execute-only rule on a high-rate stream re-fired on every data
  point. Now refunds only on the first consecutive failure (transient failures
  still retry immediately); repeat failures pace at the cooldown window.
- **Backend-unavailable overlay UX** — the WebSocket gave up permanently after
  ~140s when never connected (false-positive on a slow-booting edge box,
  contradicting the slow-startup patience added earlier) and the overlay
  flashed on every transient error. Now never gives up (keeps slow-polling) and
  surfaces never-connected via a `gaveUp` flag.

---

## [0.9.11] - 2026-07-22

Security hardening + edge (aarch64 / RK3576) readiness + the reliability
tail of 0.9.10's silent-failure audit. The headline is closing the
CRITICAL wasmtime aarch64 sandbox escape — the sandbox every ARM board
we ship to runs untrusted extension code inside.

### Security
- **wasmtime 26 → 36** — fixes RUSTSEC-2026-0096, a CRITICAL (9.0) aarch64
  Cranelift sandbox escape. Crossed 10 major versions but the port was a
  2-line change (drop the redundant `component-model` feature + remove the
  now-deleted `static_memory_maximum_size` knob; the core embed API is
  stable). The official extension marketplace ships zero prebuilt WASM
  extensions (all 26 are native binaries), so the blast radius is limited
  to users who compile their own wasm extensions.
- **boa_engine 0.17 → 0.21** — fixes RUSTSEC-2024-0444 (AsyncGenerator
  DoS in the transform JS engine). The 0.20 Realm refactor needed three
  mechanical adaptations (Context lifetime removed, FunctionObjectBuilder
  takes `&Realm`, `to_json` returns `Option`); 19 transform tests pass.
- **rumqttc 0.24 → 0.25** — unifies rustls 0.22 → 0.23 across the MQTT
  stack (devices were on 0.22, data-push on 0.23). Zero breaking changes.
  webpki 0.102 (4 advisories) remains — rumqttc 0.25.1 pins it directly,
  upstream-blocked; re-checked on each rumqttc release.
- **rustls CryptoProvider installed at startup** — `neomind-api`'s reqwest
  uses a `-no-provider` rustls build, which doesn't auto-select a crypto
  provider. `ring` is now enabled and `CryptoProvider::install_default()`
  runs in `start_server`, fixing a startup panic on TLS-using builds.
- **CI cargo-audit gate** — CI now fails on new advisories so debt can't
  silently accumulate; evaluated-and-deferred ones are `--ignore`d with a
  reason and a re-check trigger (webpki upstream-blocked, protobuf
  output-only / not exploitable).
- **Logout actually revokes the JWT** — `logout` now removes the server
  session and `validate_token` checks the sessions map, so a logged-out
  token is invalid immediately instead of being accepted until expiry.

### Reliability / silent-failure
- **No more force-seeded "Default Ollama"** — fresh installs no longer
  create a `Default Ollama ministral-3:3b` backend that looks configured
  but always fails (the box has no such model). Users add their own
  backend; the active runtime returns "No active LLM backend configured"
  instead of a phantom. Existing installs keep the stale row — delete it
  in the UI.
- **EventBus no longer drops events silently** — a slow subscriber used to
  lag with only a debug log (FilteredReceiver logged nothing at all).
  Receivers now count dropped events, warn on lag, and expose
  `dropped_count()`.
- **Auto-onboard is bounded** — the auto-registered device_id is
  sanitized and the uplink sample is capped (2 MiB), so a malformed or
  huge first payload can't pollute the registry or exhaust memory.

### Edge / aarch64 (RK3576)
- **Concurrency env overrides + target-cpu** — `NEOMIND_MAX_CONCURRENT`,
  `NEOMIND_PER_BACKEND`, and `NEOMIND_TOOL_CONCURRENCY` (all validated
  ≥1) let ops tune the scheduler to a board's cores; `.cargo/config.toml`
  sets `target-cpu=cortex-a72` for aarch64-unknown-linux-gnu builds.
- **install.sh rate-limit fix** — resolving "latest version" via the
  GitHub API hit 403 on shared NAT IPs (60 req/h). Switched to the
  `/releases/latest` 302-redirect endpoint, which is not rate-limited.
- **Docker native arm64 runner** — arm64 images now build on a native
  runner (no QEMU) and are merged into a multi-arch manifest, so they're
  fast and binary-correct.

### Added
- **`neomind upgrade` + `neomind uninstall`** — the CLI can now
  self-manage on Linux/systemd: `upgrade` pulls the latest release binary
  and restarts the unit; `uninstall` removes the binary, unit, and data
  dir.

### Changed
- **Auth-flow UI unified** — login + both setup steps now share a single
  brand-orange honeycomb background (extracted to `HoneycombBackground`),
  a floating top-right language switcher (frees vertical space and stops
  the setup page scrolling when the form is short), and a common card
  style (`backdrop-blur-xl` + `color-mix`, lighter shadow). The honeycomb
  renders invisible on first paint — base opacity lives on the cell class,
  not the keyframes, fixing the FOUC where every cell flashed solid orange
  before the breathe animation started.
- **macOS app icon** — squircle reduced to ~80% of the canvas; macOS does
  not auto-mask Tauri's icns, so the icon must carry its own rounded
  corners (a square fill renders as a flat square, which is worse).
- **Instance switch fail-fast** — a dead backend is detected in <100 ms
  before any localStorage write or reload, lands on an error overlay with
  a single Cancel, and reverts instantly; the login instance picker shows
  live online/offline dots per backend.

## [0.9.10] - 2026-07-21

Agent quality + reliability. After three plumbing-hardening releases
(0.9.7–0.9.9), this one returns to the differentiator — the local AI
agent — fixing silent-degradation paths across the HTTP chat path, the
event-agent runtime, telemetry persistence, and the camera image-analysis
pipeline.

### Fixed
- **HTTP chat is now multi-round** — `POST /api/sessions/:id/chat` called
  the single-round `process_message` (one LLM call, tool results never fed
  back), so any HTTP client silently degraded to a single-round agent. It
  now consumes the same ReAct event stream as WebSocket (tool results fed
  back each round) and aggregates `AgentEvent`s into `ChatResponse` — the
  gap that cost eval 43.5 points purely on transport (HTTP F → WS B).
- **Telemetry no longer lost on flush failure** — `flush_buffer` drained the
  write buffer before writing, so a failed redb write permanently lost those
  points (disk full, IO). Failed batches are now re-queued for the next
  flush, bounded by a hard cap so a persistent failure can't grow memory.
- **Agent execution history no longer grows unbounded** — `cleanup_executions`
  existed but was never scheduled; executions now prune every 6h (>30 days,
  matching messages).
- **Greeting/confirmation fast-path no longer misfires** — `starts_with`
  matched substantive messages ("ok here's my question…" → "OK!";
  "ok, create a device" short-circuited without acting). Now exact match.
- **Stream errors no longer poison the next turn** — a partial buffer
  (possibly mid-tool-call JSON) was saved as the assistant message on
  stream error. Now persisted only when it looks complete.
- **Event-agent vision misconfiguration is surfaced** — a camera event-agent
  whose active LLM lacks vision silently analyzed text-only. Now logged at
  error level and the LLM is told to state the limitation in its findings.
- **Vision tool no longer wastes a round on a fake-multimodal active backend**
  — dedicated VLMs are now preferred over the active chat backend (a text
  model wrongly marked multimodal burned a round before health-tracking
  demoted it).
- **Extension image tools now get the full image** — a camera metric stored
  as `{image_url: "/api/images/…", image_base64: "<109-byte preview>"}` gave
  extensions either a relative URL they can't fetch (they run in a separate
  process) or a truncated header-only base64 they can't decode — every
  extension image call returned null. `/api/images/` Object URLs are now
  resolved to full base64 in the data collector; sub-threshold fragments
  are omitted.
- **Update prompt no longer re-appears after updating** — the post-update
  version check read `app.config().version`, which can return empty in some
  Tauri 2.x builds, making the comparison always find an "update" so the
  dialog re-popped on every launch once the localStorage marker was
  consumed. Now uses the compile-time `CARGO_PKG_VERSION` and logs
  current vs remote for diagnosis.
- **Docker image builds again** — the multi-stage build had three latent
  breakages (docker.yml only runs on tag push, and no tag had been pushed, so
  CI never caught them): the `rust:1.85-alpine` pin was stale (Cargo.lock deps
  need rustc ≥1.89 — rmqtt-net, wide, time), `rust:*-alpine` ships gcc but not
  `make` so tikv-jemalloc-sys's C build died late, and `.dockerignore` used
  `target/` (root-only) so `web/src-tauri/target` (~22GB) leaked into the build
  context. Fixed: `rust:1.92-alpine` (matches rust-toolchain.toml), `apk add
  make`, `**/target/`. The glibc bare-metal release had masked the first two.
- **Data export writes CSV instead of xlsx** — the security hardening that
  removed the high-CVE `xlsx` dep missed a dynamic `await import('xlsx')` in
  the export dialog (static-import grep blind spot), breaking the frontend
  `tsc` build. Export now emits CSV (opens in Excel, zero dependency),
  completing the dep removal that change intended.
- **Frontend dependency vulnerabilities cleared** — `npm audit fix` removed
  the 4 production-path advisories (picomatch HIGH ReDoS/glob-injection,
  postcss CSS-stringify XSS, react-router open-redirect via `//path`).
  Production-path npm audit is now 0; remaining hits are dev-only
  (vitest/vite-node) and don't ship. Rust-side advisories (wasmtime, boa,
  rustls-webpki, protobuf) are major-version ports, tracked for 0.9.11.
- **HTTP chat now persists conversation history** — the multi-round rewrite
  consumed the event stream but never saved the turn (WS saves on disconnect),
  so HTTP/CLI/3rd-party chat forgot every turn on server restart. Now calls
  `persist_history` after the stream completes.
- **HTTP chat now honors images + page context** — the rewrite dropped
  `req.images`/`page_context`, so a REST vision client silently degraded to
  text-only. Now branches on images (multimodal stream) and prepends page
  context, matching the WS path.
- **Telemetry flush isolates poison points** — on a failed batch write, points
  are retried per-transaction so a single undecodable/oversized payload no
  longer blocks fresh writes for that metric forever. `write_count` also no
  longer double-counts re-queued points on retry.
- **Image junk-filter applied consistently** — the <1 KB base64 omission now
  also covers `/api/images/` URL resolution (both branches), so a header-only
  frame stored under `/api/images/` no longer feeds extensions undecodable bytes.
- **Agent-execution cleanup no longer scans at startup** — the periodic
  cleanup's first tick (immediate) is skipped, avoiding a full-table deserialize
  during boot against the backlog this cleanup exists to clear; cleanup
  failures are now logged instead of silently dropped.
- **About page shows the real version** — `get_app_version` uses compile-time
  `CARGO_PKG_VERSION` (was `app.config().version`, which returns None/"unknown"
  in some Tauri 2.x builds).

### Added
- **Pre-built Docker image on GHCR** (`ghcr.io/camthink-ai/neomind:<version>`,
  amd64 + arm64) published on each release — customers can `docker compose pull`
  instead of building from source. The ARM image sets `JEMALLOC_SYS_WITH_LG_PAGE=16`
  so it doesn't crash on 64KB-page hosts (Raspberry Pi 5 / Jetson), matching the
  bare-metal ARM release fix.
- **`neomind device history --limit <N>`** — caps data points per metric
  (the API already supported `limit`; the flag was missing, so `--limit 20`
  errored).
- **`rule create` / `agent create` errors ship worked examples** — bare
  "Invalid JSON" / "Focused mode requires --resources" gave the agent
  nothing to self-correct from; they now include copy-pasteable examples.
- **HTTP chat honors `backend_id` + `selected_skills`** (previously dropped)
  and exposes `HTTP_CHAT_TIMEOUT_SECS` (default 300s, matching the global
  agent execution budget — the old 120s cap was too tight for multi-round
  ReAct with a thinking model).

### Changed
- HTTP chat timeout: 120s → 300s (`HTTP_CHAT_TIMEOUT_SECS`).
- Greeting/confirmation matching: prefix → exact.

## [0.9.9] - 2026-07-17

A consolidation release hardening 0.9.8's image-URL storage and data-push
reliability, plus fixes surfaced by a 0.9.4→0.9.8 review.

### Fixed
- **store_raw now reaches upgraded installs** — the per-device-type `_raw` skip
  (0.9.8) never applied to existing installs because the builtin template
  version wasn't bumped; cameras kept writing `_raw` into telemetry. Bump so
  the seeder rewrites them.
- **Image URLs are no longer enumerable** — `/api/images/<dev>/<metric>/<ts>.ext`
  had a linearly-guessable timestamp on the public route. Now
  `<ts>_<content-id>.ext` (v5 of the bytes: same image → same file/idempotent,
  not guessable from outside).
- **Image retention no longer leaks `{ts}_{n}.ext` files** — collision-named
  files weren't parsed by cleanup and leaked forever.
- **Webhook 429/503 backs off instead of cascading** — rate-limited responses
  now honor `Retry-After` (or a long default) instead of the aggressive
  exponential backoff that hammered a throttled endpoint.
- **Webhook self-loop guard** — reject push targets whose URL points at this
  server's own ingestion endpoint (would loop forward→ingest→forward).
- **Virtual metrics no longer double-delivered to wide filters** — transform's
  double-publish made `*`/`device:*` deliver each virtual metric twice; deduped
  per target within a short window.
- **`.nep` downloads pick the right hardware variant** — jetson/cuda installs
  got the CPU build (variant selection was skipped on the .nep branch); also
  verify package SHA256 before install (was only checking the ZIP magic).
- **MQTT `$SYS` survives a `#` device filter** — root wildcards no longer dedup
  away broker presence subscriptions (broke external-broker transport state).
- **Temp files cleaned on download write/flush failure** (was leaking up to
  max_size on disk-full/quota).
- **Data Explorer export bundles images stored as `/api/images/` URLs** (since
  0.9.6 these exported as just the URL string, not image bytes).

### Changed
- Image-URL tests serialized (`serial_test`) — they set `NEOMIND_DATA_DIR` via
  a process-global env var and raced under the multi-threaded test runner.

## [0.9.8] - 2026-07-16

Device image storage reliability (the headline), per-device-type raw-metric
storage, telemetry memory tuning, delivery-history UX, plus data-push
reliability, a nested batch format, and a real-data test send.

### Fixed

- **Device image loss / "image not found" on download** — the real root cause
  of the reported "image corruption under dense reporting". The image-retention
  cleanup task interpreted the image filename timestamp as **milliseconds**, but
  `save_image_binary` writes **seconds** (ingest adapters pass `now.timestamp()`).
  A brand-new image (ts ≈ 1.75e9 s) parsed as 1970-01-21 — always older than the
  cutoff — so cleanup deleted **every image, including just-uploaded ones**, while
  the telemetry DB still held the `/api/images/` URL; downloads then returned
  `404 image not found`. Cleanup now compares in seconds, matching the filename
  unit, so existing second-granularity files are no longer mass-deleted.
- **Concurrent image-write corruption** (secondary, latent hazard in the same
  path). `save_image_binary` derived both the temp file (`.tmp.<ts>`) and the
  target (`<ts>.<ext>`) from the timestamp alone; two same-second saves shared a
  single temp file and the non-atomic `fs::write` interleaved/truncated bytes,
  producing corrupt images. Each write is now staged in a unique
  `tempfile::NamedTempFile` and atomically `persist_noclobber`-ed into place,
  with an idempotency check so the same frame saved twice (storage + event bus)
  resolves to one URL.
- **Duplicate pushed metrics** — the internal MQTT broker client subscribed to
  `#` AND per-device telemetry topics, so rumqttc delivered each uplink once per
  matching subscription (twice) and every metric was pushed twice (16 events
  instead of 8). Overlap is now deduped on both the initial subscription list
  and the dynamic per-device subscribe paths.
- **Batch splitting** — the batch flush timer was set once at task start, so
  after an idle period its deadline was already in the past and `sleep_until`
  fired immediately, splitting a single uplink into spurious small batches
  (e.g. `count:7` + `count:1`). The timer now restarts on the first event of
  each new batch.
- **Virtual metrics not forwarded** — transforms published their output events
  only under `transform:{id}:`, but the source picker / telemetry dual-write
  exposes them under `device:{id}:virtual.*`. A data-push target filtering on
  the device namespace never matched, so virtual metrics (OCR/vision outputs,
  etc.) were silently dropped. Transforms now also publish a
  `device:{id}:virtual.*` DeviceMetric (`is_virtual`, feedback-safe), so
  device-namespace filters forward them.
- **`_raw` whole-payload dump** is dropped from push output (huge for cameras,
  redundant with structured metrics).
- Removed the always-null `metadata` field from push payloads (the EventBus
  `EventMetadata` was discarded and never populated).
- **Delivery history** now sorts newest-first by `created_at` (the table is
  keyed by UUID, so iteration order was unrelated to recency).
- Test-send auto-sampling skips `_raw`/`ts` but keeps `virtual.*` (extension/
  transform outputs are valid business data).

### Added

- **Per-device-type `store_raw`** (`DeviceTypeTemplate.store_raw: Option<bool>`)
  controls whether the `UnifiedExtractor` emits the `_raw` metric (full payload
  snapshot). Precedence: template > extractor config (default `true`). NE301 /
  NE101 cameras ship `store_raw: false`, so their telemetry no longer redundantly
  stores the full base64 image as `_raw` — the image is already kept as
  `/api/images/...` via the dedicated image metric.
- **Delivery-history payload copy & preview** — the payload column in
  `DeliveryHistoryPanel` gains per-row Copy + Preview buttons; Preview opens a
  nested dialog showing pretty-printed JSON, byte count, and copy.
- **Nested batch payload format** for push targets (`BatchConfig.format`:
  `flat` | `nested`, default `flat`). Nested groups events by source into
  `items[].{source_type, id, data}`, splitting the source_id field on `.` to
  rebuild the object nesting that ingestion flattens (`device:9999:values.devName`
  → `data.values.devName`). Backward compatible — existing targets stay flat.
- **Real-data test send** — `POST /api/data-push/:id/test` now sends the latest
  metric for a bound source (falling back to the fixed sample when nothing is
  bound/found). Telemetry is wired into `PushManager` via `new_with_telemetry`.

### Changed

- **Telemetry redb cache capped** to shrink production RSS. redb 2.6.3 defaults
  to a 1 GiB per-DB page cache; `telemetry.redb` is the only store large enough
  to fill it (~916 MB anonymous heap, the dominant contributor to RSS ~2.4 GB).
  Capped via `NEOMIND_TELEMETRY_CACHE_MB` (default 256 MiB); the OS page cache
  backs reads regardless, so read perf is largely preserved while moving the
  cache from non-reclaimable heap to reclaimable page cache. Target: RSS
  ~2.4 GB → ~1.7 GB.
- Batch Aggregation UI gains a Payload Format dropdown, visible field labels,
  and description/hint text (zh/en).

## [0.9.7] - 2026-07-15

Hotfix for the 0.9.6 ARM64 server startup crash.

### Fixed

- **ARM64 server crash on 16 KB / 64 KB page-size kernels** (e.g. Raspberry Pi 5).
  The 0.9.6 jemalloc global allocator was compiled with a fixed 4 KB page size, so
  on any ARM64 system whose kernel page size is larger (Pi 5 = 16 KB; some ARM64
  servers such as Kunpeng/Graviton/Ampere = 64 KB) jemalloc aborted at startup with
  `<jemalloc>: Unsupported system page size` and the server could not start. Builds
  jemalloc with a 64 KB page size (`JEMALLOC_SYS_WITH_LG_PAGE=16`), which covers
  4/16/64 KB hosts — jemalloc only requires compiled page ≥ system page, so the same
  binary runs everywhere. Linux server only; desktop, macOS, and x86_64 were
  unaffected. (#11)

## [0.9.6] - 2026-07-15

Image metric URL storage migration plus cross-boundary hardening and on-disk
file-lifecycle fixes.

### Added

- **Image metric URL storage** — image data (base64, ~50 KB-MB per data
  point) is now stored as files on disk (`data/images/<device>/<metric>/<ts>.<ext>`)
  with only a short URL (`/api/images/...`, ~50 bytes) kept in the telemetry
  database. This reduces `telemetry.redb` size by ~1000× for image-heavy
  deployments and eliminates multi-second telemetry queries that returned large
  base64 payloads.
  - **Ingestion fork conversion**: Binary → save file → URL string, passed to
    both storage and EventBus (single conversion point, guaranteed consistency).
  - **Authenticated image serving**: `GET /api/images/*path` (requires login,
    cookie-based auth for dashboard `<img src>`).
  - **Agent vision compatibility**: `image_utils::resolve_image` and
    `data_collector::extract_image_data` resolve `/api/images/` URLs → read file
    → base64 for LLM vision input. Old base64 data still works.
  - **Transform compatibility**: `find_image_data` resolves URLs → file → base64
    before injecting into JS sandbox.
  - **Image file retention**: `cleanup_expired_images()` scans `data/images/` by
    filename timestamp, deletes expired files + empty directories, synchronized
    with telemetry `image_retention` (default 72h).
  - **Retention sync fix**: `value_looks_like_image()` now recognizes
    `/api/images/` URLs so telemetry records are deleted at `image_retention`
    (not `default_retention`), preventing a 404 window where records outlive
    files.
  - **Backward compatible**: old base64 telemetry data continues to display and
    is naturally cleaned by retention. No migration needed.

- **jemalloc global allocator (Linux only)** — replaces glibc malloc to fix
  per-thread arena fragmentation that caused server RSS to climb 4-6 GB over
  days. jemalloc packs allocations tightly and returns freed pages to the OS
  promptly. macOS and Windows use their own allocators (no glibc) so they're
  unaffected. `#[cfg(target_os = "linux")]` gates both the allocator and the
  dependency.

### Fixed

- `json_to_metric_value` now short-circuits `/api/images/` URLs to
  `MetricValue::String` (prevents accidental base64 re-decoding).
- `adapter.rs convert_metric_value` Binary→base64 kept as documented fallback
  (ingestion fork converts Binary→URL before reaching adapter).

- **Image URL storage — completed cross-boundary resolution.** Several
  consumers of image metrics still expected base64 and silently mishandled
  the `/api/images/` URL form. All now resolve through the centralized
  helpers `image_storage::{read_internal_image_url,
  resolve_internal_image_to_data_url}`:
  - **Extension commands**: image args resolve to raw base64 before crossing
    the extension process boundary (extensions are a separate process and
    can't read hostless paths; previously failed with "Invalid base64").
  - **Device command downlink**: command params carrying `/api/images/`
    resolve to base64 data URLs before rendering, so external devices receive
    usable bytes (mirrors the data-push outbound fix).
  - **Chat `$cached:` references**: `LargeDataCache` recognizes `/api/images/`
    URLs, so vision-tool chaining via cached (≥32 KB) tool results still feeds
    vision tools the actual bytes instead of a raw JSON string.
  - Centralized the URL→bytes / URL→data-URL read (with symlink-escape /
    20 MB / magic-byte guards), replacing scattered local readers.
  - data-push resolves `/api/images/` after the source filter, avoiding
    resolution for sources that won't be delivered.
  - Ingestion now converts base64-**string** image payloads (not just
    `Binary`) to `/api/images/` URLs.
  - **Unpadded / whitespace-containing base64 now decodes** (e.g. NE301 cameras
    emit standard-alphabet base64 with no `=` padding, `len % 4 != 0`). The
    strict `STANDARD` decoder rejected these ("Incorrect padding") and the
    `URL_SAFE_NO_PAD` fallback used the wrong alphabet, so such images were
    left stored as raw base64 instead of URLs. `try_decode_base64_image` now
    strips whitespace + padding and decodes via `STANDARD_NO_PAD`.
  - Frontend: all image preview/download components recognize `/api/images/`
    URLs (prepend server origin; fetch-as-blob for download).

- **Image file lifecycle:**
  - Unregistering a device now purges its `data/images/<device>/` directory
    (previously lingered until age-based cleanup, up to `image_retention`).
    Path-component validation + canonicalize guard prevent traversal/symlink
    escape; best-effort, never blocks unregister.
  - `cleanup_expired_images` reclaims stale `.tmp.*` temp files left by a
    crashed `save_image_binary` (previously never collected — slow disk leak).
  - `detect_content_type` no longer flags a result as image merely for
    mentioning `/api/images/` in prose (e.g. an error message); requires a
    bare URL or a JSON string value, avoiding false vision auto-injection.

### Changed

- `getServerOrigin()` is computed per call (dropped the memoized cache) to
  avoid stale-origin risk on instance switch.

## [0.9.5] - 2026-07-13

### Added

- **Extension hardware variant selection (CUDA / Jetson)** — the extension
  marketplace now auto-selects a CUDA/Jetson-specific build when the host
  matches, falling back to the generic OS+arch build, then to wasm.
  - Detection order: `NEOMIND_EXTENSION_VARIANT` env override
    (`cpu|cuda|jetson`) → `/etc/nv_tegra_release` (Jetson, checked first) →
    `nvidia-smi` (CUDA) → CPU. Jetson is checked before CUDA so a Jetson
    with `nvidia-smi` present is not misclassified.
  - New `crates/neomind-core/src/extension/accel.rs` is the single source
    of truth (`Variant`, `fallback_keys`, `detect_variant` with `OnceLock`
    caching + best-effort degradation). `select_build_key` in
    `install_from_marketplace_handler` resolves
    `linux-aarch64-jetson` → `linux-aarch64` → `wasm`.
  - **Zero regression** — variant discrimination lives only in marketplace
    `metadata.json` `builds` keys and release filenames; the `.nep`
    internal `manifest.binaries` key stays the plain OS+arch (e.g.
    `linux_arm64`), identical for CPU and Jetson builds. Pure-wasm and
    pure-native extensions behave exactly as before; manual `.nep` upload
    is unaffected.
  - End-to-end Jetson auto-download additionally requires the marketplace
    to publish a `linux-aarch64-jetson` entry in `metadata.json` `builds`
    (NeoMind-Extensions side). Until then Jetson devices fall back to the
    CPU build, equivalent to today's behavior.

- **Extension README on the marketplace detail page** — the "View Details"
  dialog now renders the extension's `README.md`. New best-effort endpoint
  `GET /api/extensions/market/:id/readme` proxies the marketplace README
  and returns `{ content: null }` when absent (so the section is simply
  hidden, never an error). README is rendered with `react-markdown` + GFM;
  relative links/images are rewritten to absolute GitHub raw URLs so
  screenshots and doc links load. Loads asynchronously, never blocks the
  detail view.

### Fixed

- `find_nep_binary` match arms in `neomind-core::extension::loader::native`
  used hyphen keys (e.g. `"linux-arm64"`) while `detect_platform()` returns
  underscore (`linux_arm64`), so every arm was dead code. Aligned the arms
  to underscore; no behavior change for standard packages (the default
  branch already returned the correct directory).

- **Marketplace download/upload no longer buffer the whole package in memory
  (OOM fix)** — both paths used `read_to_end`/`bytes()`, so a large `.nep`
  (e.g. paddle-ocr-v6 + CUDA ORT, hundreds of MB) peaked at ~3× package size
  in RAM and OOM'd edge devices. The 0.9.5 upload-ceiling bump didn't help —
  it raised the body limit, not the in-memory buffering.
  - Downloads (`marketplace install`) stream the body to a temp file
    (`bytes_stream`), enforce a 1 GB cap (Content-Length + running byte
    counter), and extract via `install_from_file` (File-backed `ZipArchive`).
  - Uploads (`load` + `install`) stream-hash the file and read the manifest
    via a File-backed archive instead of `read_to_end`.
  - All zip entry extraction switched from `read_to_end` to `std::io::copy` /
    chunked copy — the original bug, surfaced by a memory-footprint test.
  - New `MAX_EXTENSION_DOWNLOAD_SIZE = 1 GB`; upload body limit unchanged at 512 MB.
  - Verified by `#[ignore]` memory tests: a 150 MB package spikes RSS by
    **0.8 MB** (download) / **2.2 MB** (upload), vs hundreds of MB before.

### Overview

This release fixes a class of **dark-mode rendering bugs** where many UI
elements were silently invisible, hardens HTTP body-limit handling so
large POST bodies are no longer rejected, raises the extension upload
ceiling for large ML model bundles, and folds timezone selection into
the first-run setup flow.

### Dark-mode transparency (frontend)

- **Root cause** — semantic colors are defined as OKLCH CSS variables
  (e.g. `--muted-foreground`), and Tailwind v3 cannot apply its `/opacity`
  modifier to a bare `var(--x)` color. Every `bg-muted-foreground/30`,
  `bg-background/95`, `ring-foreground/30`, `from-muted/50`, etc.
  **failed to generate any CSS rule**, leaving the element with no
  background → fully transparent. Verified empirically by compiling the
  real config: the broken classes produce no output, while `bg-muted-30`
  / `bg-bg-95` generate correctly.
- **Why dark mode looked worse** — the failures affect both themes, but
  most of these elements (skeleton bars, status dots, the streaming
  cursor, the scrollbar thumb) are meant to be *dim-but-visible*; their
  absence against a dark surface reads as an obvious hole, whereas
  against a light surface it is barely noticeable.
- **Fixes** — every broken `/opacity` usage replaced with a token that
  actually generates:
  - Skeleton loading bars (chat history), the streaming "thinking"
    cursor, off-line / disabled status dots, and the scrollbar thumb →
    solid `bg-muted-foreground` (visible in both themes).
  - Mobile chat input header `bg-background/95` → `bg-bg-95` (predefined
    95% alpha — exact equivalent).
  - Button keyboard-focus ring `ring-foreground/30` → `ring-ring`. This
    also restores the ring that vanished when the earlier
    "ring-ring-flashed-orange" workaround (from when `--ring` aliased
    `--brand`) was switched to the broken `ring-foreground/30`; `--ring`
    has since been redefined to a neutral `foreground@35%`, so `ring-ring`
    is safe again and consistent with the 16 other components using it.
  - Secondary text that used `text-muted-foreground/N` (which silently
    fell back to full-contrast foreground) → `text-muted-foreground`.
  - Gradient fade-outs and the skeleton shimmer mid-stop → predefined
    alpha tokens (`from-muted-50`, `via-muted-30`).
  - Row / element hover washes → solid `hover:bg-muted` /
    `group-hover:bg-muted`.
  - A second pass (the first scan's regex missed predefined alpha tokens
    with digits in the name, e.g. `bg-bg-50`) caught seven more: the
    **login form card** (`bg-bg-50/95` → `bg-bg-50`, the card had been
    transparent over its background glows), the calendar date-range
    highlight (`bg-accent/50` → `bg-accent`), the chat active-session
    timestamp and action-button hovers plus the extension filter count
    badge (`*-primary-foreground/N` → `white/N` — `primary-foreground`
    resolves to white in both themes, and `white` supports the opacity
    modifier), and the dashboard mobile edit-mode overlay
    (`bg-bg-30/20` → `bg-muted-30`; `--bg-30` was never defined). The
    `/opacity` bug class is now at zero across the frontend.

### HTTP body-limit alignment (backend)

- **Root cause** — the global `RequestBodyLimitLayer` (10 MB) was applied
  to the API routes, but axum's `Json` / `Bytes` extractors consult a
  *separate* `DefaultBodyLimit` whose default is 2 MB. Without an explicit
  `DefaultBodyLimit`, large POST bodies (e.g. base64 images sent to
  extension command endpoints) were rejected with **413** even though the
  request layer allowed 10 MB.
- **Fix** — `router.rs` now layers
  `DefaultBodyLimit::max(MAX_REQUEST_BODY_SIZE)` alongside
  `RequestBodyLimitLayer`, so both gates accept the same payload size.

### Extension upload ceiling

- `MAX_EXTENSION_UPLOAD_SIZE` raised from **100 MB → 512 MB** so large ML
  model bundles (e.g. paddle-ocr-v6 with CUDA ORT libraries plus
  multi-tier ONNX models) can be installed via the extension upload
  endpoint without hitting the cap.

### Setup flow

- Timezone is now captured during first-run setup: the browser timezone
  is auto-detected on the account-creation step and saved silently, and
  the completion screen exposes an adjustable timezone selector (saved
  via `PUT /settings/timezone`). Removes the need to visit Settings just
  to set the timezone on first run.
- Setup screens received mobile / layout polish: safe-area insets,
  `viewport-full` sizing, responsive icon and spacing scales, and an
  entrance animation.

---

## [0.9.4] - 2026-07-10

### Overview

This release fixes a long-standing bug where **saved extension
configurations were never reapplied** on reload, crash recovery, or
startup. The three code paths responsible all routed the saved config
through `execute_command(id, "configure", ...)`, but `configure` is a
lifecycle method, not a registered command — so it failed with
"Command not found: configure" on every invocation, and the extension
kept running with its default config.

### Extension config application

- **Root cause** — `configure` is an SDK lifecycle method invoked via
  the dedicated `ConfigUpdate` IPC channel; it is not present in any
  extension's `commands` list. `execute_command` only dispatches
  registered commands, so it silently failed on every reload/recovery.
- **Fix** — all three call sites now use the proper IPC:
  - `reload_extension_handler` (manual reload after config edit)
  - crash-recovery loop in `server/mod.rs` (auto-restart after a
    crash-loop-disabled extension is re-enabled)
  - startup load path in `extension_state.rs` (initial config apply on
    server boot)
- All three paths use `runtime.send_config_update(&id, cfg)`, which
  routes through the runner's ConfigUpdate channel →
  `neomind_extension_configure_json`, matching the hot-reload path
  already used for live config edits.

---

## [0.9.3] - 2026-07-09

### Overview

This release fixes a critical bug where **chat and scheduled agents
could not access base64 image metric data** — the data was silently
truncated to `[image data, 63B]` before reaching the LLM, making it
impossible for the agent to analyze camera snapshots, YOLO output
frames, or any telemetry metric whose value is an image.

The fix introduces a **value-level slim mechanism** that caches large
strings out of the tool-result JSON and replaces each with a
one-sentence natural-language summary containing a `$cached:`
reference. The LLM reads the summary, passes the reference to the
`vision` tool (or any image-aware tool), and the reference is
transparently resolved back to the full binary payload at tool-call
time. No new tools were added — the existing `vision` / `image_edit`
pipeline picks up the cached data automatically.

On the frontend side, the **AI Analyst dashboard component** gets
i18n completeness, real-time progress events, and a streaming-bubble
UX polish.

### Agent image-data slim mechanism

- **Root cause** — CLI's `sanitize_metric_value` truncated any string
  > 80 bytes to 60 chars, then streaming's
  `sanitize_tool_result_for_prompt` stripped `data:image/` URLs
  entirely. Double truncation: the LLM never saw usable image data.
- **`slim_large_strings_in_json`** (new method on `LargeDataCache`)
  — walks the tool-result JSON tree, detects large strings
  (`data:image/` prefix regardless of size, or any string > 64 KB),
  stores each in the cache under a deterministic `path#8hex-hash` key
  (multi-image safe), and replaces the value IN PLACE with a complete
  natural-language sentence:
  `Image data (image/jpeg, 271.4KB) cached as $cached:shell.data.metrics.values.image.value#a1b2c3d4 — pass this reference to the \`vision\` tool's \`image\` argument to analyze the content.`
  Sibling fields in the JSON object are preserved untouched.
- **`SLIM_THRESHOLD_BYTES = 64 KB`** — independent from
  `CACHE_THRESHOLD_BYTES` (32 KB, which gates `store()`). Kept higher
  so that (a) anything slim decides to cache is guaranteed to actually
  be stored, and (b) legitimate large text payloads (compact configs,
  multi-row query results, short logs) still reach the LLM verbatim
  instead of being hidden behind a reference.
- **Chat streaming path** (`stream_core.rs`, `stream_multimodal.rs`)
  — slim runs BEFORE sanitize. After slim, the value is plain text
  (no `data:image/` prefix), so sanitize's own stripping path is
  skipped. The slimmed result is what enters the tool-call-results
  vector and the LLM message history.
- **Scheduled agent path** (`tool_loop.rs`, `tool_result.rs`) —
  per-execution `LargeDataCache` created in `run_tool_loop`.
  `resolve_cached_arguments` runs before `registry.execute_parallel`
  to substitute `$cached:` references in tool-call arguments (same
  function the chat path uses). `process_tool_results` now slims
  before sanitize, mirroring the chat streaming pipeline. Both agent
  execution modes (chat + scheduled) now have fully symmetric
  slim + resolve pipelines.
- **Privacy gate preserved** — the `IMAGE_AWARE_TOOLS` list
  (`["image_edit", "vision"]`) still gates the omitted-field
  auto-inject path. `$cached:` explicit-reference resolution is
  ungated (the LLM intentionally passes the reference), but the
  defense-in-depth "inject even when the LLM omitted image args"
  branch only fires for tools that legitimately consume images.

### CLI image-data passthrough

- **`sanitize_metric_value` exception** — in agent mode
  (`NEOMIND_JSON=1` env var set), strings starting with `data:image/`,
  `http://`, or `https://` now pass through untouched. Previously,
  all strings > 80 bytes were truncated to 60 chars, which (a)
  destroyed base64 image data URLs, and (b) truncated long signed
  HTTP URLs (e.g. pre-signed S3 image links) making them unresolvable.
  Human terminal mode is unchanged — truncation still applies for
  readability.
- **`summarize_image_history`** — `device history` responses are now
  post-processed: for each metric whose sampled values (first / mid /
  last) look like images (`data:image/` prefix or URL ending in a
  known image extension), the full data-point array is replaced with a
  compact summary object containing `count`, `earliest_ts`,
  `latest_ts`, `interval_avg_ms`, `latest_value` (the full data URL,
  preserved so the slim layer can cache it), and a natural-language
  `note` pointing at the `vision` tool. Non-image metrics pass through
  untouched. Prevents 288 × 271 KB ≈ 78 MB responses from flooding
  the agent context.

### AI Analyst component improvements (frontend)

- **Full i18n** — all hardcoded English strings in `AiAnalyst`,
  `AnalystConfigPanel`, `AnalystMessageBubble`, and `AnalystTimeline`
  replaced with `t()` calls. New locale keys added under
  `aiAnalyst.*` in both `en` and `zh` `dashboard-components.json`.
- **AgentProgress / AgentThinking WS events** —
  `useAnalystSession` now handles these real-time event types,
  showing stage-level progress ("Collecting data...", "Analyzing 5
  data points...", "Tool-calling round 2") in the streaming bubble
  while the agent executes. Previously the bubble showed nothing
  until the execution completed.
- **Streaming bubble gap fix** — on `AgentExecutionCompleted`, the
  streaming content and message ID are no longer cleared immediately.
  They persist until the `getExecution` API response arrives, closing
  a blank-bubble gap between the completion event and the result
  fetch.
- **Persistent image dedup** — the timeline's image-enqueue dedup is
  now persistent across rounds (was per-round). Prevents a timing
  race where the `AgentExecutionStarted` WS event arrives before the
  telemetry update, causing the stale previous-round image to
  enqueue first and the fresh image to append right after (two
  images per update).
- **Vision model multimodal badge** — model picker entries in the
  AI Analyst config schema now show an `Eye` icon next to models
  flagged `isMultimodal`, making it visually clear which models can
  process images. The `isMultimodal` field flows through
  `SchemaContext.visionModels` → `useComponentConfigDialog` →
  `business.tsx`.
- **Schema regeneration on async load** — `useComponentConfigDialog`
  now regenerates the config schema when `visionModels` or `agents`
  arrays resolve (previously the schema was built once at dialog-open
  time with empty arrays, leaving dropdowns empty if the fetch
  hadn't completed yet).

## [0.9.2] - 2026-07-07

### Overview

This release ships the **Dashboard Duplicate** feature plus a handful
of smaller fixes that landed alongside it. The headline is a one-click
"Duplicate" action on every dashboard that produces a fully isolated
clone — including a deep copy of any component-owned transforms — so
the original and the copy can be edited or deleted independently
without breaking each other.

The dashboard action UI also gets a small refactor in the same batch:
both sidebar mode and tabs mode now expose per-dashboard actions
through a unified `MoreVertical` ("...") dropdown instead of the
previous row of inline hover buttons (which had grown to five icons
after adding Duplicate).

Rounding out the release are a llama.cpp context-overflow error
message fix, a dialog overflow CSS tweak, and a README refresh for
the extension marketplace.

### Dashboard Duplicate

- **New endpoint `POST /api/dashboards/:id/duplicate`** — server-side
  clone of a source dashboard. The new dashboard gets a fresh UUID,
  the name is suffixed with ` (copy)` (hardcoded English suffix,
  intentionally not i18n'd), `is_default` is reset to `None` (so the
  copy never silently steals default status from the original), and
  `sort_order` is set to `max + 1` to append at the end of the list.
  Emits the existing `DashboardUpdated` event with `action = "create"`
  so all realtime subscribers (WS/SSE) refresh automatically.
- **Component-owned transform deep cloning** — the key isolation
  mechanism. Components can bind transforms two ways: *referenced*
  (user picked an existing transform via the data source picker) or
  *owned* (the component created the transform inline, marked by
  `config._transformId`, and deletes it when the component is
  removed). On duplicate, only **owned** transforms are deep-cloned:
  the clone gets a fresh UUID (`transform_{uuid}`), a fresh
  `output_prefix` (`{sanitized_source}_{8-char-uuid}`, because two
  transforms sharing the same prefix would collide in the
  `extensionMetric: "<prefix>.<field>"` namespace), `execution_count`
  reset to 0, and `last_executed` cleared. All references inside the
  cloned component are rewritten consistently —
  `config._transformId`, `dataSource.transformId`,
  `dataSource.sourceId`, `dataSource.id`, plus
  `dataSource.metricId` / `dataSource.field` get their old-prefix
  portion replaced with the new prefix.
- **Shared references stay shared by design** — device IDs, agent
  IDs, and extension IDs in component data sources are NOT cloned.
  These are global resources (a temperature sensor physically exists
  once), so the duplicated dashboard references the same source.
  User-referenced transforms (no `_transformId` marker) are also
  left shared, matching the user's intent.
- **Frontend integration** — new `duplicateDashboard(id)` store
  action calls the API, runs the response through `fromDashboardDTO`
  (per the snake_case → camelCase dashboard DTO gotcha), appends to
  `dashboards[]`, and calls `recordSelfSync(newId)` so the
  backend's `DashboardUpdated` SSE event doesn't trigger a redundant
  `fetchDashboards()` refetch race. The handler then shows a toast
  and navigates to the new dashboard.
- **Pure logic helpers, fully unit-tested** —
  `new_output_prefix()` (sanitization + UUID suffix, unique across
  calls) and `rewrite_component_transform_refs()` (5-field rewrite
  gated on the `_transformId` ownership marker; no-op when the
  marker is missing or doesn't match) are extracted as pure
  functions and covered by 4 unit tests. `build_duplicate_dashboard`
  (the in-memory clone pipeline with no I/O) adds 2 more tests
  covering the full rewrite path and the `"X (copy)" → "X (copy)
  (copy)"` double-suffix edge case.

### Dashboard action menu unification

- **Sidebar mode (`DashboardListSidebar`)** — replaces the five
  inline hover buttons (Move Up / Move Down / Rename / Duplicate /
  Delete) with a single `MoreVertical` trigger opening a
  `DropdownMenu`. The trigger inherits the same hover-to-reveal
  behavior (`opacity-0 group-hover:opacity-100`) so the row stays
  clean at rest.
- **Tabs mode (`DashboardTabBar`)** — the existing per-tab
  `MoreVertical` dropdown gains a new Duplicate item between Rename
  and Delete. Mobile switcher path also updated.
- **Shared menu structure** — both modes now expose the same 5
  items in the same order: Move Up / Move Down / separator / Rename
  / Duplicate / Delete. Delete keeps the `text-error focus:text-error`
  destructive styling.

### Fixes & polish

- **llama.cpp context-overflow reporting** — `ContextOverflow` errors
  now prefer the server-reported `n_ctx` from the error body over
  the cached `max_context_length()`. The cached value can be stale
  (e.g. server restarted with a different `--ctx-size` but
  capabilities not re-detected) or a theoretical default, which
  previously produced misleading messages like `"11958 < 32000"`
  when the real server-side limit was 8192. Both the non-streaming
  and streaming error paths are updated.
- **`UnifiedFormDialog` overflow** — added `overflow-hidden` to the
  dialog content surface so child widgets no longer bleed past the
  rounded corners on small viewports.
- **README extensions refresh** — the official extensions list in
  both `README.md` and `README.zh.md` is expanded from ~9 entries
  to the current 22 (vision, voice, IoT bridges, utilities),
  reorganized by category.

### Diagnostic log archive download

- **New `GET /api/logs/download?days=N` endpoint** — bundles every
  `neomind.log.*` daily-rotated file under `<data_dir>/logs/` into a
  single in-memory ZIP and streams it back as
  `Content-Disposition: attachment`. Intended for support/diagnostic
  flows: the user picks a time range in Settings → Preferences and
  downloads a zip to email back to the team. Three defense-in-depth
  memory caps on edge devices: 64 MiB per file, 60 files max, 512 MiB
  total.
- **Local-time date filter** — `tracing_appender::rolling::daily`
  names files using LOCAL time, so the filter uses `chrono::Local`
  (not UTC) to match. Off-by-one fix: `days=1` means today only
  (was today + yesterday). The bare `neomind.log` active file
  (no date suffix) always passes the filter.
- **Canonical log path unification** — the Tauri shell now writes
  logs to `<app_data>/data/logs/` (was `<app_data>/logs/`), matching
  `NEOMIND_DATA_DIR` and the API handler's read path. A one-time
  `migrate_legacy_log_dir()` runs at startup to move existing files
  to the new location with a cross-filesystem copy+delete fallback.
  CLI `neomind logs` checks the new path first, keeps the legacy
  path as fallback for ≤0.9.1 upgraders.
- **Frontend** — `DiagnosticDataCard` lives in PreferencesTab (next
  to Data Management, both being operational features), using the
  preferences width convention (`Select w-full sm:w-[180px]` +
  inline `size="sm"` button). `api.downloadLogs` parses JSON errors
  only — raw backend text never leaks to the toast.
- **i18n fix** — pre-existing `updateAvailableWithVersion` had
  single-brace `{version}` which i18next renders literally; fixed to
  `{{version}}`.

---

## [0.9.1] - 2026-07-06

### Overview

This release bundles two release batches that landed under the same
version (the previous `[0.9.1]` section was prepared but never
tagged). The batch below covers extension ↔ agent streaming,
per-session config, plugin UX, and a brand-new built-in
`image_edit` tool. The earlier-prepared dashboard ordering + password
show/hide work is preserved as a sub-section at the end.

Three workstreams landed together because they all touch the
extension ↔ agent streaming boundary:

1. **ChatSession capability family (Phase 2 streaming)** — the
   one-shot `chat_stream` capability is now split into a persistent
   session-stream API (`chat_session_open` / `send` / `close` /
   `cancel_turn`) so extensions can hold a long-lived subscription,
   receive an authoritative stream-termination signal (`AgentStreamEnd`),
   and disambiguate overlapping turns via `turn_id`.
2. **Per-session config overrides** — voice-assistant and similar
   workloads can now bake a `systemPrompt` / `temperature` / `model` /
   `enableTools` patch into a session at creation time (REST
   `POST /api/sessions` and WS chat auto-create path) instead of
   polluting every user message via `pageContext`.
3. **Extension runtime/tooling polish** — dynamic metrics become
   visible without per-poll IPC, plugin config dialog supports
   instance rename + a dedicated thinking toggle, and the asset
   cache-control is loosened so dev iteration on extension bundles
   no longer requires a Tauri WKWebView cache flush.

A new system-prompt rule also lands: a strict 3-condition chitchat
fast path so pure greetings skip tools, while anything that smells
like domain state still routes through the tool layer.

### ChatSession capability family (Phase 2 streaming)

- **5 new `ExtensionCapability` variants**:
  `ChatStreamCancel`, `ChatSessionOpen`, `ChatSessionSend`,
  `ChatSessionClose`, `ChatStreamCancelTurn`. All added to the
  runner's `ALLOWED_CAPABILITIES` allow-list and routed through the
  new `ChatSessionCapabilityProvider` (except `ChatStreamCancel`,
  which extends the existing `ChatStreamCapabilityProvider`).
- **`NeoMindEvent::AgentStreamEnd`** — authoritative transport-layer
  terminator published alongside the existing `AgentStreamChunk`.
  Reason: chunk-internal `type=end` is ambiguous on reasoning models
  and tool loops (intermediate end-like chunks). Subscribers should
  treat `AgentStreamEnd` as the only true "no more chunks will
  arrive" signal. `event_name()` and `timestamp()` plumbing updated.
- **Direct subscriber routing on `SessionManager`**:
  `subscribe_events(session_id, buffer)` / `remove_subscriber` /
  `publish_to_subscribers`. Uses bounded `mpsc` with `try_send` so a
  slow subscriber never wedges the agent stream (events are dropped
  rather than buffered). Today 0-or-1 subscribers per session;
  fan-out is forward-compatible.
- **`turn_id` injection** — `ChatSessionSend` generates a UUIDv4
  `turn_id`, returns it immediately (does NOT wait for LLM
  completion), and tags every chunk wrapper for that turn. Callers
  can disambiguate rapid consecutive turns without guessing.
- **SDK surface** (`crates/neomind-extension-sdk/src/capabilities/chat.rs`):
  `open_session` / `send_message` / `close_session` / `cancel_turn`
  async helpers + capability constants on the host.
- **`ChatStream` hardening** — the spawn task now publishes a
  terminal `AgentStreamEnd{reason="error"}` + chunk on upstream
  `process_message_events` failure (previously a silent hang), and
  `ChatStreamCancel` is exposed as a first-class capability instead
  of relying on extension shutdown to free the LLM generation slot.
- **Manifest / allow-list wiring** — `ChatSessionCapabilityProvider`
  registered in `ServerState`; `extension-runner/main.rs`
  `ALLOWED_CAPABILITIES` extended with the 5 new names.

### Per-session config overrides

- **`neomind_agent::CreateSessionOptions`** — small Option-struct
  (`system_prompt`, `temperature`, `model`, `enable_tools`). Applied
  **only** on newly-created sessions; existing sessions reused via
  `get_or_create_session_with_options` keep their original config
  (override silently ignored). Re-exported from `neomind_agent`.
- **`SessionManager::create_session_with_options(opts)`** — side-by
  side with `create_session()`; default path unchanged.
- **REST: `POST /api/sessions`** — body is now optional
  (`Option<Json<Option<CreateSessionRequest>>>`). Honors both legacy
  `{config: AgentConfig}` (translated to the patch) and the new
  granular patch shape.
- **WS chat** — `ChatRequest.sessionConfig` (camelCase) is honored
  only at the moment of session auto-creation; subsequent frames
  targeting an existing session ignore the field by design.
- **`models::SessionConfigPatch`** + `From<SessionConfigPatch> for
  CreateSessionOptions` — keeps the boundary type in `neomind-api`
  and translates into the agent-side type at the call site.

### Plugin / LLM backend dialog UX

- **Instance rename** — `UniversalPluginConfigDialog` now pre-fills
  the instance name in edit mode and validates non-empty before
  submit; `handleUpdate` propagates `name` through
  `UpdateLlmBackendRequest`. Previously the name rendered as a
  read-only `<h3>`, making rename impossible.
- **Thinking toggle as a first-class switch** — separate from the
  multimodal override (which is a user override on top of runtime
  detection). Thinking is a plain backend config field, so the
  dialog PATCHes `thinking_enabled` directly via `api.updateLlmBackend`
  with optimistic update + rollback, mirroring the multimodal flow.
  Initial value is read from `instance.config.thinking_enabled`,
  defaulting to `true` (matches `default_thinking_enabled()`).
- **`ConfigFormBuilder` footer pattern** — new `formId` and
  `hideSubmitButton` props let a parent place the submit button in
  a dialog footer (bound via the HTML `form` attribute) instead of
  the inline bottom-of-form Button.
- **Backend logging** — `update_backend_handler` now emits a
  dedicated `User thinking_enabled setting updated` log line with
  `prev`/`new` values, distinct from the capabilities-support log
  (which reports `supports_thinking` — a model capability from
  `/api/show`, not the user's enable/disable choice).

### Extension runtime polish

- **Dynamic metric descriptor refresh** —
  `ExtensionMetricsCollector` now refreshes the cached descriptor
  from the extension every TTL window (default 60s) bounded by a
  hard timeout (default 10s). Without this, dynamically-added
  metrics (e.g. `fps.cam1`, `latency_ms.task-42` — see new
  `crates/neomind-extension-sdk/src/dynamic_metrics.rs` helper)
  stayed invisible to `/api/extensions` until the runner restarted.
  The timeout is a safety bound against mixed deployments where the
  runner may not recognize the `GetDescriptor` IPC message (would
  otherwise stall for the full `command_timeout_secs` = 300s).
  Both durations are configurable via
  `with_descriptor_ttl` / `with_descriptor_refresh_timeout`.
- **Asset cache-control loosened** — `serve_extension_asset_handler`
  switched from `public, max-age=3600` to `no-cache`. Tauri WKWebView
  was serving 1-hour-stale bundles in dev after a rebuild; bundles
  are small (tens of KB) so re-fetch on each navigation is
  negligible.
- **`install_sync` step-by-step logging** — `upload_extension_file_handler`
  and `ExtensionPackage::install_sync` now emit numbered step logs
  plus a dedicated `tracing::error!` on task-join / install failure
  with `extension_id` and `kind`, replacing bare
  `format!("Installation failed: {}")` strings.

### System prompt

- **Chitchat fast path** — three conjunctive conditions for skipping
  tools: (a) pure greeting/identity/courtesy phrase, (b) no reference
  to any domain entity (devices/metrics/rules/agents/dashboards/etc.),
  (c) a direct text reply fully satisfies the request. Includes a
  concrete "DO call tools" list for ambiguous-looking messages
  ("anything happening today?", "everything normal?", "any anomalies?")
  so the model defaults to tool-calling when in doubt. Rule applies
  by intent, not by language (English / Chinese).

### Tests / fixtures

- **`crates/neomind-core/tests/fixtures/smoke-extension/build.rs`** —
  sets the macOS dylib install name to `@rpath/extension.dylib` at
  link time so the runner's dylib validation accepts this fixture.
- **`crates/neomind-cli/test-extension/Cargo.lock`** — generated
  lockfile for the in-tree test extension example.
- **`crates/neomind-extension-sdk/src/dynamic_metrics.rs`** —
  reusable helper for multi-instance extensions to register base
  metric templates × runtime labels (e.g. `fps.cam1`).

### Built-in `image_edit` tool

A new agent tool that lets the LLM perform non-destructive image
editing operations inline — drawing detection boxes, annotations,
arrows, text, blurs, and crops — without delegating to an extension.
Designed so a single tool call handles a multi-step pipeline.

- **Pipeline executor** (`crates/neomind-agent/src/toolkit/image_edit.rs`)
  — accepts `image` + `operations[]` + `output_format`. Operations
  supported: `crop`, `draw_rect`, `draw_circle`, `draw_line`,
  `draw_arrow`, `draw_polygon`, `draw_text`, `blur_rect`. Each
  operation is validated before any pixel is touched
  (bounds / zero-area / radius > 0 / polygon ≥ 3 vertices).
- **Encode pipeline with alpha handling** — PNG preserves alpha
  verbatim; JPEG composites onto white when the source has any
  transparency (JPEG has no alpha channel); WebP attempts native
  encode with a PNG fallback (full cursor reset, not just `clear()`).
- **Output writer** — atomic write (temp + rename on same FS) to
  `data/images/<uuid>.<ext>`. Filenames are UUID-based (122 bits
  entropy) → unguessable → enables immutable HTTP cache. Path
  traversal protected via canonicalize + starts_with on a
  `current_dir().join()` base (avoids the macOS `/tmp` →
  `/private/tmp` → `/var/` blocklist trap).
- **`url` field on result** — the tool returns
  `"/api/images/<uuid>.png"` so the LLM can embed it in markdown
  replies (`![annotated](/api/images/foo.png)`), and the browser
  fetches via the new public route.
- **`GET /api/images/:filename`** (`crates/neomind-api/src/handlers/images.rs`)
  — public route (intentional: markdown `<img>` cannot carry auth
  headers). Safety: `is_safe_filename()` rejects `/`, `\`, `..`,
  leading dots, null bytes; alphanumeric + `_-_-.` only; extension
  whitelist (png/jpeg/webp/jpg). Symlink defense via canonicalize +
  starts_with. 30-day immutable cache headers (`Cache-Control:
  public, max-age=2592000, immutable`).
- **`$cached:user_image` integration** — chat-uploaded images are
  stored in `LargeDataCache` under the `user_image` key. The tool
  description teaches the LLM to pass `$cached:user_image` as the
  `image` argument; `resolve_cached_arguments` resolves the
  reference to the full base64 data URL at call time.
- **Privacy gate on auto-inject** — when the LLM omits the `image`
  field entirely, defense-in-depth auto-inject from cache fires
  **only** for tools in `IMAGE_AWARE_TOOLS` (`image_edit`, `vision`).
  Prevents user-uploaded images from silently leaking into
  `file_write` / `shell` / extension tools that log args verbatim.
  Per-arg inject path (when the LLM does pass `image`) is unchanged.
- **Single-call pipeline (no chaining)** — the description
  explicitly discourages multi-call chaining; `operations_applied`
  + `status: "completed"` fields in the result signal to the LLM
  that the work is done in one call.

---

### Earlier-prepared changes (UX polish)

A **UX polish patch**: dashboard manual ordering lands as the first
citizen of the dashboards model (sortable sidebar + tab bar), and
sensitive inputs across the app get a consistent show/hide toggle.

Themes: (1) **dashboard manual ordering** — `sort_order` field +
batch reorder API + icon-based controls; (2) **password show/hide** —
reusable `PasswordInput` component rolled out across all sensitive
inputs.

### Dashboard manual ordering

Dashboards have until now rendered in storage iteration order, which
for UUID-keyed redb tables is effectively random. Users with many
dashboards had no way to pin frequently-used ones at the top. This
release adds an explicit ordering column end-to-end.

- **`Dashboard.sort_order: Option<i32>`** (`neomind-storage`) with
  `#[serde(alias = "sort_order")]` so existing rows lacking the field
  deserialize cleanly. New dashboards are appended at
  `max_sort_order() + 1`.
- **`DashboardStore::set_sort_orders(&[(id, order)])`** — single
  transaction batch update, same pattern as `set_default()`.
- **`PUT /api/dashboards/reorder`** — body `{ dashboard_ids: [...] }`,
  response `{ ok, count }`. Emits `DashboardUpdated` with
  `action: "reorder"` so other clients sync via SSE.
- **List ordering** — `list_dashboards_handler` now sorts by
  `sort_order.unwrap_or(i32::MAX)`; legacy rows fall to the bottom in
  stable order.
- **Frontend slice** — `reorderDashboards(newOrder)` does an optimistic
  update, calls `recordSelfSync` for every affected id (SSE echo
  suppression), and rolls back on API failure.
- **Icon-only controls (no drag)** — per user request, reordering is
  surfaced exclusively via `ChevronUp`/`ChevronDown`:
  - Sidebar (`DashboardListSidebar`) — buttons in the hover action group
  - Tab bar (`DashboardTabBar`) — items in the active tab's `⋮` menu
    (desktop) and in the mobile dropdown switcher
- **DTO round-trip** — `sortOrder` (camel) ↔ `sort_order` (snake)
  flows through `fromDashboardDTO` / `toDashboardDTO` per the dashboard
  conversion invariant.

### Password show/hide toggle

Sensitive text inputs across the app (login, setup, broker, push
targets, LLM API key, message channel secrets, BLE WiFi, plugin
schema-driven fields) used plain `<Input type="password">` with no
way for the user to verify what they typed. This release introduces a
single reusable component and rolls it out everywhere.

- **`<PasswordInput>`** (`web/src/components/ui/password-input.tsx`) —
  wraps the existing IME-safe `Input` primitive with an
  `Eye`/`EyeOff` toggle button. Ref-friendly (forwardRef), so existing
  `editInputRef.focus()` patterns keep working. Labels resolve via the
  globally-loaded `auth` namespace (`showPassword` / `hidePassword`).
- **Applied to 9 locations**: login page, setup admin account, BLE
  WiFi password, data-push webhook + MQTT passwords, LLM backend API
  key, message channels (email password, bearer token, basic auth
  pass, API key value, SMTP pass, Telegram bot token, webhook secrets
  ×2), embedded broker password, plugin schema password fields.
- **Skipped** `InstanceManagerDialog.tsx` — that field has a custom
  `ShieldCheck` validation indicator anchored at the same position the
  eye would occupy; combining the two would require restructuring the
  overlay layout (out of scope for a toggle).

### Backwards compatibility

- Existing `dashboards.redb` files without `sort_order` load cleanly;
  those dashboards sort to the bottom until reordered.
- `Input` primitive behavior unchanged — the password path still
  disables IME composition (Tauri/WebKit garbled-display fix).
- No DB migration required.

---

## [0.9.0]

### Overview

A **chat-agent quality release**: a Python eval harness driving the
real `neomind serve` subprocess (146 cases, zh+en, Claude Opus 4.6 as
judge), the production gaps it surfaced, an agent-prompt evolution
(CLI reference → skills, response-format calibration, tool-toggle
parity), per-command extension tool management, and DashScope
hybrid-thinking reliability. Baseline eval: ~91% → expected ~95%+.

Themes: (1) **eval framework** — Python + production WS path; (2)
**eval-surfaced production fixes** — template seeding, `--id` flag,
rule trigger default, extension build producing `.nep`; (3) **agent
prompt evolution** — CLI reference moved to skills, fragment
consolidation, response calibration, multi-step narration; (4)
**extension tool management** — per-extension + per-command toggles,
disabled-filter across all LLM paths, tool registry rebuild on
install/uninstall; (5) **DashScope thinking reliability** — cloud
backend honors `thinking_enabled`, tool-loop streams for thinking
models; (6) **external broker parity** — `$SYS` presence synthesis.

### Eval framework

Rewritten in Python (`eval/run_eval.py` + `eval/lib/`). Each case
spawns `neomind serve` with a temp data dir, pre-seeds an API key +
LLM backend, and drives the chat agent through the production
WebSocket pipeline (multi-round ReAct, list-only-dead-end detection,
same system prompts as the chat UI). The previous in-process Rust
runner bypassed all of this and silently masked multi-tool failures.

Coverage: 146 cases (73 unique × zh+en) across every CLI domain
(device, dashboard, rule, agent, message, transform, llm, extension,
widget, system, tools, connector, push, settings). Judge scores
`tool_accuracy` / `task_completion` / `response_quality` /
`language_adherence`. Latest run: 91.1% PASS pre-fix.

### Eval-surfaced production fixes

- **First-boot template seeding** — `DeviceRegistry::new()` now seeds
  built-in device-type templates (NE101, NE301…) before loading the
  in-memory cache. Fresh installs no longer fail device registration
  with "template not found" until restart.
- **`neomind device create --id`** — optional flag (alias
  `--device-id`) preserving user-supplied IDs; previously silently
  swapped for auto-UUIDs.
- **`POST /rules` trigger default** — absent `trigger` now defaults to
  `data_change` (aligns API with skill doc; agent was cycling through
  four wrong shapes).
- **Template-metric rule validation** — `build_validation_context()`
  pulls metrics from the registered device-type template instead of
  hardcoding "temperature"/"value".
- **`neomind extension build` produces a `.nep`** — reads
  `manifest.json`, packages cdylib + binaries + optional frontend
  into `<id>-<version>.nep`, emits `NEP_PATH=<path>` for deterministic
  parsing.
- **`settings` domain in CLI help** — `shell.rs` domain table now
  lists timezone/retention/cleanup so the agent uses
  `neomind settings timezone` instead of host OS commands.

### Agent prompt evolution

- **CLI reference moved to skills** — the `shell` tool description
  dropped from 15.7 KB to ~2.5 KB (-84%, ~26 KB/turn saved). Per-domain
  syntax now sourced exclusively from `skills/builtins/*.md` (no more
  drift between the two copies).
- **Prompt fragments consolidated** — seven inline `const` blocks
  merged into a single `system_prompt.md` with conditional
  `BEGIN_VISION`/`BEGIN_THINKING` sentinels; deleted `rules.md`
  (duplicated elsewhere); trimmed cross-file redundancies (~1 KB /
  10% per turn).
- **Response format calibration** — replaced rigid 3-pattern rules
  with adaptive guidance (quick answer / action result / comparison /
  analysis / tutorial) + explicit table discipline; added
  "Calibrate effort to task" (complex → search skills first, simple →
  just do it).
- **Multi-step narration + completion self-check** — 3+ entity tasks
  lead with a one-line intent statement; before declaring done, replay
  the original request against actual work done.
- **Memory instruction corrected** — standard files (user/knowledge/
  procedures) are auto-injected into the system prompt; the old
  instruction told the LLM to waste a tool call re-reading them.
- **Error recovery rule** — fix the root cause then RETRY the
  original command (not stop after the side fix).
- **UTF-8 safe slicing** in base64 heuristics (was panicking on
  Chinese text at byte boundaries).

### Extension tool management

- **Per-extension + per-command toggles** — two new endpoints
  (`PATCH /api/extensions/:id/enabled`,
  `PATCH /api/extensions/:id/commands/:cmd/enabled`) hide tools from
  the LLM at either granularity, live (no restart). Persisted to
  `extensions.redb` (`ExtensionRecord.enabled`,
  `disabled_commands`).
- **Disabled-filter covers all LLM paths** — chat agent now uses
  `definitions_for_llm()` (was iterating all tools, ignoring disabled
  set); defense-in-depth `is_disabled()` guard added to
  `ToolRegistry::execute()`. Mid-session toggles take effect next
  message.
- **Tool registry rebuild on install/uninstall/reload** —
  `refresh_extension_tools()` is now called in all 7 lifecycle
  handlers so new tools are visible to the LLM without a server
  restart.
- **CLI invoke endpoint fix** — `invoke_agent` was calling `/execute`
  (async) instead of `/invoke` (sync, returns results).
- **Tools catalog** — `GET /api/agents/tools` read-only endpoint +
  Tools tab on the Agents page render the runtime tool registry
  (name, description, source, namespace, JSON Schema, params). UI
  shows disabled tools with a muted tint + `Disabled` badge.
- **Extension card** — on-card AI-tools switch replaced with a
  compact `AI off` footer badge (stable height, no alignment drift);
  dialog now derives live state from store instead of open-time
  snapshot.

### DashScope hybrid-thinking reliability

Two related fixes for qwen3.7-plus cloud agents failing mid-execution
with "Network error" or "malformed output":

- **Cloud backend honors `thinking_enabled`** — `ChatCompletionRequest`
  gains `enable_thinking: Option<bool>` (Qwen-only, others skip). Gotcha
  #7 was silently ignored on cloud paths; memory extraction /
  compression calls no longer burn tokens on hidden chain-of-thought.
- **Tool-loop streams for thinking models** — thinking-capable
  backends now route through `generate_to_completion` (streaming) so
  the reasoning phase can't trip the gateway idle timeout. Non-thinking
  backends unchanged.

### External MQTT broker parity

External brokers (EMQX / Mosquitto) now synthesize
`DeviceTransportOnline/Offline` from `$SYS/brokers/+/clients/+/
{connected,disconnected}` broadcasts, closing the gap with the
embedded broker's `DevicePresenceHook`. Devices on external brokers no
longer show as "never connected" in the 4-state UI. User config is
untouched (filters appended at adapter creation); harmless on brokers
that don't publish `$SYS`.

### Agent reliability

- **Transient skill dedup** — reloading the same skill N times in one
  turn (a retry-loop pattern) no longer appends N copies into the
  system prompt (was drowning the agent in ~50 KB of dupes).
- **Degenerate code-fence guard** — DeepSeek-class models occasionally
  emit just ` ``` ` as their entire answer; now detected and recovered
  via the retry-without-thinking path.
- **Scheduled prompt unified** — Free-mode data freshness (`Age`
  column), error-path journal entries (failed executions write
  `success: false` so the agent learns from failures), event-trigger
  callout section.
- **Agent status auto-recovery** — `Error → Active` sweep on restart
  alongside the existing `Executing → Active` (Error agents were
  silently dropped across server restarts).
- **Cooperative cancellation** for scheduled execution.

### Marketplace, mobile, community

- **Marketplace component reinstall + update detection** —
  `GET /api/frontend-components/updates`, refresh button re-downloads
  marketplace bundles, update badge on newer versions.
- **Mobile dashboard masonry** — desktop 12-col grid stacks
  single-column on phone viewports.
- **Onboarding docs strip** — pinned BookOpen + 3 wiki links in the
  setup step.
- **Discord community + release-notify Action** — README badge +
  `release-notify.yml` posts to `#announcements` on release.

### Upgrade notes

- **External broker users**: no action required; `$SYS` filters are
  appended automatically.
- **Agents in Error state**: first startup after upgrade sweeps
  `Error → Active` and reschedules. Check logs for "Reactivating
  agents in Error status at startup".
- **Knowledge file trim**: agents with >20 knowledge files drop oldest
  FIFO (in-memory index only; orphaned markdown files on disk are NOT
  auto-deleted).

---

## [0.8.25] - 2026-06-26

### Overview

A **mobile-only PWA hardening + UI polish** release, all frontend /
desktop-app side. No backend changes. The headline work closes two
long-standing iOS PWA standalone bugs (header offset under the notch
when the keyboard opened, chat input floating mid-screen instead of
sitting on the keyboard) and redesigns the pending-devices mobile card
to be flat, single-action (tap the card to approve), and visually
balanced. Around that: ResponsiveTable gains three new opt-in props so
other list pages can adopt the same flat-card style, and the button
focus ring is dropped (it was firing on every WebKit tap-to-focus and
reading as random orange edges).

Themes: (1) **iOS PWA viewport** — drive layout from `visualViewport`
instead of `innerHeight`/`100dvh` (which PWA standalone ignores when
the keyboard opens), lock `html { overflow: hidden }` to stop
document-level keyboard-avoidance scroll; (2) **pending device card
redesign** — drop the 3-dot menu (single approve action → tap card
directly), flatten the header, move status to the top-right slot,
collapse bottom meta into one line; (3) **ResponsiveTable extensions**
— `renderMobileBody` / `mobileFlatHeader` / `renderMobileHeaderExtra`
props for per-table mobile card customization without forking the
shared chrome; (4) **button focus ring** — removed; form inputs keep
their focus ring.

### iOS PWA viewport / keyboard

- **`html { overflow: hidden }`** (`web/src/index.css`). The layout
  viewport in iOS PWA standalone does NOT honor
  `interactive-widget=resizes-content` like Safari does — it stays
  full-screen when the soft keyboard opens, so iOS silently scrolls
  the document root scroller by ~status-bar height for keyboard
  avoidance. `position: fixed; top: 0` headers rode along with that
  scroll and ended up under the notch. `body` already had
  `overflow: hidden`; locking `html` the same way is a no-op for
  Safari / Android and blocks the PWA-only offset.
- **`--app-height` driven from `visualViewport.height`**
  (`web/src/hooks/useVisualViewport.ts`). `innerHeight` and `100dvh`
  don't shrink on PWA keyboard open; `visualViewport.height` always
  does. The `.viewport-full` utility now resolves to
  `var(--app-height, 100dvh)`, bringing keyboard-aware sizing to
  login / full-screen pages without per-page changes.
- **`--visual-viewport-offset-top`** (same hook). Once document scroll
  was blocked, iOS PWA fell back to scrolling the visual viewport
  itself, so `position: fixed; top: 0` no longer meant "top of the
  visible area." Exposed `visualViewport.offsetTop` as a CSS variable
  and bound the chat root's `top` to it — the input now sticks to the
  top of the keyboard instead of floating mid-screen.
- **Chat root** (`web/src/pages/chat.tsx`) uses
  `top: var(--visual-viewport-offset-top, 0px)` and
  `height: var(--app-height, 100dvh)`. Unmount cleanup blurs any
  focused textarea so the keyboard is gone before the next page
  mounts.
- **MobileNav drawer** (`web/src/components/layout/MobileNav.tsx`)
  blurs the active element on drawer open — gives the keyboard the
  drawer's open animation to dismiss before navigation, otherwise the
  next page would render in the shrunk viewport with content under
  the notch. Also: removed the `startTransition` wrapper around
  `navigate` (the deferred route change made taps feel non-responsive
  and prompted a second tap that interrupted the first), switched the
  nav list from Radix `ScrollArea` to native `overflow-y-auto`
  (Radix's pointer-event handling swallowed taps during momentum-scroll
  settle on iOS).
- **Defensive route-change reset** (`web/src/App.tsx`) keeps
  `window.scrollTo(0, 0)` + body / documentElement transform clears
  on every route change. Redundant with `html { overflow: hidden }`
  but cheap.

### Pending devices card redesign

- **Single-action card** (`web/src/pages/devices/PendingDevicesList.tsx`).
  The 3-dot menu was the only action surface but there was only one
  action ("approve"). Replaced with `onRowClick` — tap the card, the
  approve dialog opens directly. Removes the menu trigger chrome and
  the tap-target tax of "open menu → tap item."
- **Flat header** via the new `mobileFlatHeader` prop. The card used
  to be split into a `bg-muted` header band (title) + bordered body,
  which read as two stacked surfaces and felt heavy on a list of
  similar items. With `mobileFlatHeader`, the header band / border /
  rounded-top go away — title and body share one continuous surface.
- **Status badge in top-right slot** via the new
  `renderMobileHeaderExtra` prop. The empty space where the 3-dot menu
  used to live is now occupied by the status badge, balancing the
  card's visual weight. Body's bottom row drops from "status +
  source · time" to just one secondary line.
- **Single bottom meta line**. `device_type` code (when analysis is
  done) and `source · time` are now on one row: code pinned left,
  source/time pinned right with `ml-auto`. Reads as one context
  strip rather than two stacked muted lines.
- **Toned-down confidence**. Changed from a `bg-success-light` pill
  badge to plain `text-success` text so the status badge remains the
  only strong color signal on the card.

### ResponsiveTable extensions

`web/src/components/shared/ResponsiveTable.tsx` gains three opt-in
props so individual tables can tailor their mobile cards without
forking the shared Card chrome, header, or actions menu:

- **`renderMobileBody`** — replaces the default key-value list with
  a caller-supplied layout. Use this when the default produces
  asymmetric content (multi-line cells, centered badges, mixed cell
  shapes in one row).
- **`mobileFlatHeader`** — drops the `bg-muted` band and the border
  under the header so the header and body read as one continuous
  surface. Use when the body already provides enough visual structure.
- **`renderMobileHeaderExtra`** — extra content in the top-right of
  the card header, in the same slot as the actions menu (hidden when
  actions are present). Useful for surfacing a status badge or
  chevron when the table has no row actions but the right side would
  otherwise be empty.

### UI polish

- **Button focus ring removed** (`web/src/components/ui/button.tsx`).
  The previous `focus-visible:ring-2 ring-ring ring-offset-2` rendered
  a bright brand-orange halo whenever a button held keyboard focus or
  matched WebKit's tap-to-focus heuristics on mobile, which read as
  random orange edges on icon / ghost buttons. Buttons already
  communicate state via `hover:bg-*` and `active:scale-[0.97]`,
  matching native mobile patterns where buttons don't have focus
  rings. Form inputs (input / textarea / select) keep their focus
  ring — that's where keyboard focus indication is genuinely needed.
- **Global `--ring` token neutralized** (`web/src/index.css`).
  Replaced `--ring: var(--brand)` with a 35% foreground tint in both
  light and dark themes. The brand-orange ring made every focused
  input / tab / button flash a bright orange halo — even after the
  button-level ring removal above, the global token still drove ring
  color for inputs, tabs, and any component using `ring-ring`. The
  neutral tint preserves WCAG focus-indicator contrast for keyboard
  a11y without the brand-color noise.

## [0.8.24] - 2026-06-26

### Overview

A **data-management hardening + cross-cutting fix** release. The headline
work is a rework of the telemetry retention cleanup pipeline: the image-data
short-retention rule now detects image content by inspecting the actual
datapoint value (base64 magic bytes) instead of matching metric names, and
the cleanup path itself gains concurrency dedup, batched deletion, and
async HTTP triggering to handle million-point backlogs safely. Around that,
a batch of mobile / desktop UX fixes land: row-click detail dialogs across
Rules / Messages / Devices, the Tauri updater no longer re-prompts after a
successful install, and several long-standing theme / safe-area / i18n
bugs are closed.

Themes: (1) **retention overhaul** — content-based image detection +
cache-bypass + concurrency guard + batched deletion + async trigger;
(2) **list-page interactions** — click a row to open a detail dialog
(Rules, Messages, Devices) with richer metadata; (3) **agent reliability** —
transient LLM errors now retry instead of marking the agent Error, plus
event-trigger dedup covers image-vs-regular and cross-channel overlap;
(4) **theme & frontend polish** — light-mode tokens aligned with shadcn /
Vercel conventions, `--error-foreground` token (was missing → black text
on red), mobile chat bar transparency, Skills mobile card declutter;
(5) **Tauri updater race fix** — the "update successful" dialog no longer
re-appears on the just-installed version; (6) **iOS PWA safe-area** —
top padding / headers now clear the notch.

### Data retention overhaul

- **Content-based image retention** (`crates/neomind-storage/src/timeseries.rs`).
  The previous keyword fallback (`"image"`, `"frame"`, `"snapshot"`, …) was
  too narrow — it missed metrics like `payload` / `data` / `sample` that
  carry base64 image blobs, and false-positived on names like `framerate`.
  Replaced with `value_looks_like_image()`, which decodes the first 32 chars
  of the latest datapoint's value and checks for image magic bytes (JPEG
  `FF D8 FF`, PNG `89 50 4E 47`, GIF `GIF8`, WebP `RIFF`, BMP `BM`) or a
  `data:image/` URL prefix. Priority unchanged: explicit metric_overrides →
  device_type_overrides → image_retention (content-based) → default_hours.
- **Bypass LRU cache during retention walks**
  (`query_latest_uncached()`). `apply_retention()` walks every metric pair
  to peek at the latest value; calling the cached `query_latest()` for each
  would populate `latest_cache` (capacity 1000, TTL 60s) and evict the hot
  entries users are actively querying. The new uncached helper skips both
  cache read and cache write, leaving the hot window untouched.
- **Concurrency dedup** for `apply_retention()`. The hourly background task
  and `PUT /settings/retention` can both fire simultaneously; without a
  guard they'd race, doing duplicate full-table scans + N
  `query_latest_uncached` calls and piling on redb's single-writer lock.
  Added a `retention_in_progress: AtomicBool` gated by an RAII
  `RetentionGuard` that clears the flag on every exit path (success, error,
  panic). A concurrent caller observes the flag and returns immediately.
- **Batched deletion** in `delete_range()`. The previous implementation
  collected every key into a `Vec<(String, String, i64)>` and removed them
  in one giant `write_txn`. For a metric with millions of expired points
  this meant O(N) memory (~100 bytes/key), a single txn held open for
  minutes starving other writers, and WAL bloat. Now deletes in batches of
  1000 keys per committed txn. Partial failure is tolerable — deletion is
  idempotent, the next hourly pass picks up where this one left off.
- **Async retention trigger**
  (`crates/neomind-api/src/handlers/settings.rs`).
  `trigger_retention_cleanup` was awaiting `apply_retention()` inline on
  the HTTP path. With a large backlog this blocked the response for
  minutes, causing frontend timeouts and retry storms. The handler now
  spawns the cleanup in the background and returns `{triggered: true}`
  immediately. The in-progress flag dedupes against the concurrent hourly
  run.

### List-page interactions

- **Row-click detail dialogs** across Rules, Messages, and Devices. Clicking
  a row opens a detail dialog instead of requiring the action menu.
  - `feat(web/automation)`: `RuleDetailDialog` — click a rule to see full
    condition / actions / trigger config + paginated execution history.
  - `feat(web/messages)`: message row → quick-open detail dialog.
  - `feat(web/devices)`: richer `DeviceDetail` metadata + row-click to open.
  - `refactor(web/automation)`: `RuleDetailDialog` migrated to
    `UnifiedFormDialog` + paginated history (was ad-hoc layout).
  - Desktop `ResponsiveTable` in `RulesList` gained `onRowClick` parity
    with mobile.

### Agent reliability

- **Retry transient LLM errors** (`crates/neomind-agent`). Network blips
  and rate-limit responses now trigger an inline retry with backoff instead
  of immediately failing the execution. Transient failures no longer flip
  the agent to `Error` status — the agent stays `Active` so the scheduler
  keeps firing on the next tick. `is_transient_failure` gained unit tests.
- **Event-trigger dedup** — two related fixes:
  - Event-triggered image collection vs regular collection no longer
    double-fire (prefix-match dedup).
  - Cross-channel data collection (metric vs device overlap) no longer
    duplicates work when the same source is bound via multiple channels.

### Theme & frontend polish

- **Light-mode token alignment** (`refactor(web/theme)`). `--background`,
  `--muted`, `--secondary`, `--accent` retuned to shadcn / Vercel
  conventions (`oklch(0.985 0 0)` canvas, `oklch(0.97 0 0)` muted) so
  surfaces separate cleanly. Table headers upgraded to the Strong label
  style (`text-[11px] uppercase tracking-wider text-foreground`). Mobile
  page header switched to the `--chrome` token with `text-base` title.
- **`--error-foreground` token** — was missing entirely, causing black
  text on red error surfaces. Defined the token, added
  `error.foreground` to the Tailwind config, and swept residual
  `hsl(var())` references in `DashboardGrid` + `CustomLayer`.
- **Mobile chat input bar** — transparent `backdrop-blur` instead of the
  solid glass background that clashing with the page chrome.
- **Skills mobile card declutter** (`fix(web/skills)`). The SkillsPanel
  card packed icon + name + raw category text + up to 3 keyword tags all
  into the `bg-muted` card header, producing a tall top-heavy card with
  near-invisible muted-on-muted keyword tags. Split into focused columns:
  name (icon 36→32px + name only), category (colored Badge from the
  previously-unused `categoryConfig`), keywords (own column, renders on
  the white card body where contrast is correct).

### Tauri updater race fix

- **Update dialog no longer re-appears after a successful install**
  (`fix(update)`). `localStorage.setItem('neomind_installed_version')` was
  running AFTER `await invoke('download_and_install')`. On macOS / Windows
  Tauri's updater can trigger a process restart or webview reload the
  moment `download_and_install` resolves — so the marker write never
  executed, the next launch found no marker, fell through to
  `check_update`, and re-showed the dialog on the just-installed version.
  Two-layer fix: (1) frontend pre-writes the marker BEFORE the invoke and
  clears it on install failure; (2) backend `normalize()` now splits on
  `+` / `-` so `0.8.24+build.1` / `0.8.24-beta` match `0.8.24` — a
  last-resort safety net when the marker is lost entirely.

### iOS PWA safe-area

- **CSS variable scoping bug** (`fix(web)`). `--topnav-height` and
  `--chat-content-padding-top` were declared bare (no selector) inside an
  `@supports` block. Bare custom-property declarations have no selector to
  attach to, so they silently never applied — `var()` downstream fell back
  to the hardcoded `4rem`, causing PWA content to overflow the top safe
  area on iPhone X+ notches. Moved both into an explicit `:root {}` rule.
- **Header safe-area adoption** — `login.tsx` and `SetupHeader.tsx` headers
  gained `safe-top` so the back-button row clears the notch.
- **App.tsx** main padding fallback now mirrors the real token
  (`calc(4rem + env(safe-area-inset-top))` instead of plain `4rem`).

### i18n

- **Stop leaking hardcoded Chinese via `plugin_name`**
  (`fix(devices)`). `get_plugin_info()` in `crud.rs` was emitting
  `"内置MQTT"` / `"外部MQTT: …"` directly into the API response. English-
  locale users saw Chinese broker names regardless of their UI language.
  Backend now returns stable English identifiers; `DeviceDetail` maps
  `adapter_id` to localized labels via `t('brokerInternalMqtt' |
  'brokerExternalMqtt')`.

### Other

- **Device last_seen fallback** — after a server restart, in-memory
  `last_seen` is 0; the API now falls back to the registry value so
  devices don't momentarily appear as "never seen".
- **Tauri `Cargo.lock` sync** — `base64 0.22.1` added to the Tauri
  lockfile (workspace already had it for content-based image detection).

## [0.8.23] - 2026-06-25

### Overview

A focused **visual polish & frontend hardening** release. No new backend
features; the bulk of the work is a multi-pass audit of the web layer that
enforces the design-system rules in `web/DESIGN_SPEC.md`, iterates the chat
composer's look-and-feel, and tightens agent cloud-LLM timeouts so thinking
models stop timing out mid-tool-loop.

Themes: (1) **chat composer redesign** — a single unified input container
with iterated model-selector, capability badges, and send/cancel button
states; (2) **light-mode chrome unification** — buttons, inputs, and overlay
surfaces now sit on solid `bg-card` rather than the translucent page
background, killing the "everything-fuses-into-the-page" effect; (3) **design
hard-rule enforcement** — `/opacity` on CSS-variable colors, raw Tailwind
palette, emoji, and inline SVGs all swept out (20+ sites); (4) **agent
runtime** — `reasoning_content` field support and 60s → 300s cloud LLM
timeout for thinking models; (5) **share-proxy** whitelist expansion with
regression tests; (6) **animations** — page-enter transition, overlay popup
depth, and ambient `animate-pulse` removal from steady states.

### Chat composer redesign

- **Single unified input box** (`pages/chat.tsx`, `components/chat/`).
  Replaced the old split layout with one container that holds both the
  textarea and the toolbar row (model selector + image upload + send /
  cancel). Removed focus-within `border-primary` ring in favor of a calmer
  solid-`bg-card` surface.
- **Send / cancel buttons — circular, clearer states.** Swapped the old
  two-row rectangle buttons for a single circular primary action that flips
  between send (paper-plane) and cancel (square) states. Eliminated the
  ambiguous "two-button row" UX.
- **Model selector — single-line inline layout** with capability text
  labels (`Vision` / `Tools` / `Thinking`) instead of icons. Iterated
  through five variations (two-row, badge-style, filled icons, muted-bg
  icons) before landing on the compact text-label form. Removed the `Zap`
  indicator and other visual noise.
- **Image delete button** — shrank to a 12px circle (`h-3 w-3`, icon
  `h-1.5`), placed inline top-right, hover-only on the button itself
  rather than the whole thumbnail. Deepened background for contrast.
- **Image upload icon** `h-4.5 → h-4` to better match the toolbar's visual
  weight.
- **Dead code**: deleted `components/chat/ChatInput.tsx` — unused orphan
  file after the unified-container refactor.

### Light-mode chrome unification

- **Buttons / inputs**: `bg-background → bg-card` across the design-system
  primitives so form controls visually separate from the page background.
- **Light-mode chrome → solid white.** Killed the translucent layered
  backgrounds that made cards, popovers, and dropdowns melt into the page
  on light mode. Dashboard empty state rewritten to match.
- **Overlay surfaces — removed border.** All popovers, dropdowns, and
  hover-cards now rely on shadow + tone contrast instead of a hairline
  border, eliminating the "stacked rectangles" look.
- **Code editor token-ized** (`components/ui/code-editor.tsx`). Refactored
  raw hex values to design tokens; removed unintended gray fills that
  appeared on light theme.
- **Dashboard empty state** — replaced the ad-hoc gray card with the
  proper `EmptyState` component used elsewhere.

### Design-spec hard-rule enforcement

- **`/opacity` on CSS-variable colors** (silent failure). Tailwind's `/N`
  modifier silently produces no style when applied to a `var()` color
  reference. Swept **20+ sites** across `BuildCard`, `ToolCallVisualization`,
  `PushTargetDialog`, `AddDeviceGlobalDialog`, `TaskProgress`,
  `InstallComponentDialog`, `BleProvisionTab`, `AgentDetailPanel`,
  `PanelChatView`, `GlobalChatFab`, `DeviceTransformsDialog`, `DeviceDetail`.
  Each fix either drops the modifier or reworks the style with
  `hover:opacity-N` over a solid color.
- **Emoji → `lucide-react`**. Replaced inline custom SVGs with `lucide`
  imports in `ConnectionStatus.tsx` (4 SVGs), `ChatContainer.tsx`
  (checkmark), `ExtensionUploadDialog.tsx` (alert), `ComponentRenderer.tsx`
  (alert-triangle), `CustomLayer.tsx` (2 X icons). All icons now route
  through `@/design-system/icons` mapping.
- **`animate-pulse` removed from steady states** (`AgentCard.tsx`,
  `ExtensionGrid.tsx`). `animate-pulse` is reserved for transient
  placeholders; on Active/Error status icons and "running" extension dots
  it produced a christmas-tree effect in grids. Kept `animate-spin` for
  the genuine Executing / loading state.
- **i18n gaps** — `ConnectionStatus.tsx` had two hardcoded Chinese strings
  (`尝试 {retryCount}/10 · {nextRetryIn}s` and `重新连接`). Added
  `retryProgress`, `retrySeconds`, `reconnect` keys to both `zh/chat.json`
  and `en/chat.json`.

### Agent runtime

- **Cloud LLM timeout 60s → 300s** (`crates/neomind-agent/src/llm_backends/`).
  Thinking models (qwen3.7-plus, deepseek-r1, etc.) emit a long
  `reasoning_content` phase before the final reply; the previous 60s
  ceiling aborted mid-thought on the scheduled-execution tool-loop path,
  surfacing as `error sending request for url`. 300s aligns with the
  existing 5-minute wall-clock execution cap.
- **`reasoning_content` field support** (`openai.rs::ApiMessageResponse`).
  Alibaba / DashScope "视觉推理" hybrid thinking models return both
  `delta.reasoning_content` (chain-of-thought) and `delta.content` (final
  reply) in OpenAI-compatible mode. The response struct only had `content`
  + `tool_calls`, so tool-loop decisions emitted during the reasoning phase
  were silently dropped, triggering `malformed_output` false positives.
  Added `reasoning_content: Option<String>` and surface it ahead of
  `content`.
- **Orphan-tag malformed-output false positive** — relaxed the malformed
  detector so a trailing `<tool_call>` fragment left over from
  reasoning-mode streaming doesn't trip the validation.

### Notifications & login polish

- **Notification dropdown redesign** (`components/topnav/`). Removed the
  severity left-bar from each item (the colored dot already conveys
  severity), tightened density, fixed badge color contrast.
- **Login background simplification** (`pages/login.tsx`, `pages/setup/`).
  Eight stacked gradient layers → four, restoring legibility on low-end
  displays and fixing the muddy look in dark mode. Language switcher
  selected state moved from `bg-muted` → `bg-primary-light` for proper
  accent contrast.

### Animations & micro-interactions

- **Page-enter transition** for route changes. New `animate-fade-in` on
  the routed page wrapper — opacity-only, **no `translateY`**, so drawer
  and side-sheet layouts don't visually jump on mount.
- **Overlay popup depth** — refined the open animation on popovers /
  dropdowns so the shadow grows instead of fading, conveying elevation.
- **Dropdown item spacing** — added `my-0.5` between sibling dropdown
  items so the hover background doesn't fuse adjacent rows into one
  colored block. Item corners `rounded-sm → rounded-md` for clearer
  separation.

### Devices

- **List column rename** (`pages/devices.tsx`). The "Last Activity"
  column was misleading for customers whose devices use MQTT LWT rather
  than data publishes. Renamed to **最近上报 / Last Report** (zh / en) to
  match what the column actually shows (last telemetry publish time).

### API — share-proxy hardening

- **Whitelist expansion + regression tests** (`handlers/share_proxy.rs`).
  Expanded the share-dashboard proxy whitelist to cover the additional
  endpoints surfaced by recent frontend additions, and added table-driven
  regression tests so a future endpoint-add doesn't silently regress the
  security boundary.

## [0.8.22] - 2026-06-23

### Overview

Four themes: (1) **iOS PWA keyboard + chat UX** — closing the keyboard-overflow-
under-notch regression that affected every iOS PWA user on notched devices, plus
a follow-up that extends the same fix to mobile full-screen Radix Dialogs; (2)
**PWA icon & splash overhaul** — transparent-background icons sourced from the
original `logo-square.png`, real iOS launch screens matching the Tauri startup
visual, and removing the `maskable` declaration so desktop Chrome stops applying
its squircle mask; (3) **agent runtime capability refresh** — fixing the
"malformed tool-call output" incident where stale `supports_multimodal=true`
rows in `llm_backends.redb` caused text-only models to be sent `image_url`
parts on the scheduled-execution path; (4) **device command payload
pipeline overhaul** — a new JSON-aware template renderer, system-vs-user
parameter separation (`request_id` auto-injection, `fixed_values`
merging), verbatim adapter publishing, NE301 template corrections, and
auto-onboarding hygiene that stops the embedded broker from treating our
own outbound publishes or device LWT broadcasts as phantom discovered
devices.

### PWA — iOS keyboard handling

#### `--keyboard-offset` CSS variable (`useVisualViewport.ts`)
- **Problem**: iOS PWA standalone mode does **not** shrink `window.innerHeight`
  when the soft keyboard opens — only `visualViewport.height` does. The previous
  `body.keyboard-open` rule locked body to `var(--initial-viewport-height)`,
  which kept the layout full-screen-tall while iOS shifted it upward to reveal
  the focused input, pushing the safe-area-padded header under the notch.
- **Fix**: introduce `--keyboard-offset` CSS variable. On iOS PWA standalone
  it tracks the actual keyboard height; on every other platform (Android,
  iOS Safari browser, desktop, Tauri) it stays `0px`. This avoids the
  double-subtract regression where `100dvh - var(--keyboard-height)` would
  collapse body on Android — there `100dvh` already shrinks on its own.
- **`--app-height` also shrinks**: same conditional logic applied so the
  root container tracks the visible area on iOS PWA instead of full screen.
- **`ios-pwa-standalone` class** added to `<html>` for CSS targeting.
- **`detectIOSPwaStandalone()`** helper checks `display-mode: standalone`
  (and legacy `navigator.standalone`) plus iOS UA, including iPad's
  `MacIntel + maxTouchPoints > 1` quirk.

#### Fixed-bottom elements offset (`bottom-[var(--keyboard-offset,0px)]`)
- Four previously `bottom-0` elements now lift with the keyboard on iOS PWA:
  - `pages/chat.tsx` — chat input container
  - `components/chat/ChatInput.tsx` — standalone ChatInput
  - `components/layout/PageLayout.tsx` — page footer
  - `components/shared/PageTabs.tsx` — bottom tab navigation
- `components/chat/GlobalChatFab.tsx` — FAB + expanded panel both offset
  via `bottom-[calc(...+var(--keyboard-offset,0px))]`.

#### Dynamic viewport units (`index.css`)
- **`@layer utilities` override**: `.h-screen` / `.min-h-screen` / `.max-h-screen`
  now resolve to `100dvh` with `100vh` fallback. Fixes the iOS Safari browser
  mode where `100vh` includes the address bar.
- **iOS PWA-specific body rule** (`html.ios-pwa-standalone body.keyboard-open`):
  `height: calc(100dvh - var(--keyboard-offset, 0px))` shrinks body to the
  visible area when the keyboard is open — eliminates the upward shift that
  hid the header under the notch.
- **Mobile full-screen Dialog rule**: same calc-height override applied to
  `[role="dialog"][data-state="open"][style*="safe-area-inset-top"]`. The
  attribute selector matches only the mobile branch of `dialog.tsx` (the
  only path that injects safe-area padding as inline style), so the desktop
  centered dialog is untouched. Specificity `(0,3,0)` beats Tailwind
  `.h-full` `(0,1,0)` even with `!important`, so the calc wins and the
  `sticky bottom-0` form footer rises above the keyboard instead of being
  hidden behind it. Affects every form dialog on mobile — `UnifiedFormDialog`,
  `EditDeviceDialog`, `LLMBackendConfigDialog`, `ChannelEditorDialog`,
  `PushTargetDialog`, setup `AccountStep`, etc.
- **Touch device hover visibility** (`@media (hover: none) and (pointer: coarse)`):
  `group-hover:opacity-100` and `group-hover:opacity-50` are forced to 1/0.55
  on pure touch devices, restoring visibility of the 20+ hover-only buttons
  that were invisible on phones/tablets.

#### Chat UX polish
- **Auto-scroll pinned-to-bottom** (`pages/chat.tsx`):
  - `isPinnedToBottomRef` tracks whether the user is near the bottom.
  - Auto-scroll only fires when pinned. Scrolling up to read history no
    longer yanks the user back.
  - Re-pin on `handleSend` (so user sees their new message + AI reply) and
    on session switch (so opening a session shows the latest content).
- **Mobile textarea max-height** (`pages/chat.tsx`, `ChatContainer.tsx`):
  `max-h-[100px]` on mobile (was `max-h-40` = 160px), with JS clamp
  matching. Prevents the textarea from eating the conversation view when
  the user pastes a long block.
- **Dialogs**: `max-h-[85dvh]` / `max-h-[calc(100dvh-2rem)]` on
  `components/ui/dialog.tsx`, `alert-dialog.tsx`, `dialog/UnifiedFormDialog.tsx`
  so dialogs fit within viewport on small screens with browser chrome.

### PWA — Icons & splash screens

#### Transparent-background icons
- All 5 icons (`icon-192.png`, `icon-512.png`, `apple-touch-icon.png`,
  `favicon-16x16.png`, `favicon-32x32.png`) regenerated from the original
  `logo-square.png` with its natural alpha channel preserved — no canvas
  color fill, no `#1A1A1F` tinting.
- **Why transparent**: the previous `#1A1A1F` canvas plus interior tinting
  produced a muddy dark-gray icon that didn't match the brand. With
  transparency, the OS surfaces the wallpaper/window behind the icon
  corners naturally.
- Removed orphan `public/logo.png` (512×512 file, zero references in code).

#### iOS PWA splash screens (`apple-touch-startup-image`)
- Five static PNG splash screens generated from the Tauri `StartupLoading`
  visual (solid black background + horizontal logo, no React/JS execution
  possible on iOS launch screen):
  - `splash-1290x2796.png` — iPhone 14/13/12/11 Pro Max, XS Max
  - `splash-1179x2556.png` — iPhone 14/13/12/11 Pro, XS, X
  - `splash-1284x2778.png` — iPhone 14 Plus / 14 / 13 / 12 / 11 / XR
  - `splash-750x1334.png`  — iPhone 8 / 7 / 6s / 6 / SE
  - `splash-2048x2732.png` — iPad Pro 12.9"
- 9 `<link rel="apple-touch-startup-image">` tags added to `index.html`
  with device-specific media queries (`device-width` + `device-height` +
  `-webkit-device-pixel-ratio`).
- Logo sized to 45% of canvas width (cap 540px), centered. Bg `#000000`.

#### Manifest & theme color
- **`site.webmanifest`**: `background_color` and `theme_color` changed
  from `#1a1a1f` to `#000000` (matches the black icon/splash canvas;
  previous gray showed as a visible ring on dark wallpapers).
- **`index.html`**: dark-mode `<meta name="theme-color">` also `#000000`.
  Light-mode stays `#f7f7f7`.
- **`maskable` purpose removed** from webmanifest icons. Previously the
  same PNG was declared both `any` and `maskable`, which told desktop
  Chrome/Edge "feel free to apply your squircle mask" — producing the
  "桌面icon 圆角严重" user complaint. With only `any` purpose declared,
  browsers now display the icon as-is (square).
- **macOS caveat**: macOS itself applies a squircle mask to all icons in
  Dock/Launchpad at the OS level; that one we can't bypass from the web
  layer. Other platforms (Windows, Linux, Chrome OS) now show the
  intended square icon.

### Backend — Agent runtime capability refresh

#### Symptom
`LLM tool-calling produced malformed output` on a scheduled agent using
DeepSeek-V4. The tool-call stream returned unparseable fragments instead
of the expected `tool_call` blocks.

#### Root cause
`crates/neomind-agent/src/ai_agent/executor/llm_runtime.rs::get_llm_runtime_for_agent`
loaded backend rows straight from storage and trusted the persisted
`supports_multimodal` field verbatim. A stale row from before layered
capability detection shipped (0.8.20) had `supports_multimodal=true` on
a text-only DeepSeek backend. The chat path refreshed capabilities on
every load via `instance_manager`, so chat reported it as text-only
correctly — but the scheduled-agent path didn't, so the executor:

1. Detected the backend as multimodal → kept the `vision` tool available.
2. The LLM emitted `image_url` content parts (which the tool layer happily
   forwarded) in a text-only API request.
3. DeepSeek's text endpoint rejected the unknown `image_url` variant,
   causing the streaming tool-call parse to fail with malformed fragments.

#### Fix
- **`ensure_instance_capabilities` promoted to `pub(crate)`** in
  `crates/neomind-agent/src/llm_backends/instance_manager.rs`. Chat and
  agent-runtime paths now both go through this single refresh entry point.
- **`llm_runtime.rs::get_llm_runtime_for_agent`** calls
  `ensure_instance_capabilities(backend)` before building the cache key,
  so a stale DB row is corrected to current layered-detection output
  (registry → heuristic) before the multimodal decision is made.
- **User override preserved**: `multimodal_user_override` remains sacred
  in both paths. Refresh never clobbers a user override.

#### Regression tests
- `test_ensure_instance_capabilities_refreshes_stale_text_model` —
  verifies a DeepSeek-V4 row with stale `supports_multimodal=true` is
  downgraded to `false`.
- `test_ensure_instance_capabilities_respects_user_override` —
  verifies user override wins over auto-detection.

### Backend — Shared dashboard proxy

- **`is_share_proxy_path_allowed`** (`handlers/dashboards.rs`) now allows
  `frontend-components/` GETs through the share proxy. Community widget
  manifests and JS bundles are needed for rendering shared dashboards
  that use community widgets. Install/uninstall endpoints remain blocked
  by the existing method check.

### Upgrade notes

- **iOS PWA users**: delete the existing home-screen icon and re-add it.
  iOS caches web clip icons aggressively; the new transparent-background
  icon won't appear until the cache is invalidated.
- **Desktop PWA users** (Chrome/Edge): uninstall via `chrome://apps`,
  clear browser cache for the site, then re-install. Chrome caches PWA
  icons inside `~/Applications/Chrome Apps.localized/<App>.app/Contents/
  Resources/app.icns` and does **not** refresh that file when the source
  manifest changes — only on first install.
- No data migrations.
- No breaking API changes.

### Device command payload pipeline

A focused overhaul prompted by real-world NE301 field reports where
`capture` failed to render (`placeholder ${request_id} was not given a
value`) and the MQTT downlink topic could not be configured from the UI.

#### New structured renderer (`crates/neomind-devices/src/payload_template.rs`)
- Replaces five classes of `str.replace` bug — placeholder syntax drift,
  quote collision, type erasure, JSON injection, reactive validation —
  with a JSON-aware tree walker.
- **4-phase pipeline**: (1) state-machine scan rewrites `${name}` into
  `__PH:name__` sentinels while preserving JSON validity; (2) `serde_json`
  parse; (3) recursive tree walk replacing sentinel leaves; (4) reserialize
  as compact JSON.
- **Typed substitution** via `MetricValue` variants — Integer/Float/
  String/Boolean/Null/Array all preserve their JSON types. Binary values
  are rejected (`RenderError::BinaryUnsupported`).
- **Quote-insensitive**: `"${var}"` and `${var}` produce identical typed
  output — template authors can keep or omit quotes for readability.
- **Non-JSON fallback** for legacy bare-string payloads (HASS-style
  `ON`/`OFF`), bypassing the JSON path entirely.
- **13 unit tests** including NE301 protocol contract tests.

#### `service.rs::build_command_payload`
- Now merges `command_def.fixed_values` (template-declared constants the
  user never sees) under user-supplied params before rendering. User params
  win on key collision. Previously the production path ignored
  `fixed_values` entirely.
- **`request_id` auto-injection**: when a template references
  `${request_id}` but neither user nor `fixed_values` supplied one, the
  service mints `req-<uuid>`. Templates therefore no longer need to
  declare `request_id` in `parameters` — it is pure system plumbing and
  should never surface in a UI form. Three regression tests cover the
  merge, injection, and NE301 contract scenarios.

#### `adapters/mqtt.rs::send_command`
- Publishes the already-rendered payload **verbatim**. The previous
  implementation re-parsed the rendered string as `HashMap<String, Value>`
  and re-serialized — destroying bare-string payloads via
  `unwrap_or_default()` collapse to `{}` and randomising key order via
  HashMap iteration.
- Deleted the dead `send_command_mqtt` method (~80 lines).

#### Deletions and delegations
- `protocol/mqtt_mapping.rs::render_payload_template` now delegates to
  `payload_template::render`; updated test to expect compact JSON
  (`{"action":"set","interval":60}`).
- Deleted dead `MdlRegistry::build_command_payload` (~70 lines, zero
  callers, same `${{var}}` double-brace bug as the legacy service path).

#### NE301 template aligned with real device protocol
- `crates/neomind-storage/src/builtin_types/ne301_camera.json`:
  - **Capture** (`{"cmd": "capture", "request_id": "${request_id}"}`):
    zero parameters. Removed fabricated `enable_ai` / `chunk_size` /
    `store_to_sd` fields the device silently ignored. Removed `request_id`
    from `parameters` (auto-injected by the service).
  - **Sleep** (`{"cmd": "sleep", "request_id": "${request_id}", "params":
    {"duration_sec": ${duration_sec}}}`): only `duration_sec` user-facing
    parameter. Removed `request_id` from `parameters`.
  - Verified against the real NE301 wire format provided by the device
    vendor.

#### Auto-onboarding hygiene
- **Self-echo suppression**: new `outbound_command_topics:
  Arc<RwLock<HashSet<String>>>` field on `MqttAdapter`. `send_command`
  inserts the resolved topic before each publish; the inbound handler
  checks membership and skips auto-onboarding on hit. Fixes the log-spam +
  phantom-discovery pattern where every `capture`/`sleep` publish was
  reflected by the embedded broker back through the wildcard subscription,
  generating `Triggering auto-onboarding for non-standard topic:
  ne302/2819FD/down/control` plus a bogus discovered-device row.
- **LWT/status broadcast filtering**: new helper
  `looks_like_non_telemetry_topic(topic)` returns true for topics
  containing a `status` segment or ending in
  `online|offline|connected|disconnected|lwt|will` (near-universal LWT
  signatures). Fixes the field observation where `aicam/status/offline`
  (NE301's MQTT LWT) was being parsed as `device_id=status, is_binary=true`
  and registered as a phantom device.
- **Filter ordering fix**: the self-echo and LWT/status filters now run
  AFTER the `topic_to_device` mapping lookup, not before. Previously, a
  registered device whose telemetry topic contained a `status` segment
  or ended with `online`/`offline` (common in real IoT firmware) had its
  telemetry silently dropped by the filter — the device list showed stale
  `last_seen` and no status updates even though the device was actively
  publishing. Registered-device telemetry now always reaches the extraction
  pipeline regardless of topic naming; the filters only apply to unknown
  topics entering the auto-onboarding path.
- Regression tests `test_lwt_and_status_topics_skip_auto_onboarding` and
  `test_status_topic_filter_is_aggressive_by_design` document the contract.

#### Device detail page showed "从未上线" after server restart
- `get_device_handler` and `get_device_current_handler` read
  `device_status.last_seen` directly from the in-memory status map.
  After a server restart this map is empty, so every device got
  `last_seen=0` → `last_seen=null` in the JSON response → frontend
  rendered `disconnected` ("从未上线") even for devices that were
  previously online with persisted `config.last_seen` in redb.
- Both handlers now use the same `effective_last_seen` logic as the
  list handler: prefer `config.last_seen` (persisted), fall back to
  `device_status.last_seen` (in-memory). After restart, devices with
  a real persisted timestamp correctly show `offline` instead of
  `disconnected`.

#### Heartbeat TOCTOU race — stale mark overwrites fresh reconnection
- The heartbeat monitor ran in two phases: (1) scan all devices and
  collect stale ones into a vector, (2) mark each stale device offline.
  Between these phases, a `DeviceMetric` event could arrive and set the
  device back to `Connected` with a fresh `last_seen`. Phase 2
  unconditionally overwrote the status to `Disconnected` and fired
  `DeviceOffline` — so the frontend list received `DeviceMetric`→online,
  `DeviceOnline`→online, then `DeviceOffline`→offline (last event wins).
  The device showed offline in the list despite just having sent data.
- Phase 2 now re-checks each device's current `last_seen` under a read
  lock before marking it offline. If the device received fresh data
  after the scan phase (`entry.last_seen > stale_last_seen`), the
  offline mark is skipped entirely.

#### Dead config field removed
- `EmbeddedBrokerConfig.connection_timeout_ms` was defined (default
  60000) but never passed to the rmqtt builder — it was dead code from
  an earlier design iteration. Removed from the config struct, its
  default function, the `Default` impl, the config.toml loader, and the
  broker config DTO. rmqtt uses its own internal keep-alive enforcement
  (1.5× client-declared keep-alive via `keepalive_backoff=0.75`).
- The background heartbeat loop in `service.rs` previously collapsed any
  `Connected` device whose `last_seen` exceeded the effective offline
  timeout to `Disconnected`, even when the MQTT session itself was still
  alive (as tracked by `DevicePresenceHook` → `transport_connected`).
  This silently threw away the transport signal: the in-memory status
  diverged from reality, and the device had to fully re-establish its
  `Connected` state on the next telemetry tick instead of seamlessly
  recovering.
- The stale-check now also requires `!status.transport_connected`, so a
  device with an alive MQTT session but no recent data stays in
  `Connected`. The DTO layer still reports `online=false` (because
  `is_connected_within` checks `last_seen`), and `transport_connected`
  continues to flow through to the frontend, which correctly renders the
  `connectedIdle` state. Only when the broker actually fires
  `ClientDisconnected` (keep-alive timeout, TCP RST, etc.) does
  `transport_connected` flip to `false` and the heartbeat then collapses
  the status to `Disconnected`.

#### Frontend — command topic always configurable
- `EditDeviceDialog`, `AddDeviceDialog`, `AddDeviceGlobalDialog`: removed
  the `{hasCommands && ...}` gate around the `command_topic` input.
  Previously the downlink-topic field disappeared whenever the frontend's
  cached device-types list didn't include the target type — even when the
  device and protocol supported commands — leaving the user unable to
  configure a downlink channel from the UI.
- The `hasCommands` flag is retained in the Edit/Add dialogs purely to
  drive the **auto-fill convenience** (defaulting `command_topic` to
  `device/{type}/{id}/downlink` only when commands exist); the field
  itself is now always visible.
- Added `commandTopicHint` help text explaining the field semantics.

#### Frontend — command-parameter UX refactor
- New `ParameterForm` component iterates a command's parameters with
  consistent grouping, conditional visibility, and validation hints.
- New `ParameterInput` renders a single parameter with type-appropriate
  control (text, number, select, textarea, checkbox).
- New `seedCommandDefaults` initialises parameter values from declared
  defaults instead of the previous per-type fallback ladder.
- New `parameterExpr` minimal evaluator for `ParameterDefinition.
  visible_when` conditional rendering.
- `CommandButton` (dashboard widget) refactored onto the new
  `ParameterForm`/`seedCommandDefaults` pipeline; deleted inline default
  seeding and ad-hoc param synthesis.
- `DeviceDetail` command-sending surface refactored to match.
- Added `ParameterGroup` type to `web/src/types/device.ts`.
- i18n: added `selectValue`, `binaryPlaceholder`, `allParametersFixed`,
  `generalGroup`, and `range.{min,max}` keys (en + zh).

#### Command payload pipeline — upgrade notes
- **Device-type templates referencing `${request_id}`** in their
  `payload_template` no longer need to declare it in `parameters`. The
  service auto-injects `req-<uuid>`. Existing templates that still
  declare `request_id` in `parameters` will continue to work — the user
  value (if supplied) wins; otherwise auto-injection fills it in.
- **Device-type templates with `fixed_values`** now actually see those
  values merged into the rendered payload on the production path.
  Previously `fixed_values` was honoured only by a dead code path.
- **NE301 `ne301_camera` builtin**: the corrected template is seeded into
  `devices.redb` only on fresh databases. Existing deployments must
  re-import the type via **Import from Cloud** to pick up the zero-param
  `capture` command.

### Post-release — device transport & status consistency

- **Broker learns `client_id → device_id` mapping from publish topics.** When a
  device uses an MQTT `client_id` that differs from its NeoMind `device_id`
  (e.g. a camera with a hardcoded client `NE302-000000` registered as
  `2819FD`), transport connect/disconnect events previously fired for the raw
  client_id and were silently dropped by the frontend's status updater. The
  `DevicePresenceHook` now also hooks `MessagePublish`, resolves the publish
  topic to a registered device via `DeviceRegistry::find_device_by_telemetry_topic`,
  and caches the mapping. All subsequent `ClientConnected` / `ClientDisconnected`
  events for that client_id carry the correct NeoMind `device_id`, so
  `transport_connected` actually toggles for these devices and the 4-state UI
  (`online` / `connectedIdle` / `offline` / `disconnected`) works as designed.
  Falls back to the legacy passthrough when the registry is empty or the topic
  is unknown.
- **Detail-page fetch now propagates fresh status to the list cache.**
  `fetchDeviceDetails` and `fetchDeviceCurrentState` previously only wrote to
  `deviceDetails` / `deviceCurrentState`, leaving `state.devices` (the list
  cache) untouched. With a 10s `fetchCache` TTL, returning from the detail
  page to the list within that window showed stale `online` / `last_seen`.
  Both fetchers now merge the fresh `online`, `status`, `last_seen`,
  `transport_connected`, and `transport_changed_at` into the matching
  `devices` entry, so the list shows consistent state immediately.
- **`connectedIdle` label renamed to "连接中·待机" / "Connected·Standby".**
  The previous "已连接·空闲" ("Connected·Idle") read as "the device is doing
  nothing", confusing users into thinking it was unhealthy. "Standby"
  correctly conveys that the MQTT session is alive and the device is ready
  to accept commands.
- **External broker config dialog now warns about transport status
  limitation.** When NeoMind connects to an external MQTT broker (not the
  embedded one), it cannot detect device MQTT session state, so the 4-state
  model degrades to 3-state. A schema-driven notice banner in the adapter
  config dialog explains this at creation time, rather than building a costly
  `$SYS` / HTTP-API presence sync that external-broker users don't need.

## [0.8.21] - 2026-06-22

### Overview

Two themes: (1) **webhook URL resolution correctness** — closing the loop on 0.8.20's
server-URL work with proper reverse-proxy discrimination, a canonical frontend hook,
and memory safety for multipart payloads; (2) **device offline alert rule** — exposing
`__last_seen_age_secs` as a virtual metric so users can build "alert if no telemetry
for N hours" rules without touching device configs.

### Security — Webhook hardening (0.8.20 follow-ups)

#### Reverse-proxy header discrimination (`handlers/common.rs`)
- **`Host` header alone no longer trusted** (`resolve_server_url`): every HTTP request
  carries `Host`, and its value just echoes whatever the client typed in the URL. The
  previous logic treated `Host: localhost:9375` from a browser/curl as a reverse-proxy
  signal and returned `http://localhost:9375` as the canonical webhook URL — which
  broke display for every device-facing surface (Tauri desktop, browser-via-SSH-tunnel).
  New logic requires an explicit reverse-proxy indicator (`X-Forwarded-Proto` **or**
  `X-Forwarded-Host`) before `Host` is trusted. Falls through to LAN-IP auto-detection
  otherwise, bringing webhook URL resolution in line with MQTT broker IP display.
  Closes the "Host spoofing leaks canonical URL" concern raised in the 0.8.20 audit
  comment block.
- **`X-Forwarded-Host` now honored** alongside `X-Forwarded-Proto`. Either is sufficient
  to mark the request as reverse-proxied. `X-Forwarded-Host` takes precedence over raw
  `Host` when both are present (nginx `proxy_set_header X-Forwarded-Host $host` pattern).
- **Tests**: added `test_resolve_server_url_host_alone_not_trusted` and
  `test_resolve_server_url_forwarded_host_alone_trusted` to lock in the discriminator.

#### Memory amplification caps (`handlers/devices/webhook.rs`)
- **`DefaultBodyLimit::max(8 MB)`** on webhook routes: was axum default (2 MB, broke
  typical 1080p JPEG uploads) then 16 MB (no upper bound on memory amplification).
  8 MB accommodates a 1080p JPEG with headroom while blocking 4K raw uploads; paired
  with the per-value guard below, caps total memory amplification at ~40 MB per
  concurrent request (base64 inflation + pipeline clones).
- **Per-value string size guard** (`MAX_VALUE_STRING_SIZE = 2 MB`,
  `enforce_max_string_size`): pathological JSON with a single 10 MB base64 string
  would have been accepted by the body limit but still cloned ~5× through the
  telemetry pipeline. The guard rejects oversized scalar strings before they enter
  the EventBus. Content-Type-aware: `image/*` uploads and multipart parts bypass
  the guard since they encode binary intentionally and are already bounded by the
  body limit.
- **RFC 2046 boundary validation** (`is_valid_boundary`): rejects malicious
  boundaries that could hijack the multipart parser (tspecials, whitespace, control
  chars, length > 70).
- **Multipart part count cap** (`MAX_MULTIPART_PARTS = 64`): prevents memory
  exhaustion from pathological multipart payloads with thousands of parts.
- **Multipart parser** (hand-rolled, not `axum::extract::Multipart`): tolerates
  CRLF/LF mix, missing part headers, missing `name=` field. Supports camera devices
  that POST `multipart/form-data` with `metadata` (JSON) + `image` (JPEG) parts.
- **Error response sanitized**: catch-all no longer leaks `e.to_string()` — returns
  generic `ErrorResponse::internal("Webhook processing failed")`.

#### Authentication hardening (`adapters/webhook.rs`)
- **Constant-time comparison for `api_key` and `webhook_token`** (new helper
  `constant_time_eq`): replaces direct `==` to close the timing-attack surface.
  Length-mismatch early exit is preserved (industry standard; libsodium does the
  same). Helper duplicated locally rather than imported cross-crate from
  `neomind-api::auth::constant_time_eq_str` to avoid breaking layering.
- **`update_device_handler.offline_timeout_secs`**: confirmed direct assignment
  (NOT `.or(existing)`) — this is intentional so JSON `null` clears the override.
  Audited as correct; documented in CLAUDE.md gotcha #5.

### Frontend — Canonical server URL (`lib/server-url.ts`, new)

- **`useServerUrl()` hook** (React 18 `useSyncExternalStore`): replaces
  `getServerOrigin()` in 7 webhook DISPLAY call sites. Browser mode returns
  `window.location.origin` directly when non-localhost (the URL the user typed is
  exactly what devices should use); falls through to backend consultation only
  when the user is accessing via `localhost`/`127.0.0.1` (SSH-tunnel case) or in
  Tauri desktop mode. `getServerOrigin()` is preserved for fetch/WS calls where
  `localhost:9375` is correct (frontend→backend on the same machine).
- **`prefetchServerUrl()` warm-up** in `App.tsx`: module-level cache populated on
  app mount, so by the time any webhook dialog opens the LAN IP is already
  resolved — no localhost flash on first render. Cross-component subscription via
  `useSyncExternalStore` ensures all displays update atomically when the prefetch
  resolves.
- **`/api/system/network-info` extended** (`handlers/basic.rs`): response now
  includes `server_url` and `server_url_source` from `resolve_server_url(headers)`,
  alongside the existing `ssid` and `ip` fields.

### Added — Device offline alert rule
- **New virtual metric `device:<id>:__last_seen_age_secs`**: enables rules that
  fire when a device has had no telemetry for a configured duration. A 60s
  background task (`DeviceStatusEmitter`) refreshes the metric for every device
  currently referenced by a rule subscription. The emitter pushes `0` while
  the device is online (age < `effective_offline_timeout`) and the actual age
  once offline — so a rule like `age > 300` only starts firing when the device
  is genuinely considered offline by the platform. Validator enforces ≥60s
  cooldown (matches the emitter tick) for virtual-metric rules; production
  guidance is 5 min – 1 h to avoid alert fatigue. The rule UI adds a
  "设备离线告警 / Device offline alert" template (default 12h, Critical severity)
  for one-click setup, plus a "System metrics / 系统指标" group in the rule-builder
  metric dropdown exposing `__last_seen_age_secs`.
  See `docs/superpowers/specs/2026-06-22-device-offline-rule-design.md`.
- **`test_rule_handler` computes `__last_seen_age_secs` on-demand**: the 60s
  emitter may not have ticked yet when a user tests a freshly-created rule.
  The handler now computes the current age inline (same semantics: 0 while
  online, actual age once offline) so users can test rules immediately.

### Added — `__webhook_image` system metric (fault-tolerant image fallback)
- **Problem**: NE301/NE302 cameras upload images via webhook multipart, but the
  first image part was only aliased to `{image_data, frame, snapshot}`. If the
  device-type template's image metric used a different name (or had no image
  metric at all), the uploaded image silently disappeared into `_raw` and was
  unrecoverable in the frontend.
- **Fix**: multipart parser now aliases the first image part to
  `__webhook_image` in addition to the existing names. The unified extractor
  learns `SYSTEM_PASS_THROUGH_KEYS` — keys with the `__` prefix that bypass
  template matching and are always extracted if present. Result: any
  webhook-uploaded image is always recoverable via
  `device:<id>:__webhook_image`, regardless of device-type template naming.
  Mirrors the existing `__last_seen_age_secs` convention.
- **Null-guard regression fix**: `extract_by_path` returns
  `Ok(Some(Value::Null))` for missing keys (legacy semantics), so the
  pass-through explicitly skips null values — otherwise every webhook payload
  would synthesize a phantom `__webhook_image: null` metric.

### Tests
- 9 webhook multipart / memory-guard unit tests (all passing).
- 2 `resolve_server_url` tests covering the Host-only rejection and
  X-Forwarded-Host-only acceptance paths (23 total in `handlers::common::tests`).
- 1 `constant_time_eq` test covering equal / differing / empty / length-mismatch paths
  (11 total in `adapters::webhook::tests`).
- 2 new unified_extractor regression tests for the `__webhook_image`
  pass-through (present-case + null-guard phantom-rejection), 10 total in
  `unified_extractor::tests`.
- All 83 `neomind-devices` lib tests pass (webhook, registry, service, telemetry,
  unified_extractor).
- All 25 `neomind-rules` lib tests + 2 offline-rule integration tests pass.

## [0.8.20] - 2026-06-22

### Overview

Three themes: (1) **webhook security hardening** — wiring adapter-level IP/API-key controls that were silently no-ops, rate-limiting inbound POSTs, throttling discovery events, and correctly resolving the server URL behind HTTPS proxies; (2) **mobile layout redesign** for settings & drill-down views; (3) **PWA polish** plus a long tail of mobile UX fixes (pagination, header overflow, dialogs). No breaking API changes; no new runtime dependencies.

### Security — Webhook

#### Dead-code controls wired to real values
- **`validate_request` always no-op** (`crates/neomind-devices/src/adapters/webhook.rs` + `crates/neomind-api/src/handlers/devices/webhook.rs`): the handler forwarded `(None, None)` for both `X-API-Key` and remote IP, making the adapter-level API key check and IP allow/block lists dead code. Now forwards the real `X-API-Key` header and remote IP from `ConnectInfo<SocketAddr>`. *(4aab3bbc)*
- **`ConnectInfo<SocketAddr>` silently None app-wide** (`server/mod.rs`): server bound without `into_make_service_with_connect_info`, so every `Optional<ConnectInfo>` extractor degraded to None and every IP-based control was a no-op. Now enabled globally. *(4aab3bbc)*
- **`get_webhook_url_handler` was public** (`server/router.rs`): leaked device existence (404 vs 200) and the configured `NEOMIND_SERVER_URL` to unauthenticated callers. Moved to protected_routes. *(4aab3bbc)*

#### Rate limiting & discovery throttle
- **Webhook POST rate limit** (`server/middleware.rs` + `router.rs`): webhook routes now live in a dedicated `webhook_routes` block behind `webhook_rate_limit_middleware`. The composite `client_id` includes `device_id` from the URL path, so devices sharing an adapter API key still get independent buckets. *(4aab3bbc)*
- **Per-IP discovery throttle** (`adapters/webhook.rs::process_webhook`): default 30/min (configurable via `discovery_rate_per_minute`). Caps `DeviceDiscovered` emissions to stop auto-onboard / LLM amplification when attackers rotate `device_id`s. Telemetry metrics still process when the cap is hit — only the event is suppressed. *(4aab3bbc)*

#### Server URL resolution behind proxies
- **Hardcoded `http://localhost:9375` behind HTTPS proxy** (`handlers/common.rs::resolve_server_url`): webhook URLs returned by `/api/devices/:id/webhook-url` and `neomind system info` were always `http://` when `NEOMIND_SERVER_URL` was unset. Behind an HTTPS reverse proxy (typical prod), devices hit a 301 from nginx and either silently dropped the POST body on redirect or failed outright. New 3-tier priority chain: `NEOMIND_SERVER_URL` env > `X-Forwarded-Proto` + `Host` headers > localhost fallback. Response now includes a `url_source` tag (`env | proxy_header | fallback`) plus a `hint` field for fallback cases. *(663e94df)*
- **`neomind system info` CLI** (`cli-ops/src/system.rs`): surfaces `url_source` alongside `network.api_url` and `device_connection.webhook.url`, plus a top-level `url_hint` with operator guidance pointing at both `NEOMIND_API_BASE` (CLI side) and `NEOMIND_SERVER_URL` (server side). *(663e94df)*
- **`.env.example`**: documents the priority chain and the HTTPS deployment gotcha. *(663e94df)*

#### Tests
- **3 webhook unit tests** (`tests/webhook_*.rs`): `validate_request` IP/API-key enforcement, per-IP discovery throttle (caps `DeviceDiscovered` while metrics still process), and `resolve_server_url` priority chain (env > proxy_header > fallback). *(704eb8b5)*

### Mobile UI — Pagination & drill-down follow-ups

### Mobile UI — Pagination & drill-down follow-ups

#### Pagination
- **Skills panel mobile infinite scroll** (`web/src/pages/agents-components/SkillsPanel.tsx`): changing page on mobile now APPENDS new items (deduped by id) instead of replacing the list and scrolling back to page 1. Desktop keeps the replace + scroll-to-top behavior. Mirrors the canonical pattern already used in `messages.tsx` and `data-explorer.tsx`.
- **PushTargetsTab cumulative slice** (`web/src/components/datapush/PushTargetsTab.tsx`): applied the automation.tsx client-side cumulative slice `(0, page * pageSize)` on mobile so previous items stay visible. Previously page 2 replaced page 1, losing context.
- **Pagination count text hidden on mobile** (`web/src/components/shared/Pagination.tsx`): the `"共 N 条 / 第 x / y 页"` strip now uses `hidden md:block` — manual pagination inside dialogs has too little horizontal space.
- **Manual pagination inside FullScreenDialog**: `DeviceDetail` Metric History dialog and `DeliveryHistoryPanel` now pass `hideOnMobile={false}` so explicit page buttons always render. The mobile infinite sentinel relies on the outer page scroll container, which doesn't exist inside a FullScreenDialog — without the override the dialog got stuck on page 1.

#### Device detail
- **Header overflow with long names** (`web/src/pages/devices/DeviceDetail.tsx`): added the `min-w-0 flex-1` + `shrink-0` icon chain so long device names truncate instead of forcing horizontal scroll on mobile.
- **MobilePageHeader title override** (`web/src/pages/devices.tsx`): when a device detail view is open, the mobile header now shows the device name (falling back to "Device detail") via `mobileHeader.titleOverride`, matching the desktop breadcrumb. The duplicate inline back button is hidden on mobile (`hidden md:inline-flex`) since `leftExtra` already provides one.

#### Other mobile follow-ups
- **Grid overflow & truncate min-w-0** across 17 files (`e38c7432`): swept through all list/card layouts where flex children had implicit `min-width: auto` and pushed content past the viewport.
- **Mobile tabs**: agent/channel/extension dialog tabs now wrap instead of horizontal-scroll; extension dialog tabs get larger tap targets; mobile category tabs in `UnifiedDataSourceConfig` switch to a grid.
- **Unified design tokens, button scale, icon buttons** (`faaab85d`): design-token sweep to kill stray raw Tailwind palette usage that slipped through earlier audits.

### PWA

- **Install metadata** (`web/index.html`, `web/public/site.webmanifest`): added `theme-color` (with `prefers-color-scheme` light/dark variants), `application-name`, `apple-mobile-web-app-capable`, `mobile-web-app-capable`, `apple-mobile-web-app-title`, `format-detection`. Manifest gained `id`, `scope`, `display_override: ["window-controls-overlay", "standalone"]`, `lang`, `dir`, `categories`, and split the icons into separate `purpose: "any"` and `purpose: "maskable"` entries (some browsers reject combined-purpose icons). `background_color` switched to `#1a1a1f` to match dark mode.
- **Status bar style** (`web/index.html`): `apple-mobile-web-app-status-bar-style` set to `default` (not `black-translucent`). `black-translucent` overlays the webview behind the status bar, which dropped the top safe-area inset and made the sticky header appear transparent on iOS. `default` keeps the status bar opaque and the webview starts below it — more reliable given NeoMind's many sticky headers.
- **No new dependencies**: this release does NOT add `vite-plugin-pwa` or any PWA runtime package. All improvements are pure HTML/manifest meta + the existing backend-driven theme switching.

### Data Explorer

- **History table** (`web/src/pages/data-explorer.tsx`): replaced the raw `<table>` wrapped in a nested `<ScrollArea h-[400px]>` with `ResponsiveTable`. Quality column auto-hides when every row has null quality. Truncated values get a `title` tooltip. Added a count badge next to the "History" heading.
- **Time-range select** (`web/src/pages/data-explorer.tsx`): normalized the dropdown from `w-[110px] h-8 text-xs` to the standard `w-[140px]` (default `h-10 text-sm`) so it stops looking visually out-of-place next to sibling controls.

### Mobile UI — Sticky headers & card overflow

#### Sticky header background gaps
- **PageLayout scroll container** (`web/src/components/layout/PageLayout.tsx`): added `bg-background overscroll-none` so the iOS rubber-band bounce never exposes a transparent strip above the first child.
- **Sticky drill-down headers** (LLM / MQTT / Webhook): replaced the broken `-mt-2 pt-2` hack (which fails when the element is "stuck" — negative margin-top shifts the border-box DOWN, re-exposing the gap) with a `before:` pseudo-element that paints an 8px `bg-background` strip above the header. Works in both natural-flow and stuck states.
- **Desktop PageHeader strip**: added `bg-background` to the title wrapper and `headerContent` (tabs/buttons) wrapper so the title → tabs → scroll-container transition is visually seamless.

#### Card overflow on narrow screens
- **Multi-instance grids** (LLM backends, MQTT brokers, webhook adapters): changed `grid gap-4 md:grid-cols-2` to `grid-cols-[minmax(0,1fr)] md:grid-cols-2`. Grid items have implicit `min-width: auto`, so long broker URLs / instance names pushed cards wider than the viewport. `minmax(0,1fr)` allows the column to shrink, letting `truncate` + `min-w-0` work.
- **CardTitle truncate chain**: added `min-w-0` to the outer flex-1 wrapper, the title row, and the CardTitle itself in both UnifiedLLMBackendsTab and UnifiedDeviceConnectionsTab.

#### Redundant spacing
- **Settings page** (`web/src/pages/settings.tsx`): removed the mobile `<div className="pt-2">` wrapper around tab content. PageLayout's scroll container already adds `pt-2` on mobile; the double padding created a 16px gap that the sticky header's 8px `::before` couldn't cover.

#### Mobile drawer consolidation
- **MobileNav** (`web/src/components/layout/MobileNav.tsx`): removed Instance and About rows. Instance manager moved to Settings → About tab; About was already accessible from Settings. Theme/Language quick toggles retained. Drawer now focuses on navigation only.

#### Top-bar button consistency
- **chat.tsx** and **PageTabs.tsx**: unified all top-bar action button icons to `h-5 w-5` (20×20px). Previously chat.tsx used inline `style={{ width: 18, height: 18 }}` and MobileTabActionsCompact used raw icon sizes, creating visual inconsistency with the hamburger's `h-5 w-5`.

## [0.8.19] - 2026-06-18

### Overview

Multi-round security & reliability audit (rounds 3–19). 24 commits across 6 audit domains: agent executor, rules engine, LLM backends, storage, CLI ops, API auth, devices, messages, data-push, extensions. Several critical security fixes (zip-slip paths, public admin-register, broken channel-disable, WebSocket auth bypass), plus numerous crash-avoidance and concurrency hardening fixes. No breaking API changes; all fixes are drop-in.

### Security

#### Zip-slip / path traversal
- **Extension `install_sync`** (`crates/neomind-core/src/extension/package.rs`): manifest-controlled `binary_rel_path` and bundled-library paths were joined raw to `ext_dir`, allowing a malicious `.nep` to write binaries outside the install directory via `../` traversal. Now routed through `safe_join_within`. The async install path was already defended via `file_name()`; `install_sync` (used by `POST /api/extensions/upload/file`) was missed. *(Round 16, `7051afb2`)*
- **Extension `extract_directory` / `extract_directory_sync`** (same file): zip entry names were joined directly to `dst_dir`. A malicious community-marketplace `.nep` could ship `frontend/../../etc/cron.d/backdoor`. *(Round 3, `185d6c33`)* — central defense via `safe_join_within` with tests for plain traversal, mid-path `..`, absolute paths, and cur-dir no-op.

#### Public endpoints that shouldn't be
- **`POST /api/auth/register`** (`crates/neomind-api/src/handlers/auth_users.rs`): read `role` from the request body, so anyone on the network could self-register an admin JWT and bypass the admin-only `create_user_handler`. Now always creates `UserRole::User`; the `role` field is still accepted for backwards-compat with older clients but silently ignored. *(Round 16, `7051afb2`)*
- **`POST /api/setup/llm-config`** (`crates/neomind-api/src/handlers/setup.rs`): public endpoint that wrote the active LLM backend config (including API key) without checking `users.is_empty()`. After server boot, anonymous callers could redirect agent traffic to attacker-controlled endpoints or exfiltrate keys. Now gated by the same first-time-setup invariant used by `initialize_admin_handler`. *(Round 16, `7051afb2`)*
- **`x-internal-proxy: share` auth bypass** (`hybrid_auth_middleware`): the header alone granted User role, exploitable from the network because the server binds 0.0.0.0 by default. Now requires a second `x-internal-proxy-secret` header matching a fresh 32-byte per-process random secret. Constant-time compare. *(Round 3, `e179b0df`)*

#### Credential handling
- **Plaintext password fallback** (`auth_users.rs`): when bcrypt failed (password >72 bytes, RNG unavailable), `hash_password` silently degraded to `format!("fallback_hash_{}", password)` — cleartext storage, with `verify_password` doing direct string comparison. `hash_password` now returns `Result` and propagates errors; `verify_password` refuses legacy `fallback_hash_` entries and logs a critical warning (admin must reset affected accounts). *(Round 3, `185d6c33`)*
- **MQTT credential creation TOCTOU** (`add_credential_handler`): uniqueness check and insert ran as two separate redb transactions; concurrent requests for the same username both passed the check, second write silently overwrote the first credential's bcrypt hash while both callers got HTTP 200. New `try_add_mqtt_credential` does check-then-insert inside a single write transaction. *(Round 11, `defccf96`)*

#### Share proxy & dashboard auth
- **Share proxy allowlist** (`crates/neomind-api/src/handlers/dashboards.rs`): the previous blocklist silently allowed share-link holders to read every device, telemetry series, agent execution, and extension output. Switched to allowlist (telemetry/devices/extensions/agents/data-sources/messages read paths only); writes still blocked by method check. *(Round 15, `1f27e6e2`)*

#### Marketplace DoS
- **`market_install_handler` OOM** (`crates/neomind-api/src/handlers/extensions.rs`): called `.bytes().await` / `.text().await` on manifest/bundle responses with no size limit (only a 15s timeout). A compromised marketplace repo or tampered `NEOMIND_MARKET_URL` could serve a multi-GB payload buffered fully into memory. New `collect_capped` short-circuits on advertised `Content-Length > 10 MB` AND streams chunk-by-chunk to catch servers that omit/under-report length. *(Round 11, `ba9e1162`)*

### Critical Bug Fixes

#### Message channels
- **`set_enabled` didn't stop delivery** (`crates/neomind-messages/src/manager.rs:333`): `create_message` called `channel.is_enabled()` on the channel struct, whose internal `enabled` field is set once at factory create and never mutated by `set_enabled`. Disabling a channel via `PUT /channels/:name/enabled` kept delivery running while the UI reported it as disabled. New `ChannelRegistry::is_enabled_effective()` consults the registry override first; `create_message` uses it. *(Round 16, `7051afb2`)*
- **`set_enabled` wiped persisted filter** (`channels/mod.rs:493-501`): constructed a fresh `StoredChannelConfig` with `filter: ChannelFilter::default()`. Toggling enable silently turned any restricted channel into accept-all (since `get_filter` reads from disk on every send). Now preserves the existing filter. *(Round 16, `7051afb2`)*
- **Telegram CJK panic** (`channels/telegram.rs:111-115`): `&text[..4090]` sliced on byte index, panicking whenever a multi-byte character (Chinese, emoji) straddled the cut — taking down delivery for all subsequent channels in the loop. Now truncates on UTF-8 char boundary (`chars().take(4094)` + `…`). *(Round 16, `7051afb2`)*

#### Agent executor
- **`running_executions` slot leak** (`crates/neomind-agent/src/ai_agent/scheduler/...`): scheduler task inserted `agent_id` at the top but only removed it at a single trailing statement. Any intermediate exit (semaphore close, panic, cancellation) leaked the slot; after max_concurrent (10) leaks the scheduler silently skipped every future execution. RAII `RunningSlotGuard` removes via `Drop`. *(Round 12, `bc337d50`)*
- **Stale `Executing` agents on startup**: if the prior process died mid-execution (kill -9, OOM, crash, power loss), the agent row stayed in `Executing` because `StatusGuard` only fires on in-process drop. `reload_active_agents` filters by `Active` only, so such agents were silently dropped from the scheduler forever. New `reset_stale_executing_agents` runs once at startup and resets them to `Active` (not `Error` — environmental failure). *(Round 12, `af2e0d0a`)*
- **`update_memory` stale-snapshot overwrote failure journal** (gotcha #10): based write on `agent.memory.clone()` taken when agent was loaded. In the event-trigger retry path this snapshot overwrote the failure journal written by the prior attempt's outer `Err` branch. Now reloads latest memory from store before writing. *(Round 13, `50a61b70`)*
- **Empty event data skipped journal entirely**: `execute_internal`'s early return for empty event data skipped `update_memory`, leaving no trace while outer `Ok` counted the run as success in stats. Now writes `success:false` journal entry. *(Round 13, `50a61b70`)*
- **Prefixed metrics never triggered rules**: rule-engine subscription index only saw raw keys (`values.temperature`), rules authored against the stripped name (`temperature`) silently never fired. Mirror the prefix-stripping loop at the rule-engine trigger boundary. *(Round 13, `50a61b70`)*
- **`thinking_enabled` leaked into analytical LLM calls** (gotcha #7): `parse_intent_with_llm` (JSON extraction) and Phase 2 fallback summary inherited the backend's thinking mode, burning tokens on hidden chain-of-thought before a short response. Both now set `thinking_enabled: Some(false)`. *(Round 4, `d74e3898`)*

#### Extensions
- **Stuck extension process on command timeout** (`crates/neomind-extension-runner/...`): `execute_command` only cancelled the in-flight request tracker on Timeout; the extension process kept running, turning into a zombie where every subsequent command hit the same timeout. Now acquires the process lock and calls `kill_internal`, which restarts the extension cleanly via the death-monitor path. *(Round 9, `8b202505`)*
- **`restart_count` reset to 0 on crash-recovery reload**: crash-loop detection never accumulated — persistently crashing extensions (init panic, missing dep) were restarted forever instead of being disabled. `load()` now preserves `restart_count` and `last_restart_at` from any prior `info_cache` entry. *(Round 9, `6a184e24`)*
- **`cleanup_orphaned_runners` killed ALL instances' runners**: `pkill -f neomind-extension-runner` murdered every matching process system-wide — on multi-instance hosts, starting instance B killed all of instance A's live runners. Now scoped to processes whose PPID is 1 (kernel-reparented orphans). Skipped entirely when NeoMind itself runs as PID 1 (container init). *(Round 9, `6a184e24`)*

#### Devices
- **`unregister_device` left zombie MQTT subscriptions** (`crates/neomind-devices/src/service.rs`): only removed the registry entry; the adapter's topic subscriptions stayed on the broker. Messages kept arriving and getting discarded until connection drop or restart. `unregister_device` is now async and iterates adapters calling `unsubscribe_device` BEFORE registry removal. *(Round 9, `bff575ae`)*
- **MQTT adapter ignored custom `command_topic`** (`crates/neomind-devices/src/adapters/mqtt.rs:1333`): trait method received `_topic: Option<String>` but discarded it; `send_command_mqtt` always built `device/{type}/{id}/downlink`. Devices with non-standard command topics never received downlink commands. Now honors the device-configured topic. *(Round 16, `7051afb2`)*
- **Default-topic devices misrouted to discovery path** (regression from `e78df472`): default topic `device/{type}/{id}/uplink` was never inserted into `topic_to_device`, so `topic_to_device.contains_key(topic)` returned false for every default-topic device — registered devices were misrouted to discovery, re-triggering `DeviceDiscovered` indefinitely. *(Round 0, `8c158111`)*
- **`DevicePresenceHook` fired for internal clients**: `ClientConnected/Disconnected` fired for the embedded broker's own connections and external-broker bridge clients (both use `neomind-<broker_id>-<uuid>` client_id). Now skips any `neomind-` namespace client_id. *(Round 3, `e179b0df`)*

#### LLM backends
- **Capability refresh race reverted concurrent edits** (`instance_manager.rs::refresh_all_capabilities`): snapshotted instances, awaited Ollama `/api/show`, then wrote back the stale snapshot — silently reverting any concurrent user edits (name, endpoint, model, API key). Now re-fetches the current instance from the in-memory map after the await and merges only capability fields. *(Round 14, `c5d635b7`)*
- **Multimodal-disables-thinking rule was stale**: `adjust_capabilities_for_model` disabled thinking whenever a model was detected as multimodal — correct for 2024-era llava-class models but wrong for gpt-4o, qwen3.5-vl, gemini-2.0-flash-thinking, claude-opus-4 (all support both vision and thinking). Dropped the rule; `PATCH /capabilities` override remains as escape hatch. *(Round 14, `c5d635b7`)*
- **Stream drain after WS drop**: OpenAI/Ollama response handlers kept consuming the upstream HTTP body after the mpsc receiver was dropped (client closed chat, agent timeout, manual stop), burning output tokens and holding connection-pool slots. Added `tx.is_closed()` short-circuit at the top of each chunk loop. *(Round 14, `c5d635b7`)*

#### Sessions / timeseries
- **WS disconnect leaked LLM stream**: previously only persisted history, leaving the in-flight LLM stream running (burning tokens up to global timeout) and leaking the `cancel_senders` entry. Now calls `cancel_session` on disconnect, which both signals the stream and removes the entry. *(Round 13, `89756550`)*
- **`query_aggregated` panic on `bucket_size_secs=0`**: integer divide-by-zero aborted the process. Returns `Error::InvalidInput` instead. *(Round 13, `89756550`)*

#### Storage
- **ExtensionStore `.expect()` panics on concurrent uninstall** (`update_error_status`, `update_health_status`): used `.expect()` on a `table.get()` result that was only checked in a prior read txn. Concurrent uninstall between read and write would panic the process. Replaced with proper `None` handling inside the write txn. *(Round 14, `c5d635b7`)*

#### Data-push
- **Retry backoff wasn't interruptible by stop signal** (`scheduler.rs`): `deliver_with_retry` and `flush_batch` slept inside a plain `tokio::time::sleep`, so `PushScheduler::stop` blocked for the full backoff sum when a downstream destination was unreachable. New `sleep_or_cancel` races sleep against a cloned watch receiver. *(Round 12, `47d71fea`)*
- **`cleanup_logs` never invoked**: data-push.redb grew without bound in high-frequency push scenarios. New background task (15s startup delay, 24h cycle) calls `cleanup_logs(30)`. *(Round 3, `e179b0df`)*

#### Rules engine
- **`cleanup_history(30)` ran only once at startup**: long-running servers accumulated unbounded trigger history in `rule_history.redb` between restarts. New 24h-cycle background task with 20s startup delay. *(Round 12, `38293aba`)*

#### Memory
- **USER.md / KNOWLEDGE.md concurrent-write corruption** (`crates/neomind-agent/src/memory/...`): multiple call sites (background system-context task, agent-summary task, agent memory tool, user-initiated edits, section replacement) concurrently read-modify-wrote shared files with no serialization — one writer could clobber another. Added a `tokio::sync::Mutex` per shared file, held across each op (microseconds for typical memory sizes). *(Round 12, `12d2f69b`)*

#### Tauri desktop
- **Hide-to-tray with no tray**: when `create_tray_menu` failed (Linux WMs without StatusNotifierApplet, e.g. bare i3/sway), the close handler still called `prevent_close() + window.hide()` — no tray icon and no Dock to click left the user with an invisible but running app. Now gates hide-to-tray on tray creation success; falls through to normal close (macOS Dock reopen, Linux exit) otherwise. *(Round 12, `a4a86042`)*

### CLI / API Quality

- **CLI `create` handlers lost entity_id in response envelope** (`crates/neomind-cli-ops/src/{agent_cmd,rule,dashboard,llm,extension,transform}.rs`): read top-level `id` field, but API wraps entities in `{"data": {"id": ...}}` — chained CLI workflows (`neomind agent create ... && neomind agent status <id>`) always saw "unknown". Now extracts from `data.data`. *(Round 15, `1f27e6e2`)*
- **Windows `kill_process_by_pid`** (`crates/neomind-agent/src/toolkit/shell.rs`): called `TerminateProcess(pid, ...)` directly — `TerminateProcess` requires a real process handle, not a PID. Now opens a handle with `PROCESS_TERMINATE` first, terminates, and `CloseHandle`s. *(Round 15, `1f27e6e2`)*
- **`message_channels` invalid `min_severity` silently disabled filter**: `PUT /channels/:name/filter` accepted any string and coerced unrecognized values to `None`, silently turning a restricted channel into accept-all. Now returns 400 with the valid value list. *(Round 15, `1f27e6e2`)*
- **Ollama error response polluted Tauri terminal**: used `println!` which bypasses tracing. Switched to `tracing::warn!`. *(Round 14, `c5d635b7`)*

### JWT / Auth UX
- **JWT expiry boundary** (`validate_token`): `exp < now` strict less-than rejected tokens right at the boundary on clock-skewed clients. Now allows ±30s tolerance (industry-standard for JWT libraries). *(Round 3, `e179b0df`)*

### Internal Hygiene
- `clippy(mdl_format)`: replace 3.14 test value with 2.71 (deny-by-default `approx_constant` lint)
- `clippy(validator)`: escape zero-width space literal as `\u{200B}` (`invisible_characters` lint)
- `clippy(conversation_integration)`: drop always-true `u64 >= 0` assertion
- Deprecate `HeartbeatConfig::is_stale` (dead; tempts callers to bypass per-device `effective_offline_timeout`)
- `allow(deprecated)` on system_memory tests that intentionally exercise the legacy MarkdownMemoryStore API for backwards-compat coverage
- *(Round 0, `8c158111`)*

### Rounds 17–19 (follow-up audit passes)

- **EventBus receiver lag** (`crates/neomind-core/src/eventbus.rs`): `recv()` returned `None` on `Lagged` when the queue had already drained, silently dropping subscribers. Now loops to the next event. *(Round 17, `b1db83bd`)*
- **REST chat path missed memory snapshot** (`session.rs::process_message`): only the WS path injected `MemorySnapshot`, so REST `/api/sessions/:id/chat` ran without persisted user/knowledge context. *(Round 17)*
- **Thinking-model token waste on analytical calls** (`summarization.rs`): summarization LLM call ran with the agent's thinking flag still set. Now saves/restores `thinking_enabled` around the call. *(Round 17)*
- **Tool-result detection inverted** (`streaming/context.rs`): condition `tool_call_id.is_some() && role == "assistant"` was always false (tool results have `role == "tool"`); context compaction mis-classified tool messages. *(Round 17)*
- **Shell tool dedup false-positive** (`tool_loop.rs`): all shell calls shared signature `"shell|"` (used `action`, not `command`), so the loop-dedup guard treated every shell invocation as a duplicate. Signature now includes the command. *(Round 17)*
- **Memory tool session-id race** (`memory_tool.rs` / `sessions.rs`): handler-level writes to a global `memory_session_handle` before processing caused cross-session memory contamination under concurrency. Session id is now set on the agent's tool registry at the start of each `process` call. *(Round 17)*
- **`MemorySnapshot::load` panicked on current_thread runtime** (`snapshot.rs`): used `block_in_place`, which panics on single-threaded runtimes. Replaced with sync file reads via new `MarkdownMemoryStore::read_file_sync`. *(Round 17)*
- **Frontend double `/api/api/` prefix** (`ChatContainer.tsx`, `LLMBackendConfigDialog.tsx`): `fetchAPI("/api/skills")` doubled the prefix since `getApiBase()` already includes `/api`. *(Round 17)*
- **Path traversal in extension handlers** (`extensions.rs`): `serve_extension_asset_handler` and `uninstall_extension_handler` accepted raw `:id` path params; `%2F`-encoded `..%2F..%2F` bypassed axum routing. New `validate_extension_id` (alnum + `-`/`_` only) mirrors the existing `is_safe_skill_id` / `validate_component_id` pattern. *(Round 18, `b9eb8c6b`)*
- **Windows orphan cleanup killed all runners** (`isolated/manager.rs`): `taskkill /F /IM <exe>` matched every runner process system-wide. Replaced with PowerShell CIM enumeration + per-process parent-PID liveness check, killing only true orphans (PPID dead). *(Round 18)*
- **WebSocket event-stream auth bypass** (`handlers/events.rs`): the auth `while let Some(msg)` loop had no guard on natural exit — a client that half-closed or sent only Binary/Ping frames fell through into the event-sending loop unauthenticated. Now tracks an explicit success flag and closes the socket if the loop exits without auth. *(Round 19, `f54a8c47`)*
- **`PushScheduler::stop` held write lock across await** (`data-push/scheduler.rs`): serialised all target operations; a slow stop (retry backoff) blocked concurrent start/stop/update. Now removes under the lock, drops the guard, then awaits (mirrors `stop_all`). *(Round 19)*
- **Exponential backoff overflow** (`data-push/scheduler.rs`): `backoff *= 2` could overflow `u64` on pathological retry configs. Switched to `saturating_mul`. *(Round 19)*
- **JWT signature non-constant-time compare** (`auth_users.rs`): used `String::ne`; now reuses `crate::auth::constant_time_eq_str` (made `pub(crate)`) to avoid timing side-channel on signature bytes. *(Round 19)*

### Deferred (acknowledged, not fixed in this release)

Items identified in audit rounds 14–16 but deferred to avoid scope creep or because they require schema/lock-structure changes. Safe to ship without; tracked for follow-up.

- **JWT revocation**: `sessions` HashMap is write-only; logout and user-delete don't invalidate existing tokens until natural 7-day expiry. *(Round 16, A4)*
- **API key `permissions` field** is defined and tested but never enforced by any middleware. Read-only keys currently grant full access. *(Round 16, A5)*
- **Setup `initialize` TOCTOU**: `list_users()` and `register()` span an await with the lock released; concurrent racing setup is possible on first boot. *(Round 16, A6)*
- **Extension install archive-bomb**: compressed 100 MB upload can expand to ~100 GB; no cumulative uncompressed-size check. *(Round 16, A7)*
- **Telemetry compress mode**: 90-day window with 50-metric concurrent queries can pull ~388M points into RAM. *(Round 16, A8)*
- **Messages dedup TOCTOU** (read-check-write across two lock scopes) under concurrent rule firing. *(Round 16, M3)*
- **Channel filter disk read per send**: `get_filter` opens a redb read txn for every channel on every message — no in-memory cache. *(Round 16, M7)*
- **No retry / dead-letter on channel send failure**: webhook blip = permanent alert loss. *(Round 16, M8)*
- **MQTT data-push eventloop**: only polled inside `send()` with a 5s timeout; connection death during idle >60s isn't detected until next send. *(Round 16, P2)*
- **`extract_by_path` returns `Some(Null)` for missing keys**: template-driven extraction pollutes telemetry with null data points. *(Round 16, D2)*
- **Webhook device timestamp not validated**: clients can send `timestamp: 0` or far-future values. *(Round 16, D4)*
- **MQTT `#` wildcard mishandled**: treated as single-level match; `sensors/#` never matches `sensors/temp/room1`. *(Round 16, D5)*
- **`process_message` swallows parse errors as `0`**: malformed payloads become silent zero telemetry. *(Round 16, D7)*
- **Rule `{value}` placeholder only handles numeric**: string-triggered rules show literal `{value}` in alert text. *(Round 16, R1)*
- **Storage TOCTOU pattern**: 5 `AgentStore::update_*` methods, plus DeviceRegistry/InstanceStore equivalents, follow read-modify-write across two transactions. Needs a coordinated refactor with a write-txn-internal merge helper. *(Rounds 14)*

## [0.8.18] - 2026-06-17

### MQTT — Internal Broker Subscription Regression Fix

Fixes a regression introduced in `0.8.16` (`7903c7e3`) that silently broke device auto-discovery on the **internal embedded broker**.

- **Root cause:** when fixing external broker duplicate-subscription bugs, `self.config.subscribe_topics` was removed from `add_broker()`'s initial subscriptions. However, the internal embedded broker starts via `MqttAdapter::start()` → `add_broker()` (not `add_broker_with_tls`), and its config sets `subscribe_topics = ["#"]` to subscribe to ALL topics for auto-discovery. As a result, the internal broker only subscribed to `device/+/+/uplink` and `device/+/+/downlink`, and devices publishing to any custom topic (e.g. `ne101/abc`, `sensor/foo`) were silently dropped at the adapter boundary.
- **Fix:** restore `self.config.subscribe_topics` inclusion in `add_broker()`, with deduplication to avoid the original duplicate-subscription issue. `add_broker_with_tls` (external brokers) is unaffected because it takes `subscribe_topics` as an explicit parameter.
- **Symptom resolved:** custom-topic devices publishing to the embedded broker (port 8883) now correctly appear in Pending Devices after a few samples.

## [0.8.17] - 2026-06-17

### Pending Devices — Standard-Uplink Auto-Onboarding Fix

Fixes a silent-drop bug where devices publishing to standard uplink topics (`device/{type}/{id}/uplink`) were **never added to the Pending Devices draft list**, even when the device was unregistered.

- **Root cause:** in `mqtt.rs`, the `is_standard_uplink` branch extracted `device_id`/`device_type` from the topic, ran `UnifiedExtractor` (which returns 0 metrics for unknown device types), published `DeviceOnline`, and `return`-ed early — so `DeviceDiscovered` was never published and auto-onboarding never triggered. Only non-standard topics (e.g. `sensor/foo`) reached the auto-onboarding branch.
- **Fix:** at the top of the `is_standard_uplink` branch, check `topic_to_device` for registration; if the topic has no registered device, treat it as a discovery candidate and fall through to the auto-onboarding branch, which publishes `DeviceDiscovered` and creates a draft.
- **Side effect:** the standard branch no longer pollutes the in-memory `device_types` cache with entries for unregistered devices.
- **UI cleanup:** removed a redundant "💡 Changes take effect for newly discovered devices…" info box from the auto-onboarding config dialog (i18n keys pruned).

## [0.8.16] - 2026-06-16

### Device Connectivity — 4-State Connection Model

Resolves a customer-reported UX bug where MQTT-connected devices that hadn't published data within the default 5-minute window displayed "Never Connected" (disconnected), misleading users into thinking the device was misconfigured. The fix introduces a 4-state connection model that decouples transport-level (MQTT session) connectivity from data-driven (telemetry) activity.

#### Phase 0 — Hardcoded Timeout Fix
- `DeviceStatus::is_connected()` used a hardcoded 300s timeout that bypassed the configurable `HeartbeatConfig::offline_timeout`. Replaced with `is_connected_within(timeout_secs)` and updated all 6 call sites in the API layer (`crud.rs`, `agents.rs`, `stats.rs`) to use the configurable value.

#### Phase 1 — Transport-Layer Tracking (Embedded Broker)
- New `DevicePresenceHook` in `embedded_broker.rs` hooks into rmqtt's `ClientConnected`/`ClientDisconnected` lifecycle events, publishing `DeviceTransportOnline`/`DeviceTransportOffline` events independently of data activity.
- `DeviceStatus` gained `transport_connected: bool` and `transport_changed_at: i64` (with `#[serde(default)]` for forward compatibility with existing storage).
- EventBus wired to all 3 `EmbeddedBroker::new` call sites in `server/types.rs` (initial + 2 rollback paths).

#### Phase 2 — Per-Device Offline Timeout Override
- `DeviceConfig.offline_timeout_secs: Option<u64>` and `DeviceTypeTemplate.default_offline_timeout_secs: Option<u64>` added with forward-compatible serde defaults.
- Resolution priority: **device override → template default → global `HeartbeatConfig::offline_timeout`**.
- `DeviceService::effective_offline_timeout(device_id)` helper resolves the fully-qualified timeout for any device.
- All 6 `is_connected_within()` call sites in `crud.rs` now resolve per-device timeouts.
- Exposed via `PUT /api/devices/:id` (`UpdateDeviceRequest.offline_timeout_secs`) and `DeviceDto` responses.
- **Backend validation:** 30–86400 seconds (30s min to avoid status flicker, 24h max).
- `DeviceDto.effective_offline_timeout_secs` lets the frontend display the resolved default without a separate API call.

#### Phase 3 — Frontend 4-State UI
- New `web/src/lib/utils/deviceStatus.ts` with `getDeviceState()` returning `online | connectedIdle | offline | disconnected`. Gracefully degrades to legacy 3-state when `transport_connected` is undefined (older backend or external broker).
- New `DeviceStatusBadge` component (`web/src/components/shared/DeviceStatusBadge.tsx`) renders all 4 states with proper color variants (success / info / warning / muted).
- Wired into `DeviceList.tsx` (desktop table + mobile card) and `DeviceDetail.tsx` header.
- `EditDeviceDialog` gained an offline-timeout input field with inline validation, default-value display, and placement at the bottom of the form.
- **i18n:** Added `statusLabels.connectedIdle` (en: "Connected·Idle", zh: "已连接·空闲"); clarified zh `disconnected` from ambiguous "未连接" to "从未上线".

#### External Broker Behavior
- External MQTT brokers (Mosquitto, EMQX, etc.) without rmqtt hooks gracefully degrade to 3-state: **Online / Offline / Never Connected**. The "Connected·Idle" state only appears with the embedded broker. This is correct behavior — NeoMind cannot detect MQTT session state without broker-level hooks.

## [0.8.15] - 2026-06-16

### LLM Backend — Multimodal Capability Override

- **Manual override switch added:** the LLM backend edit dialog now exposes a Switch + "Reset to auto" control that PATCHes `/api/llm-backends/:id/capabilities` immediately, decoupled from the dialog's Save button. Previously the override endpoint existed but was only reachable via raw `curl`, leaving users no in-product way to correct auto-detection false positives (text-only Qwen tiers misclassified as vision, registry gaps for bare aliases like `claude-3-sonnet`, future unregistered vision models).
- **Three-state semantics:** `multimodal_user_override == null` → Auto (Switch reflects the auto-detected value, caption shows `Vision (Auto)`); `true`/`false` → pinned, caption shows `Vision (Override)` and a Reset button appears.
- **Create vs edit mode:** the Switch only renders when editing an existing backend (PATCH needs an id). Create mode keeps the original read-only Vision badge.
- **Error handling:** uses `useErrorHandler` + `extractErrorMessage`; the API client passes `skipErrorToast: true` to avoid double-toasting. 404 → "Backend not found", other → generic failure toast with the API message.
- **No `onRefresh()` after success:** the parent's `loadData` would `setLoading(true)` and unmount the dialog mid-interaction; the PATCH response is authoritative for local state, and the parent card list reconciles on the next natural refresh.


### Device Connectivity — External MQTT Broker Fixes

Fixes a long-standing bug where devices stayed stuck at "未连接" (disconnected) after connecting to an external `mqtts://` broker (e.g. on Windows or any platform). Seven issues in the external broker subscription path are resolved:

- **Duplicate SUBSCRIBE eliminated:** removed a double-merge of `subscribe_topics` in `add_broker_with_tls` that caused each topic to be subscribed twice.
- **Empty `subscribe_topics` no longer clobbers defaults:** `Some([])` in create/update broker handlers is now ignored, so the default three telemetry topics survive.
- **Re-subscribe on broker add:** every registered device's `telemetry_topic` is now re-subscribed when a broker is added (covers the server-restart path) via the new `subscribe_device_telemetry_topics` helper.
- **Event loop deadlock fixed:** the event loop is now spawned *before* the first `client.subscribe()` call, and the request channel capacity is raised 10 → 100 — previously subscribing more than 10 topics deadlocked silently.
- **All-failed subscriptions now surface an error:** instead of silently marking the broker "connected" when every subscription failed, the broker now returns an error and tears down its spawned task + client (no more half-connected brokers leaking).
- **Auto re-subscribe on reconnect:** broker reconnects (Err → Ok transition) now trigger `resubscribe_after_reconnect`, since `clean_session=true` brokers forget subscriptions on every disconnect.
- **`clean_session` honored:** the `MqttConfig.clean_session` flag is now actually applied via `set_clean_session`, instead of being a dead field.
- **Adapter broadcast:** `register_device` now broadcasts to *all* matching MQTT adapters instead of stopping at the first, so a device with `adapter_id=None` is subscribed on every connected broker.

### Agent — Default Ollama Model & Schema Localization

- **Default model switched:** the default Ollama model across `default_model`, the placeholder, and the schema default is now `qwen3.5:4b` (was `ministral-3:3b`), matching the eval model and a generally available Ollama tag.
- **LLM backend schema localized to English:** all form schema strings (titles, descriptions, display names) in `instance_manager.rs` were converted from Chinese to English for consistency with the rest of the schema.

## [0.8.13] - 2026-06-14

### Frontend Polish & TopNav Redesign

A product-quality pass on the web UI: navigation reorganization, micro-interactions, and a batch of bug fixes exposed by the new toast stacking.

#### TopNav right cluster redesign

Reordered the right-side toolbar from an ungrouped row into a logical flow: **identity → attention → preferences → user**. New `SystemHealthButton` provides a glanceable status dot (green/yellow/red) backed by a dropdown mini-panel showing backend connection, device online count, and unread alerts. `InstanceSelector` icon changed from `Wifi` to `Server` to avoid visual redundancy with the health indicator.

#### User menu

Avatar dropdown now shows a role badge (`admin`/`user`/`viewer`) alongside the username, with **Preferences** and **About** shortcuts that deep-link to the corresponding settings tabs. (Help item deferred until the standalone wiki launches.)

#### Micro-interactions & polish

- **Button press feedback:** all buttons now scale to 97% on `active` (`active:scale-[0.97]`).
- **Toast stacking:** limit raised from 1 → 3 with uniform `gap-2` spacing.
- **404 page:** dedicated `NotFound` page with `FileQuestion` icon and quick-return buttons, replacing the silent redirect to `/`.
- **Route transitions:** page content fades in on every route change (`animate-fade-in` keyed by pathname).
- **Top progress bar:** lightweight 2px `NavigationProgress` bar animates 0% → 80% → 100% on each navigation.

#### Bug fixes

- **401 toast duplication:** when a JWT expired, multiple concurrent API calls (`fetchAlerts`, `fetchDevices`, `checkAuthStatus`) each fired their own "Unauthorized" toast. Previously masked by `TOAST_LIMIT=1`. Added a 3-second throttle (`shouldShowUnauthorizedToast`) so only the first 401 in a burst shows a toast.
- **Settings tab deep-link:** clicking "Preferences" in the user menu while already on `/settings` didn't switch tabs — `useState` initializer only runs on mount. Added a `useEffect` syncing `activeSection` to the `?tab=` URL param on subsequent navigations.

### Agent LLM Error Surfacing

Agents used to swallow LLM failures mid-execution and fall back silently, leaving the user with no indication that the AI brain had stopped working. This round makes errors visible and classifies them so transient failures can retry while permanent ones fail fast.

#### `LlmError::Api` variant + `is_permanent()`

New structured error type distinguishes API-level failures (HTTP 4xx/5xx, rate limits, auth errors) from transient network hiccups. `is_permanent()` returns `true` for 4xx (except 429), `false` for 5xx/429/timeouts — driving retry vs. fail-fast decisions.

#### Removed silent fallbacks

The analyzer path previously caught all LLM errors and produced a generic fallback analysis, hiding the real failure. Now:

- Ollama and OpenAI backends return `LlmError::Api` with the actual status/message.
- The tool loop surfaces the error instead of silently continuing.
- Mid-execution LLM failures mark the agent run as **Failed** (not silently "succeeded with fallback").

#### Tests

Integration tests cover `LlmError` classification across all status codes (4xx permanent, 5xx retryable, 429 retryable, timeout retryable).

### Onboarding Wizard Simplification

Streamlined the getting-started wizard from **3 steps → 2 steps** (Setup → Ready). The intermediate "Capability Panorama" step was removed — users found it redundant after the core setup already explains what each module does.

### Dashboard & Data

- **Sparkline pipeline unification:** extracted a shared `useChartPipeline` hook, eliminating duplicated data-transform logic across dashboard chart components.
- **Chart aggregation fix:** corrected aggregation option keys and restricted line/area charts to raw data only (aggregation on these chart types produced misleading visual artifacts).
- **Transform real-time updates:** WebSocket push now delivers Transform data source updates to dashboards in real time (previously only refreshed on poll).
- **MetricValue serialization:** JSON-typed metric values are now serialized as strings in WebSocket events, preventing `{"String":"42"}` artifacts on the frontend.

### Other Fixes

- **IME input garbling:** password field no longer accepts intermediate composition state, preventing CJK input methods from corrupting typed passwords.

## [0.8.12] - 2026-06-12

### Agent Reliability: Tool Execution & Memory

A focused round of fixes targeting two recurring failure modes in scheduled AI agents — tool-call result misattribution and runaway memory file growth. All backend-only; full test suite passes (387 tests, 6 new).

#### Tool-call result ordering (deterministic bug)

`ToolRegistry::execute_parallel` returned results in **JoinSet completion order**, but `build_round_tool_calls` paired them to calls **by index**. When parallel tools finished out of order, each result was labeled with the wrong tool's name — making execution logs show phantom failures and cross-attributed errors (e.g. a `memory` error shown under `shell`, or "1/6 succeeded" when most actually worked). Fixed with index-tagged slots that reassemble results in input order. This was the single largest source of apparent "agent chaos."

#### Memory file quadratic-growth fix

Custom memory files (`custom:{name}`, e.g. `task-understanding`) grew quadratically: the agent re-appended its full "Pattern Tracking" section every analysis, and the `add` path applied **no deduplication** (unlike `user`/`knowledge` targets). A real file reached ~70% redundant content by round 6, blowing past the char cap.

Replaced the unconditional `append_content` with `merge_custom_content` — an in-place, section-level merge:

| Agent sends | Result |
|---|---|
| Exact-duplicate section | Dropped (no-op → "Skipped") |
| Same section + one new line | Only the **new line** appended in place |
| New section header | Appended as a new section |
| Near-identical whole block (≥0.9 similarity) | Dropped |
| Header-less text | Novel lines appended |

Net effect: even if the agent ignores guidance and re-sends an entire growing section, only genuinely new data lands — growth is linear, not quadratic.

#### Tool-call hallucination self-correction

When the agent called a non-existent tool, it got a bare "not found" with no recovery path. Now `NotFound` returns **targeted guidance**:

| Hallucinated name | Hint returned |
|---|---|
| `message` / `notify` / `alert` / `send_message` / … | Exact `neomind message send` shell syntax |
| `device` / `dashboard` / `rule` / `agent` / … (11 CLI domains) | "Use shell: `neomind <domain> <action>`" |
| **Anything else** (universal fallback) | Dynamic list of the *actually-registered* tools (incl. extension tools) + "Use shell for any neomind CLI command" |

The streaming/chat path already had a silent `message`→`shell` mapper; the scheduled-executor path (where agents run) was the gap — now closed.

#### Memory capacity & guidance

- **Char limit raised:** agent memory files 5 000 → **20 000** chars (long-task context survives to the next execution).
- **Prefetch injection cap raised:** 6 000 → **12 000** chars/file, so the raised write limit is visible in-context without burning a tool-call round.
- **Auto-init template slimmed:** removed verbose "Memory Commands" examples + "Notes" block (~700 → ~300 chars per new agent).
- **Prompt guidance:** system prompt now explicitly states messages go through `shell` (no separate `message` tool) and that `add` should append only new data points, never re-list previous entries.
- **Memory tool schema:** per-action field requirements and the ~20 000 char limit are stated in the tool description so the LLM knows the constraints upfront.

### Deployment & Packaging

- **Install script:** frontend directory now swapped atomically on upgrade (staging dir → rename old → rename new → cleanup), eliminating stale Vite asset accumulation across versions.
- **Tauri sidecar:** extension-runner lookup now handles the Windows `.exe` suffix and uses a 3-tier search (staged sidecar → workspace build → error with both paths).

### Frontend

- **Rule Builder redesign:** full rewrite from multi-step wizard to split-workspace layout (`BuilderShell`). Form and DSL preview tabs share a single workspace; conditions and actions are visible simultaneously instead of paginated. All 7 action types (Execute, Set, Notify, CreateAlert, HttpRequest, Log, Delay) surfaced as one-click buttons. Required-field validation now covers name, cron expression (schedule trigger), and every action type's mandatory fields — incomplete actions show inline errors. Condition (indigo) and Action (emerald) sections are visually differentiated with accent-colored headers. Removed 3 dead step components and the redundant footer Cancel button.
- **Transform Builder:** migrated to the same `BuilderShell` split-workspace layout. Templates toolbar converted from flat buttons (height-growth bug) to a dropdown. Dead i18n fallbacks cleaned up; mobile cards now show execution count alongside last-executed time.
- **Automation list views:** mobile cards and desktop tables share `ResponsiveTable` patterns; consistency pass on icons, badges, and pagination.

### Backend

- **Rule list API:** `GET /rules` now returns `created_at` and structured `actions` (previously only in the detail endpoint), fixing empty "Created" and "Execute Actions" columns in the rule list.
- **Transform execution tracking:** `mark_executed()` now records `last_executed` + `execution_count` without bumping `updated_at`. Execution stats are persisted with a 60-second throttle (shared `Arc<Mutex<HashMap>>`) to prevent write amplification under high-frequency event evaluation.
- **Agent tool-hint fix:** `ToolError::NotFound` hint now matches the enum variant instead of stringifying, closing a gap where hallucinated-tool guidance was silently skipped.

## [0.8.11] - 2026-06-11

### Onboarding Wizard Redesign

Rewrote the getting-started dialog from a single page into a **3-step paginated wizard** (Platform Intro → Core Setup → Capability Panorama). Users can freely browse steps via clickable progress dots; finishing or skipping marks the guide as seen.

**Step 1 — Platform Intro:** positioning statement, AI-first differentiator callout, data-flow ribbon (Devices → Data → AI → Dashboards/Alerts), and 3 value pillars (chat / unified / edge).

**Step 2 — Core Setup:** two status-aware cards (LLM + Device) that auto-detect completion from backend state. Each card includes a collapsible CLI quick-start helper:
- **LLM helper:** 7-provider selector (Ollama, OpenAI, Anthropic, DeepSeek, GLM, Qwen, xAI) generates a ready-to-copy `neomind llm create` command with correct endpoints and models, plus followup `llm test` / `llm activate` commands.
- **Device helper:** single `curl` command that POSTs telemetry to the webhook endpoint — triggers auto-discovery for unregistered devices (no MQTT tools needed). Followup shows `device drafts list` → `drafts approve` to complete the closed loop.

**Step 3 — Capability Panorama:** 4 cards covering all platform modules:
1. Real-time Monitoring & Visualization (20+ built-in dashboard widgets, shareable links)
2. Automation & AI Agents (rules, scheduled/event-driven agents, 7 notification channels)
3. Extension Ecosystem (marketplace: YOLO vision, OCR, weather, integrations; tools auto-discovered by AI)
4. Custom Component Development (React IIFE widgets, `neomind widget create` scaffold, ZIP install / marketplace publish)

Full i18n support (zh/en). All design-token compliant (no hardcoded colors, valid tint tokens throughout).

### Dead Code Cleanup (51 rounds, compiler-verified)

Systematic removal of ~10,000+ lines of dead/superseded code across the entire Rust workspace. All removals verified by compiler (`cargo build --tests`) — zero functional impact, all tests pass.

**Benefits:**
- Cleaner public API surface — crate roots now export only what consumers actually use
- Faster compilation — fewer modules to parse and type-check
- Reduced cognitive load — maintainers no longer wade through unused abstractions
- Lower risk of drift — dead code silently rots and misleads future readers
- Accurate dependency picture — removed phantom couplings between modules

**Removed dead modules (~3,800 lines):**

| Module | Crate | Lines | Why dead |
|--------|-------|-------|----------|
| `planner/` (5 files) | agent | ~900 | Upfront planning superseded by streaming tool-calling + Skills |
| `context/` dead files (5) | agent | ~2000 | Referenced old tool names, zero production callers |
| `tools/event_integration.rs` | agent | ~1030 | `EventIntegratedToolRegistry` never referenced |
| `scheduler.rs` (P2.1) | agent | ~320 | Dependency-aware scheduling never wired into execution |
| `llm_backends/config.rs` | agent | ~306 | `LlmBackendConfig`/`LlmRuntimeManager` unused |
| `llm_backends/factories/` | agent | ~400 | `BackendFactory` trait + impls never instantiated |
| `session.rs` cleanup subsystem | agent | ~127 | Cleanup task never started (`cleanup_running` always false) |
| `storage/mod.rs` | core | 94 | `StorageBackend`/`StorageFactory` traits never adopted |
| `monitoring.rs` | storage | 614 | `StorageMonitor` never integrated |
| `backup.rs` | storage | 517 | `BackupManager` never wired |
| `llm_data.rs` | storage | 746 | Superseded by `system_memory::MarkdownMemoryStore` |
| `device_state.rs` | storage | 905 | Superseded by device_registry + neomind-devices |
| `agent_summary.rs` | storage | 82 | Zero callers |
| `history.rs` | rules | 606 | `RuleHistoryStorage` never used (production uses storage::business) |

**Removed dead re-exports (~250+ across all crates):**

Compiler-based verification (strip all `pub use`, rebuild, add back only what errors demand). Eliminates grep false positives from multi-line brace imports. Cleaned across: neomind-core (17), neomind-storage (~60), neomind-agent (7), neomind-devices (33), neomind-rules (43), neomind-messages (12), neomind-cli-ops (3), neomind-data-push (1), neomind-api (~38).

**Removed dead functions/types/Default impls (~150+ items):**

- Dead LLM backend dynamic registration system (`BackendFactory`, `BackendRegistry`, global singleton, `DynamicLlmRuntime`)
- Dead event bus persistence + backpressure (`EventPersistence`, `publish_with_backpressure*`)
- Dead extension health monitoring (`ExtensionHealthInfo`, `get_health_info()`)
- Dead `ExtensionToolGenerator`/`ExtensionFilter` (superseded by `ExtensionToolExecutor`)
- Dead `AgentExecutionResult`, `EntityResolver`, `MemoryManager` wrappers
- 30+ dead `Default` impls where `::default()`/`unwrap_or_default()`/`or_default()` never called
- Dead semantic wrapper methods, factory methods, and event variants never emitted

### Agent Module Architecture Refactor

Major decomposition of two oversized source files into focused, maintainable sub-modules. Pure structural refactoring — zero logic changes, all public APIs preserved via re-exports. 4 rounds of code review, 540/540 tests pass.

**`streaming.rs` (4,231 lines → 12 sub-modules):**

- `intent.rs` — "List-only dead end" detection (action verb matching, read-only tool detection, action hint extraction)
- `cache.rs` — `ToolResultCache` with TTL, size limits, and key normalization
- `thinking.rs` — Thinking content cleanup and repetition removal
- `tool_detect.rs` — JSON tool call detection in LLM response buffer
- `sanitize.rs` — Base64 stripping, data image URL replacement, UTF-8 safe truncation
- `dedup.rs` — Cross-round tool result deduplication with entity ID extraction
- `result_format.rs` — Tool result formatting for shell, device, agent, rule, extension outputs
- `context.rs` — Context window building with tiered compaction, token estimation, message priority
- `resolve.rs` — Cached argument resolution, tool name mapping, image auto-injection
- `tool_exec.rs` — Tool execution with retry and caching
- `stream_core.rs` — Main text-only streaming loop (ReAct pattern)
- `stream_multimodal.rs` — Multimodal (text + image) streaming loop

**`executor/mod.rs` (3,169 → 1,450 lines + 4 sub-modules):**

- `tool_loop.rs` — Multi-round tool execution loop with deduplication, duplicate round detection, and Phase 2 summary generation
- `tool_prompt.rs` — System prompt construction (resource sections, tool messages, knowledge injection)
- `tool_result.rs` — Tool result processing, Phase 2 summary via LLM, and final decision building
- `compact.rs` — Message compaction for context window management

### Testing

- **17 new boundary tests** covering cross-module interfaces:
  - `intent`: 4 tests (Chinese/English action verbs, read-only detection, action hints)
  - `cache`: 5 tests (key consistency, TTL expiration, insert/get, cacheability)
  - `dedup`: 4 tests (latest-keep, entity separation, JSON/non-JSON key generation)
  - `resolve`: 4 tests (passthrough, missing references, HTTP URL handling, tool name resolution)

### CLI Domain Tool Consolidation

Unified all CLI domain tools (device, agent, rule, message, transform, alert) to route through the `shell` tool, eliminating duplicate tool definitions in the registry and simplifying the LLM's tool surface.

- **Mapper routing** — `ToolNameMapper` now maps CLI domains (`device`, `agent`, `rule`, etc.) to `shell` instead of standalone tool names. `build_cli_command()` converts structured arguments into `neomind <domain> <action> --flag value` CLI commands
- **Registry fallback** — `ToolRegistry::execute()` and `execute_parallel()` detect CLI domain tool names and fall back to shell execution when the tool isn't directly registered
- **tool_exec integration** — `execute_with_retry_impl()` converts CLI domain calls to shell commands before execution, handling timeout inheritance correctly
- **Removed `format_for_llm`** — No longer needed; tool descriptions now come from the shell tool's embedded CLI reference

### In-Process CLI Dispatch (eliminate stale-binary class of bugs)

The agent's shell tool no longer depends on whatever `neomind` binary happens to be in PATH. Data commands now run in-process via a shared dispatcher, returning structured `CliResponse` directly — no subprocess, no PATH binary dependency, no version drift.

**Root cause eliminated:** the agent shell tool executed `neomind` commands by spawning `$SHELL -l -c "neomind ..."`, which resolved `neomind` from PATH. The PATH binary had drifted to v0.7.8/v0.8.2 while the server ran v0.8.11, causing truncated/incorrect output (e.g. `dashboard list` emitted full JSON, exceeded the 10000-char shell truncation, got cut mid-array, and the LLM saw an incomplete list).

**Architecture:**
- `neomind-cli-ops` now owns the clap command types (`dispatch/commands.rs`), data-command handlers (`dispatch/handlers.rs`), and the top-level dispatcher (`dispatch::dispatch(argv) -> Result<CliResponse, DispatchError>`)
- `neomind-cli` binary thinned from ~4700 to ~1500 lines — delegates data commands to cli-ops handlers, keeps interactive functions (serve/chat/logs)
- Agent `shell.rs` intercepts `neomind ` commands via `try_in_process_dispatch()` — calls `dispatch(argv)` directly in-process; falls back to subprocess on `DispatchError::NotInProcess` (side-effecting/interactive/local-only commands)
- `try_parse_from` (not `parse`) prevents `exit()`-ing the agent process on malformed arguments
- Per-command timeout via `tokio::time::timeout` matches subprocess lifecycle behavior

**Verification:** all 7 `--help` outputs byte-identical before/after; in-process dispatch returns correct data (e.g. `dashboard list` → total=16) against running server; fallback to subprocess works for side-effecting commands; parse errors return gracefully instead of killing the process.

### Tauri Sidecar Removal (-99 MB)

Removed the `neomind-cli` binary from the Tauri app bundle (`externalBin`). The 99 MB sidecar was bundled but never actually used — the agent shell tool resolved `neomind` from PATH, not from the app bundle. With in-process dispatch, data commands now use `neomind-cli-ops` compiled directly into the Tauri binary (always version-synced with the running server).

- Removed `binaries/neomind-cli` from `tauri.conf.json` externalBin
- Removed CLI binary copy logic from `build.rs`
- Deleted `scripts/build-cli.sh`
- CI Tauri build job no longer builds/copies CLI (standalone CLI for Docker/server distribution unchanged)

### Fixed

- **Re-export completeness** — Added missing `cleanup_thinking_content` and `format_tool_results` re-exports in `agent/mod.rs`
- **Focused+ tool guidance** — Updated to use `shell` commands (`neomind device history`, `neomind device control`) instead of removed `device(action=...)` pattern

## [0.8.10] - 2026-06-11

### Agent Native Tool Calling

Complete overhaul of the tool-call parsing pipeline — agent executor now uses native structured tool calls from the LLM API response directly, instead of parsing them from freeform text.

- **Native `tool_calls` field** — Added `LlmOutput.tool_calls: Option<Vec<Value>>` in `neomind-core::llm::backend`. All three backends (OpenAI, Ollama, llama.cpp) now populate this field with structured JSON from the API response, preserving tool call IDs
- **Priority-based parsing** — Tool loop uses native `tool_calls` first → text parsing fallback → thinking field fallback. Eliminates fragility of regex-based extraction for models that support native tool calling
- **`FinishReason::ToolCalls`** — New finish reason variant for tool-call stop conditions. OpenAI (`tool_calls`), Anthropic (`tool_use`), Ollama, and llama.cpp all map to this instead of `Stop`
- **Continuation mechanism** — When the LLM is still making tool calls at `max_rounds`, up to 10 additional rounds are allowed so the agent can finish its work instead of being cut off mid-task
- **Vision tool exclusion** — Vision tool is excluded from the tool list when the multimodal LLM already receives images inline, avoiding redundant image analysis

### Agent Data Collection

- **Device info block** — `build_resource_table()` now renders a separate `**Devices:**` section above the metrics table, showing device ID, name, and type for each bound device
- **Resource display names** — Resource table shows both `resource_id` and `name` when they differ (e.g. `device-001 (Temperature Sensor)`)
- **Image metric child-path skip** — Data collector skips child paths of already-collected image metrics (e.g. `values.image.image_base64` under `values.image`) to prevent duplicate image data
- **Event-triggered device metadata** — Event-triggered executions now include device metadata (ID, type, name, adapter) in the data context, so the LLM knows which device triggered the event
- **Image metric extraction guard** — If an event metric is recognized as an image but extraction fails (no URL, no base64), execution is skipped instead of producing an empty analysis

### AI Analyst

- **WS/API dedup** — Invoke response now uses execution ID for message dedup with WebSocket events, preventing duplicate AI messages when both WS and HTTP API return results
- **Streaming placeholder cleanup** — Properly cleans up streaming placeholders and polling intervals when invoke resolves or errors, including error/timeout paths
- **Agent name dedup** — Agent name now includes component ID prefix (`AI Analyst [a1b2c3d4]`) to prevent name collisions across multiple analyst instances
- **Device info filter** — History message loader skips `device_info` entries (device metadata, not sensor data) when building AI analyst context

### Device Data

- **JSON key trimming** — `UnifiedExtractor` now trims whitespace from JSON object keys before processing. Handles devices that send keys with leading/trailing spaces (e.g. `" values.image"`) which would break downstream metric lookups
- **Empty key skip** — JSON keys that are empty after trimming are skipped entirely

### Fixed

- **CI release uploads** — Added `contents: write` permission to GitHub Actions workflow for release asset uploads

## [0.8.9] - 2026-06-10

### Image History Performance Overhaul

Complete end-to-end optimization of the image telemetry data pipeline — from API response to rendered `<img>`. Cuts first meaningful paint from ~12s to ~1-2s for dashboards with camera image history widgets.

- **Two-phase loading** — ImageHistory now loads 3 latest images (1h range) within ~1-2s, then fetches full 200-image history in the background. User sees images immediately instead of waiting for the entire 6MB+ payload
- **Pre-normalized base64 pipeline** — Raw base64 from the database is converted to `data:image/...;base64,...` data URLs once at fetch time (fetch layer, WS events, store merge path), eliminating expensive per-render normalization (`isPureBase64` regex + `atob` + string copies on 50KB strings × 200 images)
- **Fast-path rendering** — `toImageHistoryItems()` detects pre-normalized data URLs via `startsWith()` (no regex/atob) and skips `normalizeImageUrl()` entirely — zero string copies per image
- **Fingerprint-based tracking** — Replaced full base64 URL storage in source tracking Sets/arrays with lightweight fingerprints (length + charCode + last 32 chars), reducing tracking memory from ~10MB to ~8KB
- **O(n) source comparison** — Replaced O(n²) `filter+includes` on 50KB strings with Set-based O(n) intersection using fingerprints
- **Removed cache busting** — Eliminated pointless `#timestamp` fragment appended to data/blob URLs (no effect on inline content, only created 50KB string copies)
- **mergeLiveData O(k) optimization** — Fetched data is already sorted by `sortTelemetryResults`; only live WS points need individual insertion — eliminates O(n²) array copies (was 20,100 intermediate arrays per merge)
- **Raw cache limiting** — Telemetry cache for image sources stores only the last 5 raw items instead of all 200, reducing in-memory cache from ~30-50MB to ~1.5MB
- **Phase reset on source change** — Fixed bug where switching image data source kept `phase='full'`, causing stale data to display while the new source loaded

### DataSource Pipeline Optimization

- **Single-pass source categorization** — Replaced 5 separate `useMemo` + `.filter()` calls in `useDataSource` with a single loop that extracts telemetry/polling/extension sources + device ID sets + WS flag in one pass
- **Stable setDataAdapter** — Eliminated 3 identical per-render closures (one per sub-hook) with a single `useCallback` adapter, reducing re-render cascade
- **Shared `getTs` utility** — Extracted timestamp accessor from `useExtensionSource` into `eventProcessors` shared module, deduplicating identical local functions
- **Backward scan for extension events** — Changed `findIndex` (forward scan) to backward loop in event dedup, which is cache-friendly and stops at first match
- **Extension cache key** — Reused `effectiveTimeWindow` computed during fetch instead of recomputing per-source in cache step
- **findDevice O(1) cache** — Module-level `Map` cache in `deviceUtils.ts` shared across all callers, replacing O(n) `.find()` scan; used by `deviceSlice` telemetry flush path

### AI Analyst Enhancement

- **Config panel** — Added settings dialog (gear button) with model selector, system prompt editor, and context window size control
- **Model selector** — Uses design-system `Select` component with Auto (default) option, vision capability indicator (Eye icon), and per-backend model grouping
- **Model name persistence** — Config now saves both `modelId` and `modelName` to survive page refresh
- **Streaming indicator** — Input bar shows streaming state during LLM response generation
- **Icon picker** — Replaced raw `lucide-react` barrel import with `dynamicIconMap` for tree-shaking in icon picker, component library, community registry, and dynamic registry
- **Barrel import cleanup** — Removed `import * as lucideReact` from 6 files (ComponentLibrarySidebar, InstallComponentDialog, VisualDashboard, componentLibraryUtils, CommunityRegistry, DynamicRegistry), replaced with individual imports or `dynamicIconMap`

### Backend

- **Image data extraction** — Added `image_base64` and `image_mime_type` field support in `data_collector.rs::extract_image_data`, covering more extension output formats
- **Qwen 3.7 multimodal** — Added `qwen3.7` to heuristic vision match for native multimodal detection
- **Agent error message** — LLM failure path now produces actionable conclusion ("check model availability and capabilities") instead of generic fallback
- **Agent API handlers** — Fixed agent CRUD and execution endpoints in `neomind-api`
- **Agent storage query** — Fixed agent list query in `neomind-storage`

### Agent Memory & Context

- **Agent focused-path simplification** — Removed ~1300 lines of dead code from `analyzer.rs` and `response_parser.rs` (dead `insight` field, 5 unused JSON parsing functions, `build_focused_system_prompt`, `build_available_commands_description`, etc.)
- **Tool result hard limit** — Consolidated duplicate `TOOL_RESULT_MAX_LEN` constants into single 128KB module-level limit
- **Knowledge inline injection** — `build_tool_system_prompt()` now receives pre-fetched knowledge file contents, eliminating per-execution tool-call overhead
- **Context compaction refinement** — Adjusted priority-based token compaction thresholds for 128K context models
- **Context-aware history** — `build_history_context()` updated with knowledge content parameter and improved data freshness display
- **Memory journal** — Relaxed action_taken truncation to 150 chars/action, improved learning guidance language
- **Streaming dedup cleanup** — Reduced `MAX_TOOL_ITERATIONS` from 100 to 30 (matches scheduled executor max_rounds)

### Fixed

- **Component config save** — Added loading spinner and disabled state to save buttons in `ComponentConfigDialog` (both desktop and mobile layouts), prevents double-submit
- **Duplicate toast on agent save** — Removed redundant toast from `AgentsPage`, now handled by `AgentEditorFullScreen`

### Frontend

- **Font loading** — Switched Google Fonts to async load (`media="print" onload`) to eliminate render-blocking, added italic 400/800 weights
- **Rules list** — Added `Created` and `Last Triggered` columns with execution count display
- **Transforms list** — Added `Created` and `Last Executed` columns, replaced Transform Code with description subtitle, mobile cards show last executed time
- **Agent editor** — Added Max Chain Depth slider control
- **ResponsiveTable** — Fixed row hover to use `bg-muted-30` for consistency with design tokens
- **MapDisplay** — Removed unused center point indicator
- **Device detail** — Minor UI fixes

## [0.8.8] - 2026-06-09

### Visual Quality & Brand Identity

- **Brand color system** — Added `--brand` CSS variable (NeoMind orange #E05727) with light/dark variants, registered in Tailwind config
- **Enhanced Aurora background** — Doubled aurora gradient opacity for more visible ambient lighting in both light and dark modes
- **Card hover lift effect** — ResponsiveTable mobile cards and DeviceList cards now lift with shadow on hover (`hover:shadow-md hover:-translate-y-0.5`)
- **Table row brand-tinted hover** — Desktop table rows highlight with subtle brand color on hover instead of plain gray
- **Unified loading states** — Replaced 11 raw `Loader2` spinners across page-level and dialog contexts with consistent `LoadingState` component
- **Extension marquee brand color** — Empty state marquee cards use brand color for icon backgrounds and hover borders
- **EmptyState consistency** — Unified icon container styling across `EmptyState` and `EmptyStateCompact`

### Fixed

- **AiAnalyst JSX structure** — Fixed unclosed div tag in initializing state render
- **AgentDetailPanel stale closure** — Used ref to avoid stale closure over agent ID in event handlers

---

## [0.8.7] - 2026-06-08

### Agent Memory & Context Engineering Overhaul

Complete rewrite of the agent memory system — replacing a complex hierarchical model (ShortTermMemory, LongTermMemory, TaskProfile, fingerprint-based dedup, LLM reflection) with a simple and effective ExecutionJournal + KnowledgeFileRef design.

### Added

- **ExecutionJournal** — FIFO ring buffer of `ExecutionRecord` (max 10 entries). Each execution logs outcome, actions taken, success status, and timestamp
- **KnowledgeFileRef** — Index entries for agent-scoped knowledge files created by the LLM via the `memory` tool. Replaces TaskProfile + Baselines
- **Agent-scoped knowledge files** — `custom:{name}` files now isolated per agent at `agents/{agent_id}/custom/{name}.md`
- **Rule creation metric discovery enforcement** — Three-layer defense to prevent LLM from creating rules with guessed device IDs or metric names
- **Smart tool result compaction** — `compact_messages()` now uses `smart_summarize_tool_result()` to preserve key data instead of blind truncation
- **Agent knowledge file initialization at creation** — `task-understanding.md` is created immediately when an agent is created
- **Knowledge file content API field** — `KnowledgeFileRefDto` now includes optional `content` field
- **Complex MetricValue in Extension transforms** — `TransformedMetric.value` upgraded from `f64` to `MetricValue` (Float/Integer/Boolean/String/Json)
- **Extension input/output mapping resolution** — Automatic dot-path extraction, URL fetch, base64 encoding for transform parameters
- **Dynamic output type detection** — Transform output registry detects MetricValue variant instead of hardcoding Float
- **Image metric click-to-view** — Map and CustomLayer metric popups detect image values and display thumbnails
- **`window.neomind` API** — `callExtension()`, `fetchDeviceValues()`, `createTransform()`, `updateTransform()`, `deleteTransform()`, `listTransforms()` for community components
- **Dashboard Advanced tab** — Component config dialog supports custom `AdvancedPanel` from community/extension bundles
- **Community component `config` prop** — `ComponentRenderer` passes full config object to community components
- **Dashboard SSE self-sync echo suppression** — Prevents stale server data from overwriting in-progress edits
- **Transform `input_raw` and `__imageData` variables** — JS transform context for vision workflows

### Frontend Visual Polish

- **Stagger fade-in-up animations** — List rows, card grids, and skeletons animate in with staggered delays
- **Chart entrance animations** — LineChart, BarChart, PieChart animate on first render with gradient fills
- **Shimmer skeleton effect** — Skeleton loading upgraded from pulse to shimmer sweep animation
- **Page-level fade-in transition** — Pages wrapped in `PageLayout` fade in smoothly on mount
- **Chat message entrance animation** — New chat messages animate in as they appear
- **Theme switch transition** — Theme toggle has color transition and icon rotation animation
- **Card hover effects** — Cards lift with shadow on hover
- **Component library sidebar** — Flat grid layout with post-install highlight animation

### Changed

- **Time context** — Reduced to single concise line
- **HistoryConfig mode-aware** — Focused mode uses `HistoryConfig::focused()`
- **Frontend Memory tab** — Knowledge Files cards + Execution Journal timeline
- **Extension default memory limit** — Raised from 2048MB to 4096MB for ML model workloads
- **Cross-platform library search path** — `LD_LIBRARY_PATH` / `DYLD_LIBRARY_PATH` / `PATH` for shared libraries
- **Extension IPC channel initialization** — Event channel created before stdin reader, preventing race conditions

### Removed

- **ShortTermMemory, LongTermMemory, MemorySummary, ImportantMemory, TaskProfile** — Deleted from `AgentMemory`
- **Complex memory write logic** — ~260 lines removed
- **Context fingerprint functions** — ~60 lines removed
- **System prompt editor** — Removed custom system prompt textarea from agent editor

### Fixed

- **Vision tool mis-invocation** — System prompt no longer tells LLM to "Use the vision tool" when images are already embedded. Changed to "(included in message)" label
- **Vision tool incomplete data URL handling** — `resolve_image()` correctly parses `image/jpeg;base64,...` without `data:` prefix
- **Agent base64 image cleaning** — URL-safe char conversion, whitespace stripping, padding fix, decode+re-encode validation
- **Gradient ID collisions** — LineChart and BarChart gradient fills use unique IDs per chart instance
- **O(n²) stagger index** — Reduced to O(1)
- **Extension memory limit** — RLIMIT_AS raised to 4GB to accommodate ONNX Runtime + rayon thread pools
- **Cargo.lock tracked** — Removed from .gitignore for reproducible CI builds
- **CI build fallback** — Platform-specific bundle type fallback (deb+rpm / app / nsis) when full build fails
- **Transform `extensions.invoke()` JS API** — Properly creates `extensions` object with `invoke` method
- **Extension health state display** — Distinct colors for Error/Warning/Stopped states
- **Extension crash recovery error reporting** — All failure paths write error status to storage
- **Extension list filter** — Added Stopped/Failed filter option
- **Device telemetry image normalization** — Normalized on initial fetch
- **Dashboard drag jump** — Freeze container width during drag/resize
- **Component `_raw` telemetry parsing** — Parses JSON string telemetry for flat key access
- **Component render error isolation** — Per-cell ErrorBoundary prevents cascade failures
- **Extension `hasDeviceBinding` metadata** — Correctly propagates through registries
- **Agent knowledge file auto-init on update** — Legacy agents get `task-understanding.md` on first update
- **Timeseries default timeRange** — Changed from 1h to 24h
- **Agent card status glow** — Removed excessive ring effects
- **Error messages for 400/422** — Shows actual server error message
- **Metric tag extraction** — Expanded exclusion list for timeline
- **Dynamic config multi-data-source** — Respects `max_data_sources` from widget manifest
- **IPC event channel error logging** — Prevents silent message drops
- **NE101 ROI canvas** — Fixed React error #310 (conditional hook), image from device store, canvas layout fix

### Backward Compatibility

- All new `AgentMemory` fields use `#[serde(default)]` — old redb data deserializes gracefully
- No data migration required

---

## [v0.8.5] - 2026-06-03

### Added

- **MQTT broker TLS certificate generation** — Self-signed certificate generation with proper X.509 extensions (Key Usage, Extended Key Usage, Subject Alternative Names including system hostname). 1-hour clock skew tolerance for date validation. Certificate paths respect `NEOMIND_DATA_DIR` environment variable
- **MQTT broker restart API** — `PUT /api/mqtt/broker-config` now triggers automatic broker restart when port, listen address, or TLS settings change. Includes rollback logic: if the new broker fails to start, automatically restarts with the previous configuration and rebuilds the internal MQTT adapter
- **MQTT TLS status in API** — `GET /api/mqtt/status` now returns `tls_enabled` field indicating whether TLS is active on the embedded broker
- **Credential cache for MQTT authentication** — In-memory `CredentialCache` with `Arc<RwLock>` avoids redb lookups on every MQTT CONNECT packet. Cache auto-refreshes when credentials are added or deleted via the API. Custom `Debug` impl redacts sensitive fields (system password)
- **Restart lock for embedded broker** — `tokio::sync::Mutex` prevents concurrent restart operations from racing against each other
- **Environment variables documentation** — New `docs/guides/en/16-environment-variables.md` and `docs/guides/zh/16-environment-variables.md` covering server, auth, LLM, extension, CLI, and Docker configuration
- **Brotli compression** — HTTP response compression now supports both Gzip and Brotli encoding

### Changed

- **Extension command timeout increased** — Default FFI command timeout raised from 30s to 300s (configurable via `NEOMIND_FFI_TIMEOUT_SECS`). In-flight request timeout aligned to match. Prevents timeout errors for long-running extensions (YOLO, video processing)
- **Extension memory limit** — Default memory limit for isolated extensions raised from 512MB to 2048MB, accommodating ML model workloads
- **Dashboard desktop layout** — Desktop sidebar is now always expanded (removed collapsible toggle). Simplified navigation with fixed sidebar layout. Tab bar mode remains available as an alternative
- **Telemetry data fetching** — Optimized caching strategy with 10s fetch timeout (up from 5s) for better reliability over slow connections. 60s bucket alignment and 30s TTL maintained for cache freshness
- **MQTT broker config validation** — Listen address must be a valid IP. TLS cannot be enabled without pre-configured certificates. Credential uniqueness check before adding duplicates
- **Deploy configuration** — Updated nginx example with improved WebSocket proxying and Docker-specific port mappings

### Fixed

- **MQTT broker restart Send safety** — Fixed `Box<dyn StdError>` held across `.await` boundary in restart rollback path, which caused handler trait resolution failure. Store data is now extracted before any async operations
- **Telemetry test data integrity** — Fixed `test_image_retrieval_performance` and `test_telemetry_concurrent_write_performance` flaky failures by adding explicit `flush()` calls before querying. Tests were reading from redb while data was still in the write buffer
- **Extension test timeout assertion** — Updated `test_command_descriptor_dto` assertion to match the new 120s default timeout value
- **Dashboard component rendering** — Fixed extension component loading in `ComponentRenderer` by removing interfering `ErrorBoundary` wrapper
- **Dashboard mobile edit mode** — Fixed state reset when toggling mobile edit mode
- **Extension IPC routing** — Fixed stale reference in extension stream routing after process restart
- **Native confirm dialogs** — Replace `window.confirm()` with styled custom confirm dialog for dashboard widget removal and LLM instance deletion
- **Dashboard widget drag jumping** — Freeze container width measurement during drag/resize operations to prevent layout reset from stale store positions

### Changed

- **Component library and marketplace grids** — Replaced fixed responsive breakpoints with auto-fill columns (max 6) for better use of screen width on large displays

---

## [v0.8.4] - 2026-06-02

### Changed

- **System prompt slim-down (73% token reduction)** — Reduced AI agent system prompt from ~7,500 to ~1,800 tokens, freeing ~5,700 tokens per request for conversation history (+32% available context). Three-layer architecture: (1) system prompt for core decision rules, (2) CLI `--help` for command details loaded on demand, (3) skill tool for complex workflows loaded on demand. Removed redundant CLI reference table (already in shell tool JSON description), few-shot examples (modern tool-calling models don't need them), and consolidated duplicate rules across PRINCIPLES/RESPONSE_FORMAT/THINKING_GUIDELINES. Added Typical Workflows table, response format patterns, error handling hint, and vision hint. Integrated vision capability detection so multimodal models automatically receive image analysis instructions.

### Added

- **Vision tool** — AI agent can now analyze images from HTTP URLs, local files, data URLs, or raw base64 using a vision-language model (VLM). Auto-detects VLM backends via `supports_multimodal` capability and registers the tool automatically. Security hardened: SSRF protection with per-redirect validation, symlink-safe file reads via canonicalize-then-validate, MIME allowlist for data URLs, file extension whitelist with magic bytes validation, 10MB size limit. VLM backend selection follows priority: explicit config → active backend → first multimodal instance
- **4 new bridge extensions** — Home Assistant Bridge, LoRaWAN Bridge, Modbus Bridge, and Uink-RMS Bridge added to the extension marketplace for broader IoT protocol coverage
- **Layered multimodal capability detection** — Replace hardcoded heuristic with 3-tier resolution: LiteLLM registry (2,748 embedded model entries) → conservative heuristic → false. Add user override endpoint (`PATCH /api/llm-backends/:id/capabilities`), background refresh loop for Ollama instances (hourly `/api/show` polling), and source tracking (`user_override` > `runtime_api` > `registry` > `heuristic`). HTTP images pre-encoded to base64 for Ollama compatibility
- **i18n comprehensive standards** — Added section 12 to DESIGN_SPEC.md covering namespace rules, key naming convention (`{page}.{section}.{field}`), cross-namespace references, and common mistakes checklist
- **Dashboard tab bar layout mode** — Alternative to the left sidebar: a horizontal scrollable tab bar rendered inline in the toolbar header, freeing the full content width for the dashboard grid. Toggle via `PanelTop` button in the sidebar header or `PanelLeft` button on the tab bar; preference persisted in `localStorage` (`neomind_dashboard_layout_mode`). Active tab has a distinct `bg-muted` style with an elastically-expanding `⋮` action menu (cubic-bezier overshoot easing, 200ms) that reveals Rename/Delete on hover — no floating overlay. Left side has `[≡ sidebar][+]` controls; tab names truncate at 200px with full-name tooltip on hover. Layout mode is independent from the existing sidebar collapse state and fullscreen mode
- **Tooltips on dashboard toolbar action buttons** — Edit/Done, Add Component, Share, and Fullscreen buttons now use Radix `Tooltip` (300ms delay) instead of native `title=` attribute, matching the hover-label pattern used elsewhere in the dashboard chrome
- **i18n keys for tab bar** — Added `sidebar.switchToTabs` and a new `tabBar.*` namespace (`newDashboard`, `namePlaceholder`, `deleteTitle`, `deleteDescription`, `delete`, `rename`, `switchToSidebar`) in both `en` and `zh`

### Changed

- **Extensions page empty state redesign** — Replaced generic "No extensions found" with a rich ecosystem showcase: horizontal marquee scrolling 12 real extension preview cards (YOLO Video, Face Recognition, BACnet, Modbus, LoRaWAN, ONVIF, Home Assistant, Stream Player, Weather, OCR, OPC-UA, Image Analyzer), 8 category tags matching actual extension types, and CSS-only animation with edge fade and hover-pause

- **Messages page filter redesign** — Replaced heavy Sheet side-drawer with a lightweight Popover dropdown filter panel. Compact pill-style buttons replace bulky collapsible sections for Severity, Status, and Category filters. Removed CollapsibleSection component and unused Sheet/Separator/ChevronDown imports

- **Multimodal image upload avoids redundant vision tool call** — When a user uploads an image to a multimodal-capable model (e.g., GPT-4o, qwen-vl), the image is sent directly as native `Content::Parts` and the `vision` tool is filtered from the tool list. This prevents the model from calling the vision tool on images it can already see, eliminating a redundant LLM round-trip (following industry best practice: OpenAI, Anthropic, CrewAI all recommend native multimodal over tool-mediated vision)

- **Dashboard telemetry data split** — Separated real-time device telemetry (`deviceTelemetry` Record) from the `devices` array to eliminate cascading re-renders. Previously, every WebSocket metric update mapped over the entire `devices` array, causing all dashboard components to re-render. Now high-frequency metric writes only update a per-device telemetry map, while the `devices` array reference stays stable. Dashboard components use targeted selectors with `shallow` equality to re-render only when their bound device's telemetry changes. This reduces re-renders from O(n) per metric update to O(1).

- **Clippy cleanup** — Fixed 45 clippy warnings across 4 crates (`neomind-cli-ops`, `neomind-storage`, `neomind-agent`, `neomind-api`). Introduced `CredentialValidator` type alias for complex closure types, replaced `iter().cloned().collect()` with `to_vec()`, used `strip_prefix` instead of manual slicing, and resolved `await_holding_lock` in shutdown by cloning `Arc` before dropping the read guard
- **Dashboard list sorted by creation time** — Both sidebar and tab bar now display dashboards ordered by `createdAt` ascending (oldest first, newest at end), independent of backend fetch order or sync remapping. Newly created dashboards always appear at the end of the list
- **Sidebar collapsed-mode cleanup** — Removed the `+` (new dashboard) button and its divider from the collapsed sidebar view. The collapsed column now shows only the dashboard icon list; creation requires expanding the sidebar first or using the tab bar's `+` button
- **Sidebar item always shows component count** — Removed the `count > 0` guard so dashboards with 0 components display "0 components" instead of hiding the count row entirely

### Fixed

- **i18n ZH translations** — Batch translated 422 missing Chinese keys across 15 namespace files. Achieved EN/ZH parity (5,370 keys each, 17 active namespaces)
- **i18n reference consistency** — Fixed `ConfigFieldComponents.tsx` using default namespace instead of explicit `dashboardComponents`. Fixed 6 files using `t('common.xxx')` anti-pattern in default namespace context (`DeviceBindingConfig`, `MessageChannelsTab`, `MessagesTab`, `messages.tsx`). Fixed `settings.tsx` listing unregistered namespaces (`llm`, `connections`)
- **Create-dashboard navigation race** — `handleDashboardCreate` now `await flushSync()` and reads the final `currentDashboardId` from the store before navigating, so the URL receives the stable post-remap dashboard id. Previously the URL would hold the local temporary id while the store later updated to the backend-assigned id, causing the URL ↔ Store sync to bounce the user off the newly created dashboard

### Removed

- **~8,900 lines of dead frontend code** — Removed 30+ unused components, hooks, and utility modules that were superseded by page-level implementations:
  - `components/automation/` — AlertsTab, AutomationCreatorDialog, AutomationsTab, TransformsTabContent, TransformExecutionHistory (replaced by `pages/automation-components/`)
  - `components/devices/` — DeviceControl, DeviceRealtime, TemplatePreview (replaced by `pages/devices/`)
  - `components/extensions/` — ExtensionDataSourceSelector, ExtensionMetricSelector, ExtensionToolSelector, ExtensionTransformConfig (inlined into pages)
  - `components/shared/` — BulkActionBar, FullScreenEditor, KeepAlive, MonitorStatsGrid, SearchBar, SearchResultsDialog (unused)
  - `components/layout/` — SubPageHeader (unused)
  - `hooks/` — useApiData, useComponentPerf, useDialog, useInterval, useLoadingButton, useMessages (replaced by store-level fetchCache pattern)
  - `lib/` — extension-stream-hooks, fetch-with-timeout, react-query-hooks, status/utils, validation/utils, related test
- **Dead i18n namespaces** — Removed `commands.json` (58 keys, 0 references), `navigation.json` (12 keys, duplicated by `common.json` `nav`/`navShort`), `tools.json` (11 keys, unused), and orphaned camelCase `dashboardComponents.json` (merged into hyphenated version)
- **245 duplicate i18n keys from common.json** — Removed sections that existed identically in `dashboard-components.json`: visualDashboard, sizes, imageDisplay, imageHistory, layerDisplay, mapDisplay, markdownDisplay, placeholders, range, searchBar, videoDisplay, webDisplay, common

---

## [v0.8.3] - 2026-06-01

### Added

- **Docker deployment** — Production-ready multi-stage Dockerfile (Node 20 frontend + Rust 1.85 backend + Alpine runtime), docker-compose.yml with named volume persistence, health check, and `.env.example` configuration template. Single container includes API server, Web UI, embedded MQTT broker, and extension runner


- **Agent experience learning** — New per-execution insight extraction and LLM-driven task profile reflection. Agent memory now accumulates actionable knowledge over time instead of just recording what happened:
  - `MemorySummary.insight` — Inline insight from main LLM output (focused mode) or deterministic extraction (free mode: failure reasons, alert/command triggers, >20% baseline deviation, anomaly keywords). Zero extra LLM calls
  - `TaskProfile` — Evolving task-level knowledge summary (max 500 chars) generated by LLM reflection when ≥5 insights accumulated (first time) or 6-hour staleness (updates). Includes version, execution count, and freshness tracking
  - Task Knowledge injected as highest-priority section in `build_history_context()` for LLM decision-making
  - Short-term summary cards now display insights with lightbulb icon in agent detail panel
  - API DTOs (`MemorySummaryDto`, `AgentMemoryDto`) expose `insight` and `task_profile` fields
  - i18n support for Task Knowledge and Recent Key Findings (en/zh)
- **`web_fetch` tool** — AI agent can now fetch URL content directly. Returns cleaned text (HTML stripped) or raw content with configurable max length (default 5000, max 50000 chars). Security: SSRF protection blocks private/local IPs (localhost, 10.x, 192.168.x, 172.16-31.x, IPv6 unique local, link-local, IPv4-mapped IPv6), validates redirect targets, enforces 15s timeout and 1MB response limit. Content-Type media type parsing prevents binary bypass via parameter injection
- **`file_write` tool** — AI agent can create or overwrite files within allowed directories (data dir + `NEOMIND_ALLOWED_WRITE_DIRS` env var). Atomic writes via temp-file-then-rename. Supports all text file types (.rs, .toml, .py, .js, .json, .md, .conf, etc.). Blocks binary extensions (.so/.dll/.exe/.sys) and .env files. Content limit: 1MB. Auto-creates parent directories by default. Preserves file permissions on overwrite
- **`file_edit` tool** — AI agent can perform precise string replacement in existing files. Parameters: `old_string`/`new_string` with optional `replace_all`. CRLF/LF line ending normalization for cross-platform matching. File size limit: 10MB. Error messages include file preview for context when old_string not found. Atomic write preserves file permissions
- **`path_validator` module** — Shared security layer for file tools. Symlink escape prevention via `find_existing_ancestor()` + canonicalization. Path traversal (`..`) detection at component level. `NEOMIND_ALLOWED_WRITE_DIRS` env var for extension development directories
- **Memory tool 2-file API** — New file-based memory endpoints: `GET/PUT /memory/file/{category}` for direct file read/write. Memory tool now supports custom category files (`custom/{name}.md`) and per-request session binding via shared handle
- **Device list grouped by type** — `neomind device list` now groups devices by `device_type`, shows metric schema with example values from online devices (parallel enrichment), and truncates large lists (>50 devices) for token budget protection
- **LLM backend create via CLI** — `neomind llm create` registers new LLM backend instances from the command line
- **Thinking model loop detection** — Ollama backend detects and cuts off runaway thinking (loops, excessive length) for qwen3/deepseek-r1 models
- **Chat page context injection** — When the global chat FAB is opened from a page (dashboard, devices, automation, etc.), a short neutral context string (`[context] page:dashboard "name", N components`) is automatically prepended to the first user message so the AI knows which page the user is on. Context is reactive to route changes, injected only on the first message per session, and resets on new conversation
- **Dashboard community components split** — Component library now separates "My Components" (locally created / AI-generated) from "Marketplace" (installed from registry). Added `source` field to distinguish origins, with reinstall support for local components to refresh updated bundles
- **System context resource inventory** — Periodic background task gathers device/agent/extension/dashboard names and writes to KNOWLEDGE.md `<!-- system-context -->` marker section (800 char limit, 10min interval). AI now knows what resources exist without tool calls
- **LLM chat/agent summarization** — Periodic background task uses LLM to summarize recent chat sessions → `<!-- chat-summary -->` in USER.md (200 chars) and active agent execution patterns → `<!-- agent-summary -->` in KNOWLEDGE.md (300 chars). Configurable backend selection and 2h interval

### Changed

- **Agent context builder optimized** — Merged duplicate Execution History + Short-term Memory sections into single "Recent Execution History". Filtered low-value learned patterns. Baselines now show human-readable device names from resources instead of raw metric IDs
- **Agent reflection prompt language** — All LLM reflection prompts use English for consistency
- **Focused mode LLM fallback** — Deterministic fill from `situation_analysis` when small models omit `reasoning_steps`/`conclusion`/`decisions` fields. Uses `serde_json::Value` for `insight` to tolerate non-string LLM output (true, 0, null). No extra LLM calls, no circular risk
- **Memory system refactor** — Replaced old LLM-based chat extraction (`POST /api/memory/extract`) with marker-based periodic summarization. Removed dead extraction pipeline (compat stubs, category files). Memory writes are now: (1) user via memory tool, (2) background periodic summaries. Old `user_profile.md`/`task_patterns.md`/`domain_knowledge.md` files (417KB of noise) replaced by clean USER.md/KNOWLEDGE.md
- **Memory config defaults** — `agent_char_limit`: 500→1000, `summary_interval_secs`: 3600→7200, `system_context_interval_secs`: 300→600. Added `summary_backend_id` field for selecting LLM backend for summarization (defaults to active backend)
- **Agent short-term memory** — Capacity increased from 10→20 entries. `summarize_agent_context()` now includes both situation and conclusion for richer context. Learned patterns get time-based confidence decay (10%/week, removed after 28 days). Baselines pruned when data sources no longer present
- **Memory config dialog** — Replaced manual toggle switch with Radix UI Switch component. Added LLM backend selector for summarization. Removed Extract button from toolbar
- **Tool prompt architecture** — `builder.rs` now includes structured tool descriptions (Type 1: shell, Type 2: skill, Type 3: file/web) with parameter docs, security notes, and usage examples in the system prompt. `TOOL_STRATEGY` section guides LLM on when to use each tool type
- **Memory tool actions expanded** — Added `read_file`, `write_file`, `list_files` actions for direct file manipulation alongside existing category-based actions
- **Memory panel unified** — Custom memory files merged into the same table as user/knowledge files. Single unified dialog for view/edit. "Add File" button in tab actions bar. Eliminated ~200 lines of duplicate state and dialogs
- **Memory stats API unified** — `GET /api/memory/stats` now returns `{ files, custom_files }` using the new `store.stats()` API instead of deprecated `all_stats()`. Fixed stats display (was always showing 0 chars due to key mismatch)
- **Code formatting cleanup** — `cargo fmt` applied across agent, storage, API crates for consistent formatting
- **Table vertical alignment** — ResponsiveTable cells now use flex centering for consistent vertical alignment across rows with varying content heights
- **Global chat floating window** — Replaced full-screen backdrop overlay with a fixed-size floating window (380×560 desktop, 70vh mobile) anchored to bottom-right. Users can now chat while viewing Dashboard/device pages behind the window
- **Memory scheduler cleanup** — Removed system resource summary job that wrote stale "System Resources" sections to KNOWLEDGE.md every schedule interval, wasting the char budget on transient data queryable live via CLI tools

### Removed

- **`ai_metric` tool** — Removed the AI Metric tool and all related infrastructure. This tool allowed LLM agents to write custom time-series metrics (`ai:{group}:{field}`), but had no reliable use case — the Memory system already covers cross-session knowledge persistence. Full cleanup across backend, frontend, i18n, and docs:
  - **Rust**: Deleted `crates/neomind-agent/src/toolkit/ai_metric.rs` (614 lines). Removed `AiMetricsRegistry` from `AgentState`, `init_tools()`, `refresh_extension_tools()`. Removed `DataSourceType::Ai` enum variant and `DataSourceId::ai()` from `neomind-core`. Removed `collect_ai_sources()` from data handler. Removed `"ai:"` from `KNOWN_PREFIXES` in telemetry migration
  - **Frontend**: Removed `'ai-metric'` from `DataSourceType` union, `AIMetricDataSource` interface, `aiGroup` field. Cleaned all 6 config schema files, `UnifiedDataSourceConfig`, `DataSourceIndicator`, `DualModeSourceField`, `ComponentConfigBuilder`, `componentDataApi`
  - **i18n**: Removed `aiMetric`, `aiMetricDesc`, `noAiMetrics`, `aiGroupPlaceholder` from en/zh locales
  - **Docs**: Removed ai_metric references from agent (en/zh), tools (en/zh), storage (en/zh), and web dashboard (en/zh) documentation
- **`session_search` tool** — Removed conversation history search tool. LLM already has full conversation context in its prompt window, making self-search redundant. Memory system handles cross-session knowledge persistence. Deleted `crates/neomind-agent/src/toolkit/session_search.rs` (127 lines)
- **`think` tool** — Removed the explicit thinking tool (338 lines). Thinking models now handle reasoning internally via streaming. The `think` namespace removed from LLM tool routing and staged agent filter
- **`ToolFilter` dead code** — Removed unused `ToolFilter` struct, `filter_by_intent()`, `intent_prompt()` from `staged.rs` (~130 lines). Removed dead `classify_intent()`, `get_intent_prompt()`, `filter_tools_by_intent()` methods and `tool_filter` field from `LlmInterface` in `llm.rs` (~140 lines including tests). Removed unused `IntentCategory::namespace()` and `IntentClassifier::classify_category()`
- **5 unused agent components** — Deleted `AgentMemoryDialog`, `AgentExecutionsList`, `AgentListPanel`, `AgentLogicPreview`, `AgentsList` (0 references, ~1626 lines of dead code)
- **Chat memory toggle** — Memory is now always enabled (configurable via settings). The per-session toggle was redundant since the memory tool provides on-demand access regardless of snapshot preload
- **Chat skill selector** — LLM already auto-selects skills via the `skill` tool based on user intent. Manual preloading was redundant and added UI clutter
- **Memory extract endpoint** — Removed `POST /api/memory/extract` and frontend Extract button. Old LLM-based chat extraction produced 417KB of noisy data (3551 entries, mostly duplicates). Replaced by background periodic summarization
- **Dead memory modules** — Removed `compat.rs` (empty stubs), `lifecycle.rs` (unused hooks), `short_term.rs`, `mid_term.rs`, `long_term.rs`, `tiered.rs`, `bm25.rs`, `embeddings.rs` (all unused after refactor)
- **Unused `write_last_resource_summary_time`** — Removed dead method from `MarkdownMemoryStore`

### Fixed

- **Custom Layer background image UI redesign** — Merged awkward two-field layout (URL + separate file upload) into a single inline field with URL input + Upload button, matching ImageSourceField pattern
- **LayerEditorDialog save button i18n** — Added missing `common.save` translation key so save button shows localized text instead of raw key
- **Missing zh translations for spatial config** — Added `backgroundType`, `backgroundImageUrl`, `layerItemBinding`, `manageLayerItems` and related keys to Chinese locale
- **Memory tool write lock** — Write operations (add/replace/remove/create) now use `store.write().await` instead of `store.read().await` to prevent read-modify-write race conditions
- **Memory tool first-match-only** — `replace`/`remove` actions now use `.replacen(..., 1)` instead of `.replace()` to prevent multi-replace data corruption
- **Memory tool chars vs bytes** — All "X chars" messages now use `.chars().count()` instead of `.len()` for correct UTF-8/Chinese text reporting
- **Memory tool list action** — `target` parameter is now optional for `list` action (was incorrectly required)
- **Memory snapshot budget** — Added hard truncation fallback when user content alone exceeds 5000 char budget
- **Refresh extension tools** — Memory tool is now re-registered during `refresh_extension_tools()` to prevent it from disappearing after extension refresh
- **All compiler warnings resolved** — Zero warnings across neomind-storage, neomind-agent, neomind-api crates
- **Session file path traversal** — Added `validate_session_id()` to block `../` and `/` in session IDs, preventing arbitrary file access
- **Char counting consistency** — Fixed `write_file()`, `stats()`, and agent stats to use `.chars().count()` instead of `.len()` for correct UTF-8/Chinese text handling
- **Extraction lock resilience** — Extraction guard now uses `Drop` pattern to ensure lock is released even on panic, preventing permanent lock-out
- **Missing i18n keys** — Added `systemMemory.extract` and `systemMemory.custom.description` to en/zh locales
- **Session sidebar card overflow** — Fixed Radix ScrollArea Viewport injecting `display:table` + `min-width:100%` causing cards to overflow. Added CSS override to Viewport component and proper `min-w-0` flex constraints for text truncation
- **Session action buttons** — Edit/delete buttons now compact (`h-4 w-4`) and absolutely positioned floating on card right side with hover reveal, instead of inline layout
- **Dashboard stuck skeleton screens** — Fixed three root causes: loading counter leak on telemetry-only sources, retry storm (reduced to 1 retry at 500ms), and added 3s hard deadline force-clear
- **Dashboard cross-tab sync** — Emit `DashboardUpdated` event on CRUD operations. VisualDashboard subscribes for real-time sync across browser tabs
- **Dashboard chart tooltip crash** — Fixed crash when rendering telemetry point objects `{timestamp, time, value}` as React children. LineChart now correctly extracts numeric values
- **Community widget data flow** — Fixed `fetchData` prop not reaching community widgets due to missing `installedComponents.length` dependency in rendering useMemo. Removed 2.5s fetch delay for immediate registry sync
- **Data source editor binding** — Fixed `dataSourceToSelectedItems` not recognizing `type:"telemetry"` and `type:"device"` with metric fields, causing editor to not show bound state for AI-created data sources

---

## [v0.8.2] - 2026-05-29

### Changed

- **DataSource unified Source+Mode architecture** — Replaced 12 legacy `type`-based routing with 4 unified fields (`source`/`id`/`field`/`mode`). New `DataSourceSource` (device/extension/system/transform/ai) and `DataSourceMode` (latest/timeseries/command/info/list) types provide clean orthogonal dimensions. `migrateToUnified()` bidirectionally populates both old and new fields for zero-migration backward compatibility. Removed 14 type guard functions, legacy switch statements across 6 sub-hooks. All routing now uses mode-based logic with fallback to legacy fields
- **usePollingSource replaces useSystemSource** — New generic HTTP polling hook supporting latest, list, and timeseries accumulation modes. System metrics now support client-side historical accumulation (pruned by `timeRange`/`limit`). Deleted `useSystemSource.ts` entirely. `pollDataSource()` dispatch in fetch.ts provides extensible source routing for future data sources (rule lists, message lists, external APIs)
- **Config UI outputs unified fields** — `selectedItemsToDataSource` now outputs `source`/`id`/`field`/`mode` alongside legacy `type`. `suggestedMode` prop enables per-component mode hints (LED→latest, Chart→timeseries, Toggle→command, Map→info). Eliminates sourceTransform round-trips for new configurations
- **isImageDataSource refactored** — Changed from 3-arg `(params, transform, metricId)` to single-arg `(ds)` pattern. Updated 8 call sites across 4 files
- **Community/extension component fetchData API** — New `resolveDataSourceData()` utility and `fetchData` prop injection in ComponentRenderer for community/extension components. Provides mode-aware data fetching without React hook dependency

### Fixed

- **Instant telemetry initial rendering** — Telemetry-bound components (LED, ValueCard, ProgressBar, etc.) now read initial values from `store.current_values` instead of waiting for HTTP API. New `readTelemetryInitialValues` in `useStoreSource` creates synthetic data points from store, eliminating loading flash on dashboard open
- **Enhanced telemetry retry** — `useTelemetrySource` now retries with exponential backoff on transient failures instead of showing permanent error state
- **Dashboard component count mismatch** — Removed destructive `isDataSourceValid` filter in `fetchDashboards` that silently deleted components with incomplete data sources
- **Camera hardware lock leak** — `VideoDisplay` CameraAccess now properly stops MediaStream tracks on unmount via `streamRef` + cleanup
- **Dual/triple fullscreen rendering** — VideoDisplay, MapDisplay, CustomLayer no longer render content inline AND via portal simultaneously (`{!isFullscreen && content}` pattern)
- **useTelemetrySource timer leaks** — Retry setTimeout and fetch timeout promise now tracked via refs and cleaned up on unmount
- **LayerEditorDialog cancel data loss** — Cancel button now calls `onOpenChange(false)` instead of `onSave(undefined)` which wiped all layer bindings
- **Config save dataSource priority** — Simplified `handleSaveConfig` to 2 authoritative locations instead of 5, preventing restoration of intentionally-cleared data sources
- **Duplicate dashboard creation** — `HybridDashboardStorage.syncToApi` now only syncs dashboards with existing server ID mapping
- **Stack overflow on large telemetry arrays** — Replaced `Math.min(...array)` / `Math.max(...array)` with `.reduce()` pattern across 10 files to handle arrays >100K elements
- **createStableKey stack overflow** — Added depth limit (MAX_DEPTH=10) to prevent infinite recursion on deep/circular references
- **Sparkline crash on sparse data** — Added guard for `< 2` data points before rendering
- **getLinearGradient OKLCH handling** — Now uses proper `colorWithAlpha()` helper instead of raw string concatenation
- **normalizeDataSource empty array** — `[]` input no longer wrapped as `[[]]`
- **imageUtils cache memory bloat** — Inputs >10KB (base64 camera frames) skip caching to avoid multi-MB string retention
- **SharedDashboard i18n** — Replaced 6 hardcoded English error messages with `t()` calls
- **Video display config i18n** — Replaced hardcoded Chinese strings with `t()` calls
- **Chart useMemo stale data** — LineChart, BarChart, PieChart now include `sources`, `getSeriesName`, `getDeviceName` in dependency arrays
- **Renderers missing builtIn types** — Added `counter` and `metric-card` to builtInTypes Set and builtInComponentMap
- **DashboardGrid redundant data-grid** — Removed `data-grid` attribute from child elements (layouts prop is authoritative)
- **ImageDisplay fullscreen portal** — Fullscreen overlay now uses `getPortalRoot()` instead of inline rendering
- **Dashboard switch state cleanup** — `mobileSelectedId` and `mobileEditBarOpen` reset on dashboard switch
- **Deep clone on template apply** — `applyTemplate` now uses `JSON.parse(JSON.stringify())` for proper deep clone
- **configComponentId reset on delete** — `deleteDashboard` now clears `configComponentId` and `configPanelOpen`

### Fixed (Round 10)

- **Error Boundary for dashboard components** — Extension/community component runtime errors no longer crash the entire dashboard page; graceful error card with localized message
- **localStorage quota recovery** — `LocalStorageDashboardStorage.save()` now catches `QuotaExceededError`, clears stale data, and retries write
- **Hybrid storage sync race condition** — Rapid edits to a local dashboard before first server sync now preserve latest changes instead of overwriting with stale server state
- **Position validation** — `moveComponent` now clamps negative x/y to 0 and dimensions to minimum 1; `positionFromDTO` applies same validation to API responses
- **Registry validation** — Dynamic and community component registries reject types that shadow built-in widget types (e.g. registering `"line-chart"` as extension)
- **Missing type guards** — Added `isExtensionMetricSource()` and `isExtensionCommandSource()` type guards for discriminated union coverage

### Fixed (Round 11)

- **Mobile edit mode state leak** — Exiting edit mode on mobile now resets `mobileSelectedId` and `mobileEditBarOpen` instead of leaving stale mobile UI
- **Mobile drag/resize disabled** — Grid drag and resize disabled on touch devices to prevent conflicts with scrolling and touch interactions
- **Extension uninstall cleans all dashboards** — Unregistering an extension now removes its components from ALL dashboards, not just the current one
- **ComponentRenderer unmounted state updates** — Added mountedRef guard to prevent React warnings from async state updates after component unmount
- **Mobile touch targets** — Action buttons in mobile edit mode increased to 44px height (was 32px) for proper touch accessibility
- **Mobile selection overlay** — Split overlay into separate selected/unselected states; component content is now interactive when selected

### Changed

- **Dashboard configSchemas registry pattern** — Replaced 2982-line monolithic `configSchemas.tsx` switch statement with a modular registry pattern. Schema generators are now organized into `builtIn/` sub-modules (indicators, charts, controls, display, spatial, business) plus a `dynamic.tsx` handler for extension/community/custom components. No user-visible behavior changes
- **Dashboard store: eliminated slice circular dependencies** — Removed module-level `_scheduleSync`/`_flushSync` variable exports from `dashboardCrudSlice`. `scheduleSync()` and `flushSync()` are now proper slice methods accessed via `get()`, eliminating fragile module-level getter pattern
- **DataSource discriminated union types** — Added 12 type-specific interfaces (`DeviceDataSource`, `CommandDataSource`, `SystemDataSource`, etc.) with type guards (`isDeviceSource()`, `isRealtimeSource()`, `isPolledSource()`, etc.). Legacy flat `DataSource` interface preserved for backward compatibility. Updated `useDataSource` pipeline and `dashboardHelpers` to use type guards
- **useDataSource simplified state management** — Replaced 12-action `useReducer` state machine with flat `useState` + loading ref counter. Removed `activeFetchSource` tracking, `FETCH_EMPTY_RETRY`, and `FORCE_CLEAR_LOADING` actions. Loading state is now a simple counter (loading = counter > 0) managed by `startLoading`/`finishLoading` callbacks

---

## [v0.8.1] - 2026-05-27

### Added

- **Embedded MQTT broker auth & TLS management** — Redesigned `EmbeddedBroker` with `external_auth` callback for redb-backed credential validation, stop/restart lifecycle, and TLS support (cert/key paths). Broker now loads config from redb at startup and validates connections against stored credentials
- **MQTT credential storage** — New redb tables (`mqtt_credentials`, `mqtt_credentials_by_username`) for MQTT username/password management. Full CRUD methods with automatic index maintenance in `neomind-storage`
- **Embedded broker config API** — New endpoints `GET/PUT /api/settings/broker` for reading and updating embedded broker configuration (auth mode, TLS, credentials). Changes take effect on broker restart
- **Embedded broker config UI** — New `EmbeddedBrokerConfigDialog` component with auth mode toggle (anonymous/credential), credential management (add/delete), and TLS configuration (cert/key paths). Full en/zh i18n support
- **CLI: device drafts commands** — New `neomind device drafts` subcommand group (`list`, `get`, `approve`, `reject`, `config`) for managing auto-discovered device drafts. Full workflow: list pending → inspect samples → approve with name/type → or reject
- **CLI: device webhook-url** — New `neomind device webhook-url <ID>` command to retrieve the HTTP push URL for webhook adapter devices
- **CLI: extension config** — New `neomind extension config <ID>` to view config, `--set '<JSON>'` to update. Replaces manual API calls for extension configuration
- **CLI: API client auth retry** — All API client methods (GET/POST/PUT/DELETE/multipart) now automatically retry on 401 with refreshed API key from redb. API key stored in `RwLock` for thread-safe refresh
- **CLI: health check via API** — `neomind health` now queries actual LLM backend status via API instead of checking environment variables. Shows backend count, active backend ID, and setup hints
- **CLI: system info with TLS/auth/credentials** — `neomind system info` now exposes MQTT broker TLS status, auth mode, and credentials for AI agent onboarding guidance
- **Broker connection guide in Add Device dialog** — New step showing embedded broker connection details (host, port, credentials) to simplify device onboarding

### Changed

- **CLI: shell tool reference updates** — `transform test` renamed to `test-code`, `extension get` aliased to `info`, agents created as `active` by default (no longer need `control <ID> active`), push target type auto-detected from config
- **CLI: shell operator fallthrough** — Commands containing pipes (`|`), redirects (`>`), or stderr redirects (`2>`) now fall through to real shell execution instead of internal routing
- **CLI: DSL parser validation** — Rule engine now rejects function-call syntax (e.g., `device.metric(temperature)`) and empty source/metric with clear error messages
- **Session preview auto-extraction** — Session list now includes preview text auto-extracted from the first user message (50-char limit), improving session sidebar display
- **User guide improvements** — Updated documentation with Skills tab references, Data page guidance, and content fixes
- **Embedded broker migrated to rmqtt** — Replaced rumqttd with rmqtt for improved stability, plugin support, and standards compliance. Broker restart uses system credentials from redb

### Fixed

- **Storage lifetime issue** — Fixed lifetime annotation in `delete_mqtt_credential` preventing compilation
- **macOS resource limits** — Fixed macOS file descriptor limits for stable operation under high connection counts
- **MQTT InvalidAuth loop** — Resolved broker authentication loop caused by credential mismatch; parallelized broker startup for faster initialization
- **MQTT broker restart credentials** — Broker restart adapter now correctly uses system credentials from redb instead of stale values
- **Backend base64 image stripping reverted** — Reverted commit 49c1086 which stripped `data:image/...;base64,` prefix from metric/telemetry API responses, breaking all image consumers (dashboard widgets and external extensions). Backend now returns string values as-is
- **Base64 image detection** — Fixed `/9j/` (JPEG) rejection in `isPureBase64`/`isBase64Image` across ImageDisplay, ImageHistory, AgentMonitorWidget, and helpers. All components now correctly detect JPEG base64 data
- **Image URL normalization** — Fixed double-prefixed data URL handling and non-standard `data:` prefix cases using magic bytes detection in normalizeImageUrl
- **Image dynamic refresh** — Device→telemetry conversion in ImageDisplay and ImageHistory now includes `refresh` interval for live image updates
- **External placeholder SSL error** — Replaced external `via.placeholder.com` with local empty state, eliminating SSL errors for missing images
- **React setState-during-render warning** — Fixed `UnifiedDataSourceConfig` calling `onChange()` inside `setSelectedItems` updater; moved to useEffect
- **Floating chat session isolation** — PanelChatView and GlobalChatFab now share session key constant; added new conversation button; fixed session history loading on mount
- **Floating chat panel redesign** — Complete overhaul of the global floating chat panel: independent session with local state (no longer shares global store with chat page), proper LLM backend loading with "not configured" empty state, skeleton loading when reopening panel, session not found auto-recovery (silently creates new session)
- **AI response tool call rendering fixed** — `ToolCallVisualization` was deprecated (returned `null`), causing tool calls and execution process to be invisible in `MergedMessageList` and `MessageItem`. Replaced with `ToolProcessBlock` to match the main chat page's rendering
- **Floating panel card-style AI responses** — Added `assistantCard` prop to `MessageItem`/`MergedMessageList` for wrapping AI responses (thinking + tool calls + content) in a subtle card background, improving readability over the glass morphism panel background
- **Streaming cursor positioning** — Fixed floating cursor in streaming content caused by `relative inline` CSS on the wrapper; now uses proper `align-text-bottom` alignment
- **Streaming-to-saved message flash fix** — Panel's `"end"` handler now uses `currentStreamMessageId` as the saved message ID, enabling smooth transition from streaming block to persisted message without visual flash
- **Session cleanup on delete** — `deleteSession` in sessionSlice now clears the panel's persisted session ID from localStorage when the deleted session matches, preventing "Session not found" errors on next panel open
- **Missing i18n translations** — Added translations for "Edit Dashboard", "Internal Broker", "Built-in" labels in en/zh locales

---

## [v0.8.0] - 2026-05-26

### Added

- **Messaging system delivery retry** — Failed message deliveries are now automatically retried up to 3 times with a 2-minute interval scheduler. The existing `DeliveryLog` infrastructure (`can_retry`/`increment_retry`/`max_retries`) is now fully wired to a background retry loop in `AppState`
- **Webhook timeout configuration** — Webhook channels now support configurable request timeout (`timeout_secs`, default 30s) with a 10s connect timeout. Field exposed in the channel creation dialog in the UI, with en/zh i18n labels
- **Message deduplication** — Messages with the same title+source+severity within a 60-second window are automatically deduplicated. The message is still stored but channel delivery is skipped, preventing message bombing from high-frequency rule triggers
- **Automatic delivery log cleanup** — A background task now runs every 6 hours to clean up delivery logs older than 1 day and messages older than 30 days. Runs on startup and periodically via `tokio::select!` alongside the retry scheduler
- **Automatic updater fixes** — Fixed app restart and version placeholder replacement after in-app updates. Fixed service config, sudo handling, and upgrade support for the install/update flow
- **Global AI chat entry (FAB)** — Floating action button on all non-chat pages opens a full-screen glass-morphism chat overlay with smooth scale-up animation. Panel uses an independent session persisted via localStorage, shares WebSocket with the main `/chat` page. Brand orange styling, Bot icon for AI messages, i18n empty state
- **5 new notification channels** — Telegram (Bot API), WeCom (robot webhook), DingTalk (custom robot with HMAC-SHA256 sign), Slack (Incoming Webhook), Feishu (custom bot with HMAC-SHA256 sign). Each channel is feature-gated in `Cargo.toml` and registered via `ChannelFactory`. All use platform-native message formats (markdown, Block Kit, HTML)
- **Channel editor FullScreenDialog** — Replaced inline `UnifiedFormDialog` with dedicated `ChannelEditorDialog` component using `FullScreenDialog` + Sidebar layout. Left sidebar shows all 7 channel types with icons and descriptions; main area shows dynamic config form. Mobile-friendly with horizontal tab bar
- **Data push module** — New `neomind-data-push` crate for pushing device telemetry and extension output to external systems. Supports Webhook and MQTT targets with event-driven and interval-based scheduling, configurable retry with exponential backoff, data filtering, and Jinja-like template rendering. Full REST API and frontend management UI with `PushTargetDialog` and `DeliveryHistoryPanel`
- **Channel type registry** — Backend now exposes channel type schemas via `GET /api/messages/channels/types/:type/schema` with per-type JSON Schema for config validation. Frontend auto-discovers available types

### Changed

- **Email SMTP connection reuse** — `EmailChannel` now builds and caches the `SmtpTransport` at creation time via `Arc<Mutex>`, eliminating per-send SMTP connection setup overhead
- **Email recipients atomicity** — `add_recipient`/`remove_recipient` now recreate the email channel before persisting to storage, with automatic rollback on failure. Previously a failed recreation could leave `state.recipients` and `EmailChannel.to_addresses` out of sync
- **Chat message styling** — AI messages use Bot icon instead of logo image. User message bubbles use neutral black/white. User avatar uses brand orange accent. Streaming text internationalized
- **Messages page refactored** — Extracted ~500 lines of channel create/edit logic from `messages.tsx` into standalone `ChannelEditorDialog` component. Main page reduced by 40%
- **Delivery log removed** — Removed monolithic `delivery_log.rs` (591 lines). Delivery tracking now handled by channel-level retry in `ChannelFilter` with simpler dedup logic

### Fixed

- **Email TLS configuration dead code** — The `use_tls` field in `EmailChannel` was stored but never read in `send()`, which always used `Tls::Required`. Now correctly uses `builder_dangerous` when `use_tls` is false, enabling support for local mail servers (MailHog, etc.)
- **CLI robustness** — Fixed widget install multipart mismatch, added border styling to widget scaffolds, aligned CLI docs/skills/prompts with actual system behavior
- **CI build** — Fixed Tauri externalBin by building `neomind-cli` alongside `neomind-extension-runner`
- **Device auto-discovery** — Fixed `adapter_type` when registering auto-discovered devices
- **Channel config field alignment** — Email config now sends `smtp_server`/`username`/`password` (was `smtp_host`/`smtp_username`/`smtp_password`). Webhook timeout field now sends `timeout_secs` (was `timeout`). All fields match backend factory expectations
- **Channel edit form initialization** — Edit mode now correctly populates form via `useEffect` watching `open`/`editingChannel` instead of relying on `onOpenChange` callback which only fires on user actions
- **DingTalk dead code** — Removed unused `webhook_url` method that caused Rust compiler warning

---

## [v0.7.9] - 2026-05-25

### Added

- **Widget development skill** — New builtin skill `widget-development.md` with complete IIFE templates (ValueCard, Clock, Gauge, DevicePanel), jsxRuntime pattern documentation, props interface guide, manifest.json reference, and Tailwind styling rules. Based on patterns from real NeoMind-Dashboard-Components repository
- **Extension development skill** — Rewritten `extension-development.md` with complete working DataProcessor template, state management patterns (AtomicU64, RwLock, Mutex), Builder API reference, Cargo.toml requirements, and `ureq` sync HTTP guidance. Based on patterns from real NeoMind-Extensions repository
- **Transform metric discovery guidance** — Enhanced `transform-management.md` with "Discover Metrics Before Writing Code" section, auto-unwrap semantics documentation, `extensions.invoke()` usage, and three discovery paths (device metrics, extension metrics, existing transforms)
- **Extension reload command** — New `neomind extension reload <ID>` command in CLI, cli-ops, and shell.rs routing. Calls `POST /api/extensions/:id/reload` for hot-restarting extension processes
- **Agent create advanced flags** — Help text now documents all flags: `--resources`, `--metrics`, `--commands`, `--event-filter`, `--timezone`, `--enable-tool-chaining`, `--max-chain-depth`, `--priority`, `--context-window-size`
- **Shell help for extension/widget/transform** — Added detailed help entries for `extension create/build`, `widget create`, and `transform create` with workflow steps, parameter tables, and examples

### Changed

- **Dashboard add-components** — Shell help and tool description now prominently recommend `add-components` over `update --components` to prevent accidental full replacement of dashboard components
- **Rule DSL quotes** — Fixed tool description to use `RULE "<name>"` (quoted) matching the actual DSL parser requirement
- **Rule engine improvements** — Enhanced DSL parsing, validation, and generator for more robust rule creation
- **CLI error recovery** — Transform test command now flattens API error responses for clearer error messages

### Fixed

- **Webhook adapter auto-discovery** — Webhook adapter now emits `DeviceDiscovered` on every POST for unregistered devices (previously only on first POST), enabling proper sample collection for auto-onboarding
- **Webhook auto-onboarding single-trigger** — `create_draft_with_topic()` now triggers analysis immediately when `MIN_SAMPLES_FOR_ANALYSIS` samples are collected (was 1 but analysis only triggered in `add_sample_to_draft`). One webhook POST now creates draft + triggers analysis
- **Webhook URL format** — Fixed all frontend webhook URL generation from `/api/devices/webhook/{id}` to correct route `/api/devices/{id}/webhook` across 6 components
- **Webhook handler refactor** — Rewrote webhook handler from 650+ lines to ~200 lines, delegating to `WebhookAdapter.process_webhook()` instead of duplicating token verification, metric extraction, and event publishing
- **Webhook shared device registry** — Webhook adapter now receives the shared `DeviceRegistry` via `set_shared_device_registry()`, fixing token verification and device type lookup
- **Webhook token display** — Fixed `config_to_device_instance()` in compat.rs to include `connection_config.extra` fields (webhook_token, json_path, etc.) so tokens display correctly in Device Connections
- **Pending Devices WebSocket auto-update** — Fixed event handler to use correct field names (`custom_type`, `data.event_type`, snake_case values). Added `Custom` event arm in `extract_event_data()` to avoid double-wrapped serialization
- **Webhook routes** — Added 3 webhook routes to router.rs: `POST /api/devices/:id/webhook`, `POST /api/devices/webhook`, `GET /api/devices/:id/webhook-url`
- **Webhook token input** — Added webhook token generation and input to both AddDeviceDialog and ManualAddForm (AddDeviceGlobalDialog)
- **Webhook URL with real IP** — Device Connections webhook URLs now show server's real IP instead of localhost
- **Device Information webhook display** — DeviceDetail page now shows webhook URL and token for webhook adapter devices
- **Extension status/logs 500→404** — Fixed API returning 500 "IPC error" for non-existent extensions. Added existence check before IPC calls, returning proper 404
- **Boolean flag parsing** — Fixed `--tls` flag silently failing when it's the last argument. Changed from `get_flag_value()` to `args.iter().any()` for boolean flags
- **Severity level mismatch** — Fixed message send recovery hint from "error" to "emergency" to match actual API accepted values (info|warning|critical|emergency)
- **Transform auto-unwrap** — Single-key JSON input like `{"value": 42}` is now auto-unwrapped to scalar `42` for simpler transform code. Multi-key objects remain as-is
- **Extension reload routing** — `neomind extension reload` no longer falls through to `__FALLTHROUGH__` but properly calls the API endpoint
- **Marketplace dialog flickering (Windows)** — `ExtensionListContent` and `DetailContent` were defined as inline components inside `MarketplaceDialog`, causing React to unmount/remount the entire DOM subtree on every render. Replaced with stable inline JSX. Also removed duplicate `fetchExtensions()` call after install
- **EntityIconPicker flickering** — `IconPreview` was defined inside the component body, moved to module level to prevent React remounting
- **UnifiedDataSourceConfig flickering** — `ItemBadge` (2 instances) and `DataIndicator` defined inside component bodies caused unnecessary remounts. Extracted to module-level components with `t` passed via props

---

## [v0.7.9] - 2026-05-23

### Added

- **CLI command system (neomind-cli-ops)** — New shared library crate with typed API client, unified output formatting, and full CLI commands for all 8 domains: device, dashboard, rule, extension, widget, transform, agent, message. Each domain supports list/get/create/update/delete plus domain-specific actions (device control, rule testing, agent invocation, extension marketplace, etc.)
- **AI Build Mode foundation** — `neomind-cli` packaged as Tauri external binary, enabling the agent to execute CLI commands via shell tool. Full CLI command reference injected into agent system prompt for discoverability
- **System CLI** — New `system info` command aggregating MQTT broker status, network info, and webhook URL. Broker management and help modules added
- **Telemetry stats API** — New endpoint for telemetry statistics with improved telemetry handling in backend
- **Dashboard rewrite (Phase 1–4)** — Complete frontend dashboard architecture overhaul:
  - Phase 1: New type system, API client, and Zustand store slices (CRUD + data source)
  - Phase 2: Query hooks, data source abstractions, real-time event bridge
  - Phase 3: Grid layout, widget shell, config panel, component registries
  - Phase 4: Widget adapters for all chart types, feature module barrel export

### Changed

- **useDataSource pipeline rewrite** — Refactored from 16 files to 4 focused sub-hooks (useTelemetrySource, useExtensionSource, useStoreSource, useSystemSource). Fixed extension event dynamic updates and data flow bugs
- **Agent CLI integration** — Unified flag names between shell.rs and CLI for consistency. Improved CLI completeness and token efficiency in agent prompts

### Fixed

- **Dashboard scroll white screen** — Multiple fixes: debounced Recharts ResponsiveContainer, staggered chart rendering with memoization, skipped unchanged device updates, removed overflow-anchor suppression
- **Dashboard multi-widget performance** — Fixed lag, blank widgets, and unresponsive mouse in dashboards with many components
- **Dashboard config preview** — Fixed live preview not reflecting config changes, removed forced grid aspect ratio causing component distortion, preserved component aspect ratio
- **Dashboard data source config** — Improved data source selector and configuration UI
- **Extension crash diagnostics** — Improved error reporting and fixed Windows DLL search path
- **CLI compatibility** — Fixed short option conflicts in device commands, added `--json` flag, fixed output printing, added API key auth support
- **Extension runner** — Bumped to 0.7.5 with improved crash protection

## [v0.7.8] - 2026-05-16

### Changed

- **Extension marketplace dialogs** — Converted extension detail and install dialogs to `FullScreenDialog` for better layout on all screen sizes
- **Transform Builder toolbar** — Redesigned Code step toolbar, removed step titles for cleaner UI
- **Data Explorer detail view** — Optimized list layouts and detail panel styling
- **Telemetry storage identifiers** — Unified all storage source IDs with `device:` prefix for consistency

### Fixed

- **Dashboard telemetry data sorting** — Fixed time-series data returning oldest points instead of newest when storage limit push-down was used. Added `query_range_rev()` for efficient descending-order queries. Applied stable sort across all telemetry transform paths to prevent JavaScript's unstable `Array.sort` from shuffling equal-timestamp points
- **Image history cross-metric interference** — Tightened `eventMetricMatches()` to prevent `foo.image` matching `bar.image` via last-segment comparison. Image data sources in the store change path now use content-only deduplication (same image content at any timestamp is treated as duplicate) instead of timestamp+value pair matching
- **Image history stale data injection** — Added time range validation to WebSocket and SSE event merge paths — events with timestamps outside the component's configured time range are now rejected. Fixed `findMetricValue` step 4 to require structurally similar key names instead of matching any image-like value
- **Store merge data misalignment** — `fetchTelemetryData` now only merges store values when API returns empty, preventing stale `current_values` from being stamped with `now` and displacing real latest data
- **Timestamp consistency** — All telemetry paths now use `Math.floor(Date.now() / 1000)` (integer seconds) instead of `Date.now() / 1000` (float). Fixed `extractTimestamp` in `ImageHistory` to correctly normalize seconds↔milliseconds
- **Extension marketplace install timeout** — Increased HTTP request timeout from 30s to 120s and extension startup timeout from 30s to 120s to allow heavy extensions (e.g. stream-player with 70+ FFmpeg dylibs) to complete installation
- **Update dialog reappearing after restart** — Prevented version update dialog from showing again after the app has been restarted following an update
- **AI chat message flicker** — Eliminated brief content flash when AI streaming completes and the final message replaces the streaming state
- **CI build warnings** — Resolved event capability test timeout and remaining build warnings

## [v0.7.7] - 2026-05-15

### Added

- **Data retention configuration** — New `GET/PUT /api/settings/retention` and `POST /api/settings/retention/cleanup` endpoints for automatic telemetry cleanup. Configurable retention period (never–90 days), image data retention, cleanup interval, and manual trigger
- **Preferences UI — Data Management** — New data management section in Settings > Preferences with auto-cleanup toggle, retention period selector, image data retention selector, and manual cleanup button
- **Extension FFI timeout protection** — Added `safe_ffi_call_with_timeout` with 30-second limit for all extension FFI calls, preventing hung extensions from blocking the runner
- **Extension event queue backpressure** — Event queue now capped at 1000 entries; oldest events dropped with warning log when queue is full

### Changed

- **Server startup parallelization** — Split initialization into Phase A (parallel store opening via `spawn_blocking`) and Phase B (background services). All redb stores (rule, agent, dashboard, instance, extension) open concurrently, reducing cold-start time
- **Concurrent extension loading** — Extension loading now uses bounded parallelism (`Semaphore(4)`) instead of sequential loading
- **Lazy GPU detection** — GPU info collected on first `/api/stats` request instead of at startup, eliminating startup delay on systems without GPU
- **Frontend cache eviction** — `useDataSource` now enforces max cache sizes with FIFO eviction for system stats, telemetry, and extension data caches
- **Extension stream lifecycle** — Added `destroy()` method for complete client cleanup; proper subscription handler cleanup on reconnect
- **Robust dashboard conversion** — `positionFromDTO` returns safe defaults for missing/malformed position data; better validation of component DTOs

### Fixed

- **Integration test redb lock conflict** — `ExtensionStore::open` now supports `:memory:` mode (isolated temp DB per call); `new_for_testing()` uses `:memory:` to eliminate parallel test file lock failures (87/87 tests passing)
- **Backend switching race condition** — `set_active` now holds a DashMap guard to prevent concurrent instance removal during active backend switch
- **Channel handler error handling** — Replaced `expect("Just created")` / `expect("Just updated")` with proper `ok_or_else` error responses in channel CRUD handlers
- **Dashboard scroll white screen** — `ChartContainer` replaced ResizeObserver + useState with pure CSS (`minHeight: 120`), eliminating the first-frame blank render. Grid items use `content-visibility: auto` with `contain-intrinsic-size: 300px` to prevent GPU texture exhaustion during fast scrolling
- **Chart component deduplication** — Extracted shared `toTelemetrySource`, `getDeviceName`, `getPropertyDisplayName`, `getSeriesName`, and `ChartTooltip` from LineChart/BarChart/PieChart into shared modules (~300 lines removed)
- **Cache implementation unified** — `useDataSource` telemetry cache migrated from raw Map + manual TTL/eviction to `TypedCache` with metadata support, unified with system stats and extension caches (~70 lines removed)

### Removed (Dead Code Cleanup)

- **Legacy `LlmBackend` trait** — Removed unused trait from `neomind-core` along with `LlmConfig`, `GenerationResult`, `StopReason`, `GenerationStream` types (0 implementations, fully replaced by `LlmRuntime`)
- **`TokenizerWrapper`** — Removed empty placeholder module (`llm_backends/tokenizer.rs`), never had a real implementation
- **`ContextRelevance::Low`** — Removed unused enum variant that was never constructed or matched
- **`StorageResult.source`** — Removed unused `'local' | 'api' | 'cache'` field from frontend persistence types (set 28 times, never read)
- **Dead functions/constants** — Removed 10 `#[allow(dead_code)]` items: `filter_simplified_tools`, `AsyncThinkStorage`, `AggressiveMockLlm`, `COMPOUND_SEPARATORS`, `MAX_TOOL_CALLS_PER_REQUEST_DEFAULT`, `DEFAULT_CONTEXT_TOKENS`, `extract_conversation_entities_topics`, `build_memory_injection_hint`, `detect_complex_intent_with_llm`, `is_complex_multi_step_intent_fallback`
- **Dead struct fields** — Removed `MessageManager.data_dir`, `MqttMapping.capabilities`, `HttpPollingTask.error_count`, `ExtensionStreamEvent::Heartbeat` variant
- **Unused example files** — Removed 5 dead examples from `crates/neomind-devices/examples/`
- **Incorrect `#[allow(dead_code)]` annotations** — Cleaned from `IsolatedExtensionLoader.native_loader` (actively used), `StreamEvent`, `CloudDeviceTypesIndex`

### Fixed

- **Integration test redb lock conflict** — `ExtensionStore::open` now supports `:memory:` mode (isolated temp DB per call); `new_for_testing()` uses `:memory:` to eliminate parallel test file lock failures (87/87 tests passing)
- **Clippy warnings** — Auto-fixed ~57 clippy issues: unnecessary `to_string`, redundant closures, `and_then→map`, `filter_map→map`, `map_or` simplification, `strip_prefix`, `is_multiple_of`, empty lines after doc comments

## [v0.7.6] - 2026-05-14

### Performance

- **WKWebView dashboard rendering** — Replaced `translate3d(0,0,0)` with `content-visibility: auto` + `isolation: isolate` + `contain: layout paint` to prevent GPU compositing layer exhaustion during loading/scrolling, eliminating white screen flash on Tauri macOS
- **Sparkline render optimization** — Extracted `SparklineContent` to top-level `memo`-wrapped component to prevent remount on each parent render; wrapped `Sparkline` export in `React.memo` to skip reconciliation when props unchanged
- **DashboardGrid render optimization** — Removed `devicesLength` from `gridComponents` useMemo dependency to prevent 3-second full rebuild on unrelated device changes
- **Limit push-down to storage** — Added `limit: Option<usize>` parameter through `query_telemetry` → `query_limited` → `query_range` chain, capping data allocation at the storage layer instead of filtering after full read
- **N+1 query elimination** — Replaced per-metric `latest()` loops with single-transaction `latest_batch` in `get_current_metrics`, reducing storage transactions linearly with metric count
- **Cold-start metrics warmup** — `list_metrics` now caches results in `metrics_info` DashMap after the first cold-start range scan, skipping full-table scans on subsequent calls
- **Debounced dashboard persistence** — `storage.sync` debounced to 500ms trailing window to coalesce rapid drag/resize events into a single API call
- **HTTP timeout layers** — Added `RequestBodyTimeoutLayer(20s)` nested inside `TimeoutLayer(30s)` to prevent slow-client DoS while preserving proper LIFO semantics
- **Code deduplication** — Extracted `createStableKey` utility from 3 duplicate implementations into shared `@/lib/stable-key.ts`

### Fixed

- **Timeout layer ordering** — Swapped `TimeoutLayer(30s)` and `RequestBodyTimeoutLayer(60s)` so the body timeout (20s) fires before the overall request timeout (30s), per Tower LIFO middleware semantics
- **Cold-start `list_metrics` returning empty** — Removed early-return guard that prevented the fallback range scan from running after server restart; added `metrics_initialized = true` after both `list_metrics` and `list_all_metrics_grouped` fallback scans
- **`moveComponent` stale closure** — Replaced separate `moveDebounceTimer` with shared `scheduleSync()` mutable-ref pattern to capture latest dashboard state during rapid drag operations
- **`handleIdChange` dashboard overwrite** — Added `activeDashboardId` guard to only update `currentDashboard`/`currentDashboardId` when the user hasn't switched away during sync
- **Sparkline const between import blocks** — Moved `SVG_OVERFLOW_VISIBLE` style constant to after all imports to satisfy linter
- **LoadingState animation** — Restored missing `animate-pulse` on loading skeleton placeholder
- **Removed unused `AlertCircle` import** from `DefaultStates.tsx`
- **Flaky test** — Added `flush()` method to `TimeSeriesStorage`/`ExtensionMetricsStorage` and call it in `test_extension_storage_write_query` to drain write buffer before asserting query results

### Chore

- **Gitignore** — Added `.worktrees/` for git worktree isolation

## [v0.7.5] - 2026-05-13

### Added

- **Unified execution engine: Focused / Focused+ / Free** — Focused mode agents can now opt into tool calling via the `enable_tool_chaining` toggle, creating a "Focused+" mode that combines pre-collected data with multi-round tool queries. The `run_tool_loop` engine is shared across Free (30 rounds, full autonomy) and Focused+ (configurable rounds, recommended tool guidance). Original Focused JSON path preserved as fallback when tool chaining is disabled
- **ToolLoopConfig** — New configuration struct driving the tool loop with mode-specific parameters: `max_rounds` (30 for Free, `max_chain_depth` for Focused+) and `recommended_tools` (prompt guidance extracted from bound resources for Focused+, unrestricted for Free)
- **Focused mode tool chaining toggle** — Agent editor shows an "Enable Tool Chaining" switch under Focused mode, persisted via `enable_tool_chaining` field. Hidden when Free mode is selected
- **Focused+ grouped resource prompt** — Focused+ system prompt groups bound resources by type (metrics with current values, commands) and provides a lightweight snapshot table instead of dumping raw pre-collected JSON. LLM is guided to use `device(action="history")` for historical queries, eliminating the need for manual `time_range` / `include_history` configuration
- **Data Collection config hidden for Focused+** — When tool chaining is enabled, the per-resource Data Collection config panel (time range, include history) is hidden since the LLM queries what it needs via tools
- **Adaptive time-series compression for device history** — `device(action="history")` now returns one of two formats, automatically picking the smallest: compact values array (`{"values": [...]}`) or adaptive series (`{"series": [{"range": "...", "kept": 12.0}, {"range": "...", "fluctuated": [12.5, ...]}]}`). Stable periods compress to single `"kept"` entries, significantly reducing token usage for the LLM
- **Mid-task context compaction** — When agent memory exceeds 70% of the context budget during long ReAct loops, old tool execution rounds are automatically summarized into a structured progress summary. Keeps recent rounds intact, preventing context overflow mid-task
- **Actual prompt overhead measurement** — Context window budget now measures real system prompt + tool definition tokens instead of using fixed percentage heuristics. Allocates `model_capacity - overhead - 1024` for history with a 20% safety floor
- **Agent summary API** — New `GET /api/agents?view=summary` endpoint returning lightweight `{id, name, status}` for dashboard dropdowns, replacing full agent payload
- **LargeDataCache eviction** — Cache now enforces max 20 entries and 50MB total. Oldest entries evicted automatically when limits are exceeded
- **Release build profile** — Added LTO thin, codegen-units=1, strip, opt-level=3 for smaller optimized binaries

### Changed

- **Time-series write buffering** — Single-point writes are now batched in an in-memory buffer (200 points, 500ms flush interval) and flushed to redb as batched transactions, significantly improving high-frequency device telemetry throughput. Flush is offloaded to `spawn_blocking` to avoid blocking the async runtime
- **Async storage I/O** — `MessageStore` operations (`insert`, `update`, `delete`, `list`) now have `*_async` wrappers that offload blocking redb I/O to `spawn_blocking`, preventing tokio runtime stalls
- **Batch delivery log writes** — Message delivery logs are collected per send cycle and written in a single lock acquisition, reducing lock contention
- **Tool response ID naming** — All aggregated tool responses now use explicit field names (`device_id`, `agent_id`, `rule_id`, `message_id`, `extension_id`) instead of generic `"id"`, improving LLM clarity
- **Token estimation consolidation** — Unified `estimate_tokens` and `estimate_message_tokens` into `tokenizer` module. Thinking content is correctly excluded from token counts (not sent to LLM)
- **Tool result compaction thresholds** — Increased keep threshold from 4KB→8KB, data-action preview from 300→2048 chars, and `CompactionConfig.max_message_length` from 8K/6K→32K/16K to preserve compact time-series format intact
- **Ollama thinking timeout guard** — Added `!skip_remaining_thinking` check to prevent repeated timeout warnings. Added 180s hard limit after timeout — terminates stream if model is stuck in thinking loop
- **ExtensionStore singleton** — `ExtensionState` now holds a shared `Arc<ExtensionStore>` instead of opening the database per call in `load_from_storage` and error handling paths
- **Error handling improvements** — `IsolatedExtension::new` uses `ok_or_else()` instead of `expect()` for child process stdin/stdout/stderr. API handlers use `From` conversion with `?` instead of `.map_err()`
- **InFlightRequests lock optimization** — Send response outside the mutex critical section, reducing lock hold time
- **Shared `ExtensionStore` in state** — `ExtensionState` constructors now accept `Arc<ExtensionStore>`, eliminating redundant `open()` calls in `load_from_storage` and auto-discovery
- **Image insight extraction** — Rewritten to use char-level operations for UTF-8 safety. Image analyses deduplicated by content fingerprint to prevent memory bloat
- **Agent panic protection** — `execute_agent` now catches panics via `catch_unwind` and converts them to Failed execution records instead of crashing the scheduler

### Fixed

- **Dashboard widget loading flash** — All 8 generic dashboard components (ValueCard, LineChart, BarChart, PieChart, Sparkline, ProgressBar, LEDIndicator, AgentMonitorWidget) now use `showLoading = loading && !hasData` pattern, preventing skeleton flash during periodic telemetry refreshes
- **DashboardGrid blank first frame** — Initial container width measurement now uses `useLayoutEffect` instead of `useEffect`, eliminating the blank frame caused by width 0 → measure → re-render
- **Dashboard DTO type safety** — Refactored `fromDashboardDTO` / `toDashboardDTO` to eliminate all `any` casts. Proper `ComponentDTO` interface, discriminated `GenericComponent`/`BusinessComponent` handling via `isGenericComponent()`
- **i18n fallback** — Removed hardcoded `lng: 'en'` default, allowing proper browser language detection. Settings tab labels now correctly use `settings:` namespace prefix
- **Agent config state injection** — Removed fragile `_agentsList`/`_visionModelsList` injection pattern in `componentConfig`. Dashboard now reads agent/model lists directly from component state
- **Extension sync consolidation** — Merged three separate extension sync effects in `App.tsx` into two cleaner effects (immediate on auth + periodic 60s timer)
- **Pending devices broker check** — Now checks both built-in MQTT broker (`connected`) and external brokers, instead of only external
- **Export dialog tree-shaking** — `xlsx` and `jszip` now loaded via dynamic `import()`, reducing initial bundle size
- **useDataSource cache leak** — Added `beforeunload` cleanup for the telemetry cache interval, preventing HMR interval accumulation in development
- **UTF-8 safe truncation** — Text truncation in agent prompts now correctly handles multi-byte characters at sentence boundaries, preventing panics on non-ASCII content
- **Agent editor state reset** — Creating a new agent now correctly resets `enableToolChaining` to prevent stale state from previous edits

## [v0.7.4] - 2026-05-11

### Added

- **Extension device management API** — Extensions can now register device type templates and device instances via new capabilities `DeviceTemplateRegister`, `DeviceRegister`, `DeviceUnregister`. Enables extensions to act as virtual device adapters
- **Extension command routing** — `DeviceService` now routes commands for extension-registered devices (adapter_type="extension") back to the owning extension via an `ExtensionCommandRouter` callback
- **Extension log viewer** — New `GET/DELETE /api/extensions/:id/logs` endpoints. Extensions capture stderr into a ring buffer (500 lines) with structured log entries (timestamp, level, message), viewable from the frontend details dialog
- **Extension crash recovery with config restore** — After crash recovery restart, the system automatically re-applies the extension's saved configuration from the extension store
- **Extension config_parameters support** — Extension runner now parses `config_parameters` from metadata JSON, enabling extensions to declare their configuration schema
- **Device metric update sets last_seen** — Reporting metrics from an extension now updates the device's `last_seen` timestamp, preventing "Never Connected" false status
- **Extension details full-screen dialog** — `ExtensionDetailsDialog` redesigned as `FullScreenDialog` with sidebar navigation: Overview, Configuration, Logs, Metrics, Commands — replacing the old tabbed modal
- **Extension SDK v0.6.3** — New `register_template()`, `register_device()`, `unregister_device()` functions for device management from extensions
- **Dashboard sharing system** — Full-featured share link management for dashboards: create links with read-only or interactive permissions, set expiration (1h–30d), copy/revoke links. Backend proxy forwards API requests via `x-internal-proxy` header for auth bypass. Shared dashboards render using the same component pipeline as the main dashboard
- **ShareManagerDialog** — New full-screen dialog for managing share links with "Add Share" dashed card pattern. Creation form in nested `UnifiedFormDialog` (z-[110])
- **Dashboard DualModeSourceField** — New dual-mode data source selector supporting both extension metrics and device metrics. Video-display component supports device-metric binding
- **Component library FullScreenDialog** — Replaced Sheet-based component library picker with `FullScreenDialog` for better space and consistency
- **Community component marketplace** — Backend API for browsing, installing, and managing community dashboard components. Manual install via file upload supported. New `FrontendComponentStore` for filesystem-based component storage
- **Marketplace browser & import UI** — `ComponentMarketplace` full-screen dialog for browsing and installing marketplace components with one-click install/uninstall. `InstallComponentDialog` for manual component import via file upload (manifest.json + bundle.js)
- **Frontend component runtime** — `CommunityRegistry`, `ComponentRenderer`, Zustand store slice for frontend components. WebSocket event system and lifecycle hooks for community components
- **Device binding for components** — Dashboard components can bind to devices via `deviceBinding` config. Bound components receive `deviceContext` (device info, current values) and `sendDeviceCommand` function. `DeviceBindingConfig` panel for selecting bound device and command parameters
- **Extension `has_device_binding` flag** — Extension components declare device binding support via `has_device_binding` in component definition

### Changed

- **Migrate to parking_lot locks** — Replaced `std::sync::RwLock`/`Mutex` with `parking_lot` equivalents across all backend crates (~80 lock `.unwrap()` calls eliminated). parking_lot locks never poison, removing a class of potential panics
- **Replace ExtensionStats API with ExtensionLogs API** — Removed `GET /api/extensions/:id/stats` and `ExtensionStatsDto`. Replaced with the new log viewer endpoints. Frontend store updated accordingly
- **ExtensionCard redesign** — Simplified from 570-line component to 148 lines by extracting details into `ExtensionDetailsDialog`
- **Fix unsafe error handling** — `shell.rs` now checks return values of `killpg` (Unix) and `TerminateProcess` (Windows) with logging on failure
- **Fix business logic unwrap()** — Replaced ~25 `unwrap()` calls in production code with `expect()`, `unwrap_or()`, or proper error propagation
- **Fix agent semaphore panic** — Tool concurrency semaphore closure now returns an error instead of panicking
- **Fix clippy -D warnings** — Resolved `is_multiple_of`, `Default` impl, `or_insert_with`, `map_or`, wildcard pattern, and `from_str` → `parse_category` naming issues
- **Fix broken test** — `test_cursor_decode_invalid_utf8` assertion corrected
- **Fix extension uninstall dialog** — Uninstall confirmation now correctly shows the extension name instead of literal `{{name}}`
- **Fix extension grid props** — Corrected `onConfigure` → `onDetails` prop name to match `ExtensionGrid` API
- **Bump version to 0.7.4** — Updated workspace, extension-runner, web, Tauri versions. Bumped extension-sdk to 0.7.0
- **Dashboard header buttons reordered** — Edit → Add Component → Share (Share moved to rightmost position). All buttons use `rounded-md` for consistent smaller border radius
- **"Add" button label** — Changed from "Add" to "Add Component" for clarity
- **Device re-registration** — `DeviceRegistry::register()` now updates existing devices in-place instead of returning `AlreadyExists` error, enabling idempotent extension re-registration
- **Fix last_seen timestamp unit** — Extension metric updates now use seconds instead of milliseconds for `last_seen`, matching device registry expectations
- **Device command dialog spacing** — Increased spacing between form fields in command control dialog for better readability
- **Dashboard sidebar alignment** — Fixed header alignment and markdown content padding in dashboard sidebar
- **Security: protected routes** — Moved sensitive APIs (LLM backends list, etc.) from public to protected routes. Removed `skipAuth` from frontend API calls that should require authentication

## [v0.7.3] - 2026-05-08

### Added

- **Relative Time Range for Tool Queries** — New `time_range` parameter for device, rule, message, and ai_metric tools. Supports human-readable strings like `"30min"`, `"1h"`, `"1d"`, `"1w"`, `"2w"` instead of Unix timestamps, solving small model timestamp calculation errors
- **Guided Error Messages** — All tool errors now include natural language guidance (e.g., entity not found → suggest list action, unknown action → show valid actions, operation failures → suggest next steps)
- **Time-Range Query Prompt** — Prompt builder now includes explicit time-range guidance to help small models correctly choose `history` action with `time_range` for time-based queries

### Changed

- **Tighter ReAct Loop Duplicate Detection** — Stop after 1 consecutive duplicate round (was 2), lower already-executed threshold to 50% (was 60%), add message_id/extension_id to signature checks
- **Stronger Inter-Round Context** — Multi-round context prompt now uses "STOP AND THINK" pattern to prevent small models from re-calling same tools with identical arguments
- **Device Tool Description** — Enhanced with stronger time-range keywords and examples to improve small model action selection accuracy

### Fixed

- **Repeated Tool Calls** — Fixed small models repeatedly calling same tool (e.g., `message(list)` 3 times in a row) by tightening loop detection and improving inter-round prompts
- **Wrong Action for Time Queries** — Fixed models using `device(list)` instead of `device(history)` when user asks about trends or time ranges

### Removed

- **Dead Code** — Removed unused `ToolOutput::error_with_data()` method
- **Chinese Hardcoding** — Replaced all hardcoded Chinese text in code with English (aliases, error messages, examples, test assertions)

---

## [v0.7.2] - 2026-05-06

### Added

- **Multi-Instance Management** — Connect to and switch between multiple NeoMind backends (local + remote) with full-screen instance manager dialog, instance selector pill in navigation bar, and animated switch overlay
- **Instance CRUD API** — REST endpoints (`/api/instances`) for creating, listing, updating, deleting, and testing remote backend instances with API key authentication
- **Instance Storage** — Persistent storage for remote instance metadata in `instances.redb` (redb-backed)
- **Unified Auth Verification** — New `GET /api/auth/verify` endpoint that accepts both JWT and API key authentication, used for pre-switch key validation
- **API Key Pre-Validation** — Instance switching validates API keys against the remote backend before switching, preventing broken states with clear error messages
- **API Key Form Validation** — Instance add/edit form validates API keys in real-time against the remote instance before saving, with visual feedback (check/error icons)
- **Remote Instance UX** — Instance manager hides management actions (add/edit/delete) when connected to a remote instance, shows contextual hint banner
- **CLI API Key Management** — `neomind api-key create/list/delete` commands for managing API keys from the command line with custom data directory support
- **Auth Data Dir Support** — `AuthState::new_with_data_dir()` for CLI tools to use custom data directories for API key storage
- **Persistent Encryption Key** — Encryption key for API key storage auto-generated and persisted to `data/encryption_key` file, survives server restarts without needing `NEOMIND_ENCRYPTION_KEY` env var
- **Encryption Key Fallback Chain** — `CryptoService` now follows priority: env var → persistent file → generate + save, ensuring API keys remain valid across restarts

### Fixed

- **Infinite API Loop on Devices Page** — TransformsBadge and DeviceTransformsDialog fetched devices, device types, and transforms on every mount, causing N×3 redundant API calls per page load. Fixed with conditional dialog rendering (`{open && <Dialog />}`) and shared `fetchCache` for transform list queries
- **Mobile Content Top Padding** — Extensions and Settings pages had inconsistent top spacing compared to other pages. Unified mobile content padding to `pt-2` in PageLayout
- **Mobile Action Button Inconsistency** — Page action buttons used different sizes (`h-8 text-xs` vs `h-9 text-sm`) on mobile. Unified all page action buttons to use standard `size="sm"` for consistent appearance
- **Extensions Page Header Layout** — Moved Extensions page action buttons into `headerContent` slot for consistent fixed positioning with other tabbed pages
- **WebSocket Infinite Reconnect Loop** — Switching to a remote instance with an invalid API key caused WebSocket to repeatedly fail auth → reload page → fail again. Fixed by separating API key errors (disconnect without reload) from JWT errors (reload to re-login)
- **WebSocket Close Code for Auth** — Server now sends close code `4001` for WebSocket auth rejections, allowing the client to distinguish auth failures from normal disconnects
- **API Key Not Clearing on Edit** — Clearing the API key field in instance edit form didn't remove the key (empty string was sent as `undefined`). Fixed: frontend sends empty string, backend treats it as `api_key = None`
- **Stale Zustand Persist Cache** — Old `currentInstanceId` from Zustand persist could override localStorage-based instance selection after page refresh. Fixed with persist version bump (v2) and migration that removes the stale field
- **Validation Icon Layout Shift** — API key validation icon (checkmark/error/spinner) caused input field width to shift. Fixed by reserving space with `pr-8` padding on the input
- **Remote Instance Shows Offline** — Instance selector always showed offline for remote instances because `isAuthenticated` only checked JWT token, not API key. Fixed `checkAuthStatus` to recognize API key as valid authentication, enabling WebSocket connections for remote instances
- **Login Page Stuck on Remote Instance** — Switching to a remote instance with API key from login page stayed on login instead of redirecting to dashboard. Login page now detects API key auth and redirects immediately
- **Stale Instance Cache After Edit** — Editing an instance (e.g. clearing API key) updated the Zustand store but not the localStorage cache (`neomind_instance_cache`), causing login page to use stale data. Fixed: all instance CRUD operations now sync to localStorage cache immediately
- **API Key Stored in Plaintext in Browser** — Backend now returns masked API keys (e.g. `nmk_abc1****`) in list/get/update responses. Full keys are held only in JavaScript memory during the add/edit session and never persisted to localStorage. Edit form shows masked key with option to clear or replace
- **Failed Switch Doesn't Revert** — Dismissing the error overlay after a failed instance switch left `currentInstanceId` pointing to the unreachable target, causing reconnection attempts on next refresh. Fixed: `clearSwitchingError` now reverts to the previous instance
- **revertSwitch Could Get Stuck** — If the instance list was empty after switching to a remote instance, reverting failed silently. Fixed: `revertSwitch` now falls back to `getCachedInstances()` when the in-memory list is empty
- **Duplicated localStorage Key Constants** — Instance-related localStorage keys were defined independently in `instanceSlice.ts` and `login.tsx`. Extracted to shared `instance-constants.ts` module

### Changed

- **Dynamic API Base URL** — Refactored `getApiBase()` to support runtime URL switching via `setApiBase()` for multi-instance support, extracted URL/key utilities to `urls.ts`
- **WebSocket/SSE/Extension Stream Auth** — All real-time connections support both JWT token and API key authentication. API key sent as query parameter for WebSocket/SSE, enabling passwordless access to remote instances
- **ProtectedRoute Accepts API Key** — Frontend route guard allows access when either JWT token or API key is present, enabling passwordless remote instance access
- **Connection Status → Instance Selector** — TopNav connection status indicator replaced with instance selector pill showing current instance name and connectivity status
- **Instance Manager Full-Screen Dialog** — Instance list opens as full-screen dialog (replacing dropdown) for better usability on mobile and desktop
- **Login Page Instance Selector** — Login page includes instance selector dropdown using cached instance list, allowing connection to remote backends before authentication
- **Setup Wizard Split** — Setup wizard pages extracted into separate files under `web/src/pages/setup/` for maintainability

---

## [v0.7.1] - 2026-05-04

### Added

- **BLE Provisioning** — Zero-touch device setup via Bluetooth Low Energy with dual transport support (Tauri native BLE via btleplug + Web Bluetooth API)
- **BLE Device Config Read** — Read device info (MAC, SN, model, netmod type) from BLE characteristic on connect for pre-filling configuration
- **BLE Netmod Support** — Adapt provisioning UI based on device network module type (WiFi / HaLow / Cat.1 cellular), hide WiFi config for Cat.1 devices
- **BLE Re-provisioning** — Update existing device info (name, broker, MQTT config) when re-provisioning via BLE; show "Configuration Updated" success message
- **BLE Device Name Sync** — Write user-specified device name to firmware storage during BLE provisioning
- **BLE Preparation Guide** — Step-by-step instructions on scan page to guide users through the provisioning flow
- **Auto Discovery Broker Guidance** — Contextual empty state in Pending Devices that guides users to add MQTT broker in Settings
- **Network Info API** — `GET /api/system/network-info` returns WiFi SSID and LAN IP for BLE provisioning

### Fixed

- **Device Type Dropdown Loading** — Add Device dialog now fetches device types on open instead of relying on stale cache
- **WebSocket Not Auto-Recovering** — Added missing `online` event listener for network recovery and reset `isManualDisconnect` flag in `connect()`
- **WebSocket Disconnected After Page Refresh** — Auth state initially false caused disconnect flag to stick, blocking reconnect
- **About Page Memory Progress Bar** — Used `bg-*` classes instead of `text-*` for progress bar fill color
- **Layout Flicker on Page Switch** — Responsive hooks (`useIsDesktop`, `useIsMobile`, `useIsTouchDevice`, `useDeviceType`) now read `window.innerWidth` synchronously on first render
- **Focus Ring on Click** — Suppressed `:focus-visible` ring on mouse clicks in Tauri/Chromium
- **BLE WiFi SSID 404** — Fixed frontend calling non-existent `/system/wifi-ssid` endpoint → use registered `/system/network-info`
- **BLE Success Screen** — Deferred `onComplete` callback to done phase close button instead of closing dialog immediately on apply
- **BLE MQTT Characteristic Optional** — Handle older firmware without MQTT characteristic gracefully
- **BLE Empty WiFi Password** — Allow empty password for open WiFi networks

### Changed

- **BLE Two-Phase Provisioning** — Split into resolve-only (get MQTT config) → BLE write → register device, preventing phantom devices on BLE failure
- **BLE Scanned Device Cards** — Display MAC address instead of model name for easier device identification
- **Pending Devices Table** — Removed column header icons for cleaner appearance
- **Add Device Dialog Icons** — Updated tab and header icons for better semantic meaning

---

## [v0.7.0] - 2026-04-28

### Added

- **API Input Validation** — All POST/PUT endpoints validate parameters before processing
- **Settings Persistence** — Settings saved to redb database, survive server restarts
- **MQTT Topic Unsubscription** — Custom MQTT topics can be unsubscribed via API
- **Empty State Guidance** — All list pages show helpful guidance when empty
- **Confirmation Dialogs** — Destructive operations require explicit confirmation
- **Form Validation** — Agent, device, and rule editors validate input with inline error messages
- **Error Boundaries** — React Error Boundaries for graceful page failure handling
- **User-Friendly Error Messages** — Toast notifications show clear messages instead of raw errors
- **AI Analyst Display Title** — Agent name in dashboard widget linked to Display Title from agent config
- **JWT-Based Rate Limiting** — Per-user rate limiting with JWT client identification
- **Backend-Ready Event** — Tauri desktop startup uses event-based ready detection instead of polling
- **Aurora Background & Glass Morphism** — App-wide aurora gradient background layer with glass-style TopNav and PageLayout footer
- **OKLCH Color System** — CSS color tokens migrated from HSL to OKLCH for perceptually uniform color scales
- **Harmonized Accent Tokens** — OKLCH-based category accent colors (purple, orange, teal, rose) with consistent light/dark variants
- **Design System Tokens** — Centralized Tailwind config tokens for borders, radius, shadows, and layout spacing
- **Frontend Design Specification** — Comprehensive `DESIGN_SPEC.md` documenting all UI patterns, tokens, and conventions
- **Plus Jakarta Sans & Noto Sans SC Fonts** — New typography with Latin and CJK support
- **UnifiedFormDialog** — Centralized dialog component handling mobile/desktop, portal, escape key, backdrop click, and z-index extraction for backdrop sync
- **Chart Color Palette Redesign** — Visually distinct, accessible chart colors with better contrast

### Changed

- **Error Handling** — Replaced 1000+ hot-path `unwrap()` calls with safe error propagation across 8 crates
- **Pagination** — Standardized default page size to 10 across all pages
- **Loading States** — All page-level loading uses skeleton screens instead of spinners
- **Notifications** — Replaced `alert()` with toast notifications throughout the UI
- **Event Trigger Cooldown** — Default changed from 5s to 60s (configurable)
- **Frontend Visual Unification** — Unified visual style and component consistency across 109 frontend files
- **Centralized API Layer** — Standardized all frontend API calls through centralized `api.ts`, eliminating scattered `fetch()` calls
- **DashMap for Device Registry** — Replaced `RwLock<HashMap>` with `DashMap` for lock-free concurrent device operations
- **Lazy Telemetry Loading** — Telemetry data fetched on demand (detail view) instead of eagerly on page load
- **Rate Limit** — Raised to 5000/min for edge device workloads; frontend retries on 429
- **Design Token Migration** — All hardcoded Tailwind palette colors (blue-500, green-600, etc.) replaced with semantic design tokens (text-success, bg-error-light, text-accent-orange, etc.) across entire frontend
- **Dialog Consolidation** — 29 form dialogs migrated from raw Radix Dialog to UnifiedFormDialog with consistent behavior
- **Chat Welcome Page** — Redesigned welcome screen with improved layout
- **Checkbox Unification** — All checkbox components consolidated to use shared `Checkbox` from `ui/checkbox`
- **Vertical Stepper Redesign** — Improved step indicator with better visual hierarchy
- **Map Component** — Device icon click no longer navigates away; shows toast notification instead
- **Shared Layout Tokens** — Extracted reusable tokens for dashboard cards, dialog headers, and section layouts

### Performance

- **API Polling Storms** — Eliminated continuous polling from data explorer (debounced events), telemetry hooks (retry limit + throttle), and extension components (conditional polling)
- **N+1 Telemetry Queries** — Replaced N+1 pattern with single table scan in data sources API
- **Message Manager Lock Contention** — Write locks released before disk I/O, reducing p99 latency from 700ms
- **Session RwLock Contention** — Session resolution clones data and drops lock before async operations
- **Agent Execution Query** — Direct lookup by ID instead of fetching 100 records + linear search
- **Device Registry Concurrency** — `DashMap` eliminates lock contention for concurrent device reads/writes
- **Agent Editor Responsiveness** — Dialog opens immediately; resources loaded in background; validation on submit only
- **Blocking Call Chain Elimination** — Removed 25 blocking patterns across 28 files (frontend and backend)
- **Batch API Requests** — Frontend batches telemetry and data source requests to reduce HTTP overhead
- **Extension Polling** — YOLO device inference extension only polls when device binding is active
- **Fetch Deduplication** — TTL-based cache (10s) in Zustand store prevents redundant API calls on page remount; WebSocket device status events use optimistic updates instead of full refetch

### Fixed

- **Rule Engine** — Catch-all error recovery prevents scheduler crashes
- **Console Cleanup** — Removed 130+ non-essential console statements from frontend
- **Extension Runner** — Improved crash loop detection and panic handling
- **Session Flicker & Tab Jumping** — Fixed race conditions in chat session switching and tab state sync
- **Focus Management** — Proper auto-focus on dialog open, search input sync, CLS (Layout Shift) prevention
- **Delete Confirmation** — Consistent border-radius and confirmation dialogs for destructive actions
- **JWT Expiration** — Client-side token expiration check prevents 401 error storms from expired tokens
- **Base64 Image Handling** — Robust cleaning with re-encoding for Ollama compatibility
- **Thinking Model Compatibility** — Disabled thinking mode in agent analyzer; made `importance` field optional in memory compression response
- **Agent Editor Input Lag** — Validation runs on submit instead of every keystroke
- **Automation Page Duplicate Loading** — Prevented duplicate resource loading on automation page navigation
- **Recharts Console Warnings** — Suppressed width/height -1 warnings from responsive charts
- **Startup Health Check** — Uses HEAD method instead of GET; increased timeout for reliability
- **Telemetry Time Range** — Frontend time range aligned with backend 30-day limit
- **User Prompt Length** — Lowered minimum from 10 to 1 character for short messages
- **Dashboard First-Load Race Condition** — Components no longer show "Failed to Load Data" on initial load; deferred data fetching waits for device list to be available before showing error state
- **Nested Dialog Z-Index** — All dashboard child dialogs (Map Editor, Layer Editor, Center Picker, AI Analyst, Agent Monitor, Command Button) now render above FullScreenDialog (z:100) using z-[110]
- **Dialog Backdrop Z-Index** — UnifiedFormDialog extracts z-index from className and applies to backdrop, fixing misaligned layering
- **Dark Mode Dialog Border** — Added visible border to UnifiedFormDialog for clear edge distinction in dark mode
- **Tailwind v3 Opacity Modifiers** — Fixed all broken CSS variable opacity modifiers (bg-primary/10 silently fails); replaced with pre-defined tokens (bg-muted-30, bg-success-light) and inline styles
- **Select Text Alignment** — Fixed text alignment in Select/Combobox components
- **Dropdown Z-Index** — Fixed dropdown menus appearing behind other UI elements
- **Nav Z-Index Conflict** — Fixed TopNav layering conflict with content below
- **Aurora Background Rendering** — Fixed CSS selector issues and glass surface rendering

### Removed

- **Swagger/OpenAPI (utoipa)** — Removed unused utoipa dependencies and auto-generated spec code

### Testing

- Added comprehensive unit tests to neomind-storage (42+ new tests)
- Added comprehensive unit tests to neomind-agent (125+ tests in tools module)
- Added comprehensive unit tests to neomind-rules (93+ new tests for DSL parser and engine)
- Added comprehensive unit tests to neomind-messages (118+ total tests)
- Added comprehensive unit tests to neomind-extension-runner (79+ new tests)
- Added comprehensive unit tests to neomind-api (24 validation tests)

---

## [v0.6.12] - 2026-04-26

### Added

- **VLM Vision Dashboard Component** — New `vlm-vision` dashboard component for real-time visual analysis using VLM (Vision Language Model) models. Streams camera/video frames to LLM backends for scene understanding, object detection, and visual Q&A directly on the dashboard.
  - `useVlmSession` hook with WebSocket streaming for low-latency frame-by-frame analysis
  - `useVlmQueue` hook with drop-intermediate-frame strategy to keep only the latest frame
  - `useVlmModels` hook for listing available LLM backends as vision models
  - `VlmMessageBubble`, `VlmTimeline`, `VlmInputBar`, `VlmConfigPanel` UI components
  - Full Zustand slice for VLM session state management
  - Registry-based component library with automatic category grouping
  - Config dialog with data source binding (device metrics, extensions, AI metrics), model selector, system prompt, and context window settings
  - i18n support (English/Chinese)

- **Event-Driven Agent Triggers for Extensions** — Agents can now be triggered by extension output events, not just device metrics. This enables agents to react to AI analysis results, external API data, and custom extension outputs.
  - Unified `DataSourceRef` model (`source_type`, `source_id`, `field`) replaces device-only `EventTriggerData`
  - `check_and_trigger_data_event()` as unified entry point for all data source types
  - `matches_data_source_filter()` supporting `Device`, `Metric`, `ExtensionMetric`, `ExtensionTool` resource types
  - ExtensionOutput feedback loop prevention with source exclusion dispatch

- **Agent Status Sync** — Agent pause/activate actions now properly sync with the scheduler (pause → unschedule, activate → reschedule), ensuring UI state matches backend execution state.

- **Extension Push-Metrics API** — New `POST /api/extensions/:id/push-metrics` endpoint for device-initiated data push that immediately stores telemetry and publishes `ExtensionOutput` events to trigger downstream agents.

### Changed

- **Dashboard Component Registry** — Replaced hardcoded `getComponentLibrary()` with registry-driven approach using `groupComponentsByCategory()`, making it easier to add new component types.
- **Tauri Updater Version Comparison** — Version check now normalizes `v` prefix and whitespace before comparison, preventing duplicate update prompts when remote JSON uses `v0.6.12` format.
- **Data Source Loading Optimization** — Added `skip_telemetry` param to `/api/data/sources` to skip expensive telemetry population for bulk listing; frontend uses server-side `source_type` filtering and parallel requests; eliminated N+1 query pattern.
- **Event-Triggered Agent Cooldown** — Changed from 5s to 60s to prevent excessive LLM calls while keeping data fresh (collection stays at 60s).
- **API Retry Policy** — Frontend now retries only gateway errors (502/503/504), not 500 application errors.
- **Unified Data Source Config** — Migrated `UnifiedDataSourceConfig` from local state to Zustand store for consistency.
- **AI Analyst Session** — Enhanced `useAnalystSession` with improved data processing, multi-source value extraction, and unmount protection for API calls. Removed `useAnalystQueue` (merged into session hook).
- **Default Image Format** — Changed default camera frame format from PNG to JPEG for better bandwidth efficiency.

### Fixed

- **Recharts Chart Rendering** — Fixed "width(-1) and height(-1)" console warnings by introducing `ChartContainer` with `ResizeObserver` and explicit pixel-sized inner container, ensuring `ResponsiveContainer` always receives valid dimensions.
- **Race Condition in Agent Execution** — Fixed `get_latest_execution` querying by ID instead of potentially stale cache. Added atomic check-and-insert for scheduler concurrency. Handled `RwLock` poison gracefully instead of panicking.
- **MQTT Lock Contention** — Fixed `last_seen` read-write lock race with `try_write`; scoped dual write lock releases to prevent contention.
- **Event Bus CPU Busy-Loop** — Added `yield_now()` in `EventBusReceiver` to prevent CPU spinning.
- **Rule Engine Deadlock** — Reduced lock scope in rule engine to prevent potential deadlock.
- **Storage Consistency** — Cache updates now happen after successful DB commit, not before. LRU cache eviction optimized from O(n) to O(1).
- **Input Size Limits** — Added limits for push-metrics (100), telemetry metrics (50), extension queries (10K), agent input (100KB), and telemetry time range (30 days max).
- **Memory Leak Prevention** — Auto-cleanup for delivery logs exceeding 1000 entries. Clean empty skill index entries on removal. Extension stream clients properly cleaned on unregister.
- **Error Handling** — Return proper HTTP 500/504 for agent execution failures. Log data collection, AI metric event, and WebSocket handler errors instead of silently dropping. Handle closed semaphore gracefully.
- **AI Analyst Data Display** — Strip "produce:" prefix from extension metric field names for correct backend key matching. Extract per-metric values instead of showing raw arrays for multi-source data.
- **Data Explorer Crash** — Guard telemetry API response to prevent crash on 502/401 when `res.data` is undefined.
- **Metric Value Parsing** — Fix fallback from 0.0 to string for non-numeric metric values.
- **Console Log Cleanup** — Removed 63+ unnecessary `console.log/info/debug` calls across frontend.
- **Dead Code Removal** — Removed `DataSourceSelector`, `DataSourceSelectorContent` components, and unused system memory extraction code from agent executor.

---

## [v0.6.11] - 2026-04-21

### Added

- **Generic Telemetry API** — New `GET /api/telemetry` endpoint for querying time-series data from any source type (devices, AI metrics, transforms, extensions) using a unified interface. Accepts `source`, `metric`, `start`, `end`, `limit`, and `aggregate` (avg/min/max/sum/count) parameters. Returns data in a consistent format with `"source_id"` key. Independent of the device-specific `/api/devices/:id/telemetry` routes.
- **Server-side Pagination for Data Sources** — `GET /api/data/sources` now supports `offset`, `limit`, `source_type`, `source`, and `search` query parameters. `populate_latest_values` runs only on the paginated subset, significantly reducing DB queries for large deployments.
- **Data Explorer Redesign** — Frontend Data Explorer rewritten with server-side pagination, filtering by source type and source name, and search. Replaced client-side filtering with API-driven filtering for better performance.
- **Extension Push Mode** — Extensions can now push data to the host via a native FFI callback (`PushOutputWriterFn`), bypassing the JSON FFI round-trip. New `send_push_output()` SDK function and `neomind_extension_register_push_writer` FFI export.
- **Extension Instance Reset** — New `neomind_extension_reset_instance()` FFI export allows the runner to re-initialize extensions without restarting the process. Extension instance storage changed from `OnceLock` to `RwLock<Option<...>>` with double-checked locking.
- **CString Memory Safety** — `json_ptr()` now tracks the last 4 allocations per thread, automatically freeing the oldest when the buffer is full. Prevents memory leaks when the host doesn't call `free_string`.
- **IPC Event Subscription** — Extension runner now supports event subscription via IPC. New `event_handler.rs` and `ipc_routing.rs` modules provide channel-based stdin message routing and event state management.
- **IPC ConfigUpdate Message** — New `ConfigUpdate` IpcMessage and `ConfigUpdated` IpcResponse support hot-reloading extension configuration.
- **Extension Health & Config Metadata** — Extensions now expose `health_status`, `last_error`, `last_error_at`, and `config_parameters` fields. Frontend types updated accordingly.

### Changed

- **`device_id` → `source_id` Telemetry Renaming** — Renamed the first-level key in the telemetry time-series storage from `device_id` to `source_id` across the entire stack. This reflects the actual usage where telemetry stores data from multiple source types (devices, AI agents, transforms, extensions), not just devices. The rename covers 5 Rust crates and 20+ frontend files.
  - **Storage Layer** (`neomind-storage`): All `TimeSeriesStore` method parameters (`write`, `query_range`, `query_latest`, `delete_range`, `list_metrics`, etc.), struct fields (`BatchWriteRequest`, `TimeSeriesResult`), and internal DashMap keys renamed.
  - **Devices Wrapper** (`neomind-devices/telemetry`): `TimeSeriesStorage` and `MetricCache` methods updated. Method renames: `list_devices()` → `list_sources()`, `get_device()` → `get_source()`, `clear_device()` → `clear_source()`, `device_count()` → `source_count()`.
  - **Core Bridge** (`neomind-core/datasource`): `DataSourceId::device_part()` → `source_part()`, `from_storage_parts(device_id, ...)` → `from_storage_parts(source_id, ...)`. All internal tests updated.
  - **API Layer** (`neomind-api`): Extension metrics handlers, data source handlers, capability providers updated. Internal variable names aligned with new terminology.
  - **Agent Layer** (`neomind-agent`): AI metrics tool uses `source_id = format!("ai:{}", group)`. Tool output JSON key changed to `"source_id"`. Data collector uses `source_part()`.
  - **Extension State** (`extension_state`): `ExtensionMetricsStorage` method parameters and `ExtensionMetricsStorageAdapter` local variables renamed.
  - **Frontend Gradual Migration**: Added `sourceId` field to `DataSource` and `MapMarker` types (with `deviceId` deprecated). Introduced `getSourceId()` helper that prefers `sourceId` with `deviceId` fallback. All 20+ dashboard and config components updated to read via `getSourceId()` and write both fields.
- **Extension SDK Unified Trait** — Removed `wasm_extension` module. The `Extension` trait is now identical across native and WASM targets, simplifying cross-platform extension development.
- **IPC InFlightRequests: Sync Mutex** — Replaced `tokio::sync::Mutex` with `std::sync::Mutex` in `InFlightRequests` so `complete()`, `cancel()`, etc. can be called from synchronous contexts (receiver thread) without `block_on`.
- **Extension State Enum Simplified** — `ExtensionStateEnum` reduced to 4 states: `Running`, `RunningIsolated`, `Stopped`, `Error`. Removed unused `Discovered`, `Loaded`, `Initialized` states and `ExtensionTypeEnum`.
- **Extension Execute Response Simplified** — `ExtensionExecuteResponse` changed from a structured interface to `Record<string, unknown>` — the raw JSON result from the extension is returned directly.
- **SDK Version Bumped** — `neomind-extension-sdk` updated to v0.6.1.

### Removed

- **HTTP_REQUEST & KV_STORAGE Capabilities** — Removed `HttpRequest` and `KvStorage` from `ExtensionCapability` enum, SDK bindings, API providers (`HttpCapabilityProvider`, `KvCapabilityProvider`), and storage layer (`ExtensionKvStore`). Extensions can make HTTP calls and manage key-value data natively.
- **PermissionDenied Error** — Removed `CapabilityError::PermissionDenied` and `required_capabilities` from `ExtensionContextConfig`. Capability access is now determined solely by provider registration.
- **Dead IPC Forwarder** — Removed `start_ipc_forwarder` thread (~150 lines) and `SyncIpcRequest`/`SyncIpcResponse` types. The stdin reader thread handles all IPC routing.

### Fixed

- **SDK Macro Compilation Error** — Fixed `expected *mut i8, found Option<_>` in `neomind_export!` macro. `Vec::remove()` returns `T`, not `Option<T>` — changed `if let Some(old) = buf.remove(0)` to `let old = buf.remove(0)`.
- **Debug Logging Cleanup** — Converted 47 `eprintln!` calls to structured `tracing` macros across extension runner (`main.rs`, `ipc_routing.rs`) and core (`process.rs`). Only the panic handler retains `eprintln!` for safety.
- **Extension Upload Dialog Animation** — Fixed Loader2 spinner jittering during upload by converting inline component function to a JSX variable, preventing React unmount/remount cycles on every progress update.
- **Extension Bundle Cache Stale Issue** — Fixed browser loading old UMD bundles after extension reinstall/update. Three fixes applied:
  - Store's `unregisterExtension` now clears `DynamicRegistry` caches and global variables.
  - Upload dialog clears extension caches before re-syncing component registry.
  - `syncComponents` detects `bundle_url`/`global_name`/`export_name` changes and clears stale module caches.
- **Loading State Improvements** — Skeleton screen patterns improved across `LoadingState` and `ResponsiveTable` components.
- **Tauri Version Mismatch** — Fixed `tauri.conf.json` showing stale version while Cargo.toml was already updated.

### Preserved (Not Changed)

- **Extension SDK Wire Protocol**: JSON parameter key `"device_id"` unchanged — avoids breaking external extensions.
- **Device Management Code**: Device register/unregister/status/config/command handlers use `device_id` semantically and correctly.
- **API URL Routes**: All existing HTTP routes (`/api/devices/:id/telemetry`, etc.) unchanged.
- **redb File Format**: Binary storage format unaffected — only variable names changed.
- **`device_type` Fields**: Retention policy fields in storage layer correctly preserved as a separate concept.

---

## [v0.6.10] - 2026-04-20

### Added

- **AI Metrics Tool** — New `ai_metric` tool enables LLM agents to create and query custom time-series metrics (anomaly scores, predictions, derived indicators). Actions: `write` (persist data point + metadata), `read` (list all metrics with latest values or query time-series for a specific metric). Metrics appear in the Data Explorer via `ai:{group}:{field}` data source IDs. Metadata persists across restarts via JSON file.
- **AI Metrics Registry** — `AiMetricsRegistry` provides shared metadata storage between `AiMetricTool` (writes) and the data sources handler (reads), with disk persistence in `data/ai_metrics_metadata.json`.
- **Dynamic Data Explorer Tabs** — Frontend Data Explorer now dynamically creates tabs for all registered data source types, including AI Metrics. Tab content auto-refreshes when new sources are discovered.
- **Unified Data Sources Collector** — `collect_ai_sources` handler collects AI metric data sources alongside device, extension, and transform sources for the unified data API.

### Changed

- **Agent Execution Mode Redesign** — Renamed Chat Mode → **Focused Mode** and React Mode → **Free Mode** with clear differentiation across all layers (backend, API, frontend, LLM tools).
  - **Focused Mode**: User binds resources (required), LLM works within defined scope using structured data tables and decision templates. Single-pass, token-efficient. Best for monitoring, alerts, data analysis.
  - **Free Mode**: LLM freely explores with all 8 tools (device, agent, rule, message, extension, transform, skill, shell), no resource binding needed. Multi-round reasoning. Best for complex automation and device control.
- **Structured Prompt for Focused Mode** — Focused Mode prompt now uses structured Markdown tables (data table + command table + decision template) instead of loose text, improving LLM reliability for command execution.
- **Scope Validation** — Focused Mode command execution validates that commands are within bound resources, rejecting out-of-scope commands with warning logs.
- **Data Collection Config UI** — Focused Mode metric resources now show configurable data collection settings (time range, include history, trend analysis, baseline comparison) in the agent editor.
- **Notification/Alert in Focused Mode** — Focused Mode can send notifications and alerts without binding, as inherent agent capabilities.
- **Focused Mode API Validation** — Create/update agent API returns 400 error if Focused Mode has no resource binding.
- **ExecutionMode Enum** — `Chat`/`React` renamed to `Focused`/`Free` with serde aliases for backward compatibility. Old values (`"chat"`, `"react"`) still accepted via deserialization.
- **Frontend Mode Cards** — Agent editor mode selection updated with new names, icons, descriptions, and "Required" badge for Focused Mode.
- **Free Mode Resource Binding Removed** — Free Mode no longer shows resource binding section. Resources cleared when switching to Free Mode.
- **LLM Tool Descriptions** — Agent tool parameter descriptions (`execution_mode`, `resources`, `enable_tool_chaining`) in both `aggregated.rs` and `simplified.rs` updated to reflect Focused/Free semantics and resource binding rules.
- **Internal Naming Unified** — `AnalysisResult` enum variants, all doc comments, tracing messages, and log strings updated from Chat/React to Focused/Free across `neomind-agent`, `neomind-storage`, and `neomind-api`.
- **Shell Tool** — New `shell` tool enables AI agents to execute system commands on the host. Features: login shell (`$SHELL -l -c`) for full user environment (PATH, aliases), cross-platform support (Unix/macOS/Windows), configurable timeout (max 600s), output truncation (10K chars), UTF-8 safe truncation, process group isolation for clean timeout kill. Parameters: `command` (required), `timeout`, `working_dir`, `description` (audit log).
- **Agent Skill System** — User-defined skill management via the `skill` tool. Actions: `search`, `list`, `get`, `create`, `update`, `delete`. Skills are YAML frontmatter + Markdown files that provide scenario-driven operation guides for the AI agent. Includes keyword matching, token budget injection, and persistence.
- **Skills Panel UI** — Frontend panel in agent settings for creating, editing, and deleting user skills with a code editor. Supports YAML frontmatter syntax highlighting.
- **Action Enum Constraints** — LLM tool definitions now include `enum` constraints on the `action` parameter for all aggregated tools, so the LLM knows exactly which actions are available (e.g., `device` supports `list|latest|history|control|write_metric`).
- **Removed Builtin Skills** — Removed 8 hardcoded builtin skills (753 lines) that duplicated tool descriptions. The skill system now focuses on user-defined multi-tool workflow skills only.
- **Enhanced Tool Descriptions** — All 6 aggregated tool descriptions (device, agent, rule, message, extension, transform) enhanced with critical workflow hints (confirm flow, list-first pattern, required fields) to compensate for removed builtin skills.
- **Login Shell for Shell Tool** — Uses `$SHELL` environment variable with `-l` flag for full user environment; falls back to `/bin/sh -c` without `-l` in minimal environments (Docker, IoT edge).
- **Adaptive Tool Timeout** — Outer tool execution timeout in `execute_with_retry_impl` now adapts to shell tool's internal timeout (`shell_timeout + 5s` buffer) instead of hardcoded 30s.
- **Tool Name Mapper** — Added `skill` and `shell` with Chinese/English aliases (命令行, 终端, bash, cli, 技能, 指南, etc.) for fuzzy tool name resolution.
- **Non-Simplified Tool Registration** — `update_tool_definitions` now registers ALL tools from the registry (not just extension tools) that aren't already in simplified definitions, fixing shell tool not being visible to the LLM.
- **Automation Simplified** — Removed complex automation modes, simplified to transform-only workflow. Unified loading states across frontend components.

### Fixed

- **Tool Result Compaction Echoing** — The old `[Called: tool(args) → result]` compaction format was being echoed verbatim by smaller LLMs instead of generating new tool calls. Replaced with natural language sentences that clearly indicate past results and instruct the model not to repeat them.
- **AI Metric Discoverability** — `ai_metric` `read_list` returned empty when metrics were written without optional `unit`/`description` fields because metadata was only registered conditionally. Now always registers metadata on write so all metrics are discoverable.
- **AI Metric Tool Description** — Improved `ai_metric` tool description with clear examples for write and read actions, making it easier for LLMs to use correctly.
- **AI Metric Metadata Persistence** — AI metrics metadata now persists to `data/ai_metrics_metadata.json` across server restarts via `AiMetricsRegistry` disk persistence.
- **Shell Timeout Parameter** — `timeout` parameter now accepts both number (`30`) and string (`"30"`) forms, fixing LLM passing string values through simplified schema.
- **Simplified Tool Description Accuracy** — Fixed `device` tool description: `get` → `latest`, added missing `write_metric` action. Fixed `message` tool: added missing `get` action.
- **Cross-Platform Shell Dependencies** — `libc` moved to Unix-only target dependency, `windows-sys` added as Windows-only dependency for proper cross-compilation.

### Added

- **Agent Execution Mode Redesign** — Renamed Chat Mode → **Focused Mode** and React Mode → **Free Mode** with clear differentiation across all layers (backend, API, frontend, LLM tools).
  - **Focused Mode**: User binds resources (required), LLM works within defined scope using structured data tables and decision templates. Single-pass, token-efficient. Best for monitoring, alerts, data analysis.
  - **Free Mode**: LLM freely explores with all 8 tools (device, agent, rule, message, extension, transform, skill, shell), no resource binding needed. Multi-round reasoning. Best for complex automation and device control.
- **Structured Prompt for Focused Mode** — Focused Mode prompt now uses structured Markdown tables (data table + command table + decision template) instead of loose text, improving LLM reliability for command execution.
- **Scope Validation** — Focused Mode command execution validates that commands are within bound resources, rejecting out-of-scope commands with warning logs.
- **Data Collection Config UI** — Focused Mode metric resources now show configurable data collection settings (time range, include history, trend analysis, baseline comparison) in the agent editor.
- **Notification/Alert in Focused Mode** — Focused Mode can send notifications and alerts without binding, as inherent agent capabilities.
- **Focused Mode API Validation** — Create/update agent API returns 400 error if Focused Mode has no resource binding.

### Changed

- **ExecutionMode Enum** — `Chat`/`React` renamed to `Focused`/`Free` with serde aliases for backward compatibility. Old values (`"chat"`, `"react"`) still accepted via deserialization.
- **Frontend Mode Cards** — Agent editor mode selection updated with new names, icons, descriptions, and "Required" badge for Focused Mode.
- **Free Mode Resource Binding Removed** — Free Mode no longer shows resource binding section. Resources cleared when switching to Free Mode.
- **LLM Tool Descriptions** — Agent tool parameter descriptions (`execution_mode`, `resources`, `enable_tool_chaining`) in both `aggregated.rs` and `simplified.rs` updated to reflect Focused/Free semantics and resource binding rules.
- **Internal Naming Unified** — `AnalysisResult` enum variants, all doc comments, tracing messages, and log strings updated from Chat/React to Focused/Free across `neomind-agent`, `neomind-storage`, and `neomind-api`.

- **Shell Tool** — New `shell` tool enables AI agents to execute system commands on the host. Features: login shell (`$SHELL -l -c`) for full user environment (PATH, aliases), cross-platform support (Unix/macOS/Windows), configurable timeout (max 600s), output truncation (10K chars), UTF-8 safe truncation, process group isolation for clean timeout kill. Parameters: `command` (required), `timeout`, `working_dir`, `description` (audit log).
- **Agent Skill System** — User-defined skill management via the `skill` tool. Actions: `search`, `list`, `get`, `create`, `update`, `delete`. Skills are YAML frontmatter + Markdown files that provide scenario-driven operation guides for the AI agent. Includes keyword matching, token budget injection, and persistence.
- **Skills Panel UI** — Frontend panel in agent settings for creating, editing, and deleting user skills with a code editor. Supports YAML frontmatter syntax highlighting.
- **Action Enum Constraints** — LLM tool definitions now include `enum` constraints on the `action` parameter for all aggregated tools, so the LLM knows exactly which actions are available (e.g., `device` supports `list|latest|history|control|write_metric`).

### Changed

- **Removed Builtin Skills** — Removed 8 hardcoded builtin skills (753 lines) that duplicated tool descriptions. The skill system now focuses on user-defined multi-tool workflow skills only.
- **Enhanced Tool Descriptions** — All 6 aggregated tool descriptions (device, agent, rule, message, extension, transform) enhanced with critical workflow hints (confirm flow, list-first pattern, required fields) to compensate for removed builtin skills.
- **Login Shell for Shell Tool** — Uses `$SHELL` environment variable with `-l` flag for full user environment; falls back to `/bin/sh -c` without `-l` in minimal environments (Docker, IoT edge).
- **Adaptive Tool Timeout** — Outer tool execution timeout in `execute_with_retry_impl` now adapts to shell tool's internal timeout (`shell_timeout + 5s` buffer) instead of hardcoded 30s.
- **Tool Name Mapper** — Added `skill` and `shell` with Chinese/English aliases (命令行, 终端, bash, cli, 技能, 指南, etc.) for fuzzy tool name resolution.
- **Non-Simplified Tool Registration** — `update_tool_definitions` now registers ALL tools from the registry (not just extension tools) that aren't already in simplified definitions, fixing shell tool not being visible to the LLM.

### Fixed

- **Shell Timeout Parameter** — `timeout` parameter now accepts both number (`30`) and string (`"30"`) forms, fixing LLM passing string values through simplified schema.
- **Simplified Tool Description Accuracy** — Fixed `device` tool description: `get` → `latest`, added missing `write_metric` action. Fixed `message` tool: added missing `get` action.
- **Cross-Platform Shell Dependencies** — `libc` moved to Unix-only target dependency, `windows-sys` added as Windows-only dependency for proper cross-compilation.

---

## [v0.6.9] - 2025-04-16

### Added

- **Transform Aggregated Tool** — New `transform` tool enables LLM agents to manage JavaScript-based data transforms through natural conversation. Actions: `list`, `get`, `create`, `update`, `delete`, `test`. Supports scope-based targeting (global, device type, specific device), extension invocation via `extensions.invoke()`, and custom output prefixes. Full multilingual support (English/Chinese).
- **TransformStore Trait Abstraction** — `TransformStore` trait in `neomind-agent` with async CRUD methods using `serde_json::Value` for cross-crate data transfer, implemented for `SharedAutomationStore` in `neomind-api`. Avoids circular dependency between crates.
- **Virtual Metrics in Device Tool** — `device(action="list")` (detailed mode) now includes `virtual_metrics` field showing metrics from Transform/extension writes not in the device template. `device(action="latest")` appends virtual metrics with latest values into the metrics array, so the LLM can see and query all available metrics.
- **Device Write Metric Action** — New `device(action="write_metric")` action allows the AI agent to write values to device metrics. Accepts `device_id`, `metric`, `value` (string/number/boolean/null), and optional `timestamp`. Enables calibration values, status flags, computed results, and any AI-generated data to be persisted on devices.
- **Dynamic Context Compaction** — Context compaction parameters (`keep_recent`, `history_share`, `message_length`) now adapt to model capacity (>16k/8k-16k/<8k). Large models get 95% effective context allocation.
- **LLM Default Context Length** — Default max context token increased from 4096/8192 to 128000 across all backends (Ollama, llama.cpp, mock), matching modern model capabilities.
- **GLM & MiniMax Model Detection** — Added context length detection for GLM (128k) and MiniMax/abab (512k) models.

### Changed

- **Keyword Planner** — Rule intent planner now distinguishes transform-related queries from rule queries, routing to the correct tool (transform vs rule) based on message keywords (convert, transform, data processing, 数据转换, 数据解析, etc.).
- **Unified Alert/Message Tools** — Alert tool merged into message tool with consistent descriptions and examples.
- **Anti-Hallucination Tool Formatting** — Tool result summaries now use structured markers (`**[ToolResult:agent]** preview...`) instead of predictable "✓ tool executed successfully" patterns, making it harder for the LLM to memorize and hallucinate responses in long conversations.

### Fixed

- **Tool Result Cache Invalidation** — Cache not invalidated on write actions (create/update/delete/control) across all tools, causing stale data on subsequent reads. Now properly invalidated after all mutations.
- **`_raw` Metric Filtering** — `_raw` and `*_raw` metrics (containing large base64 images, full MQTT payloads) now replaced with `[raw payload, {size}]` in tool output, preventing token waste in LLM context. Virtual metrics discovery also skips these noise fields.
- **Duplicate Round Content** — Last tool-call round's content was displayed twice: once in the tool round block and once as the final message. Fixed in both backend (no longer storing `final_response_content` in `round_contents_map`) and frontend (no longer saving last round content on stream end).
- **Message List Detection** — `message(list)` output was misidentified as "Conversation Log". Added message-object detection (title/level/read fields) for correct formatting.
- **User Message Preservation** — User messages now always preserved in context window (User priority >= System), preventing critical context loss during compaction.

---

## [v0.6.8] - 2025-04-15

### Added

- **Per-Round Thinking Persistence** — Backend now tracks and stores thinking content per tool-call round (`round_thinking` field on `AgentMessage`), enabling grouped rendering in the frontend with visual round labels and color-coded badges.
- **Thinking Deduplication** — Frontend detects and hides thinking content that duplicates the final response (Phase 2 LLM echo), avoiding redundant display.
- **Streaming Loading Indicator** — Consistent loading dots shown during streaming when content hasn't arrived yet, replacing the previous empty-gap behavior after tool calls or thinking blocks.

### Changed

- **LLM Pipeline Optimization** — Removed deprecated `is_likely_thinking` filter in Ollama paths (Ollama already separates content/thinking correctly); removed keyword-based thinking control overrides — thinking now respects user/instance `thinking_enabled` setting directly (`Instance setting → LlmInterface → Ollama backend`).
- **Unified LLM Defaults** — Standardized parameters across configs: temperature 0.3, top_p 0.7, top_k 40, repeat_penalty 1.05 for better tool-calling determinism.
- **Prompt Cleanup** — Removed Quick Reference table and tool description double-injection from system prompts (~284 lines of deprecated constants removed from `builder.rs`); tool definitions now handled entirely by `PromptBuilder`.
- **Unified Chat Text Sizing** — All chat message block font sizes unified to 13px (thinking content, tool call content, markdown body, round content), with labels at 11px. Previously ranged from 10px–14px across different blocks.
- **Softer Block Styling** — Thinking and tool-call blocks now use borderless rounded backgrounds (`bg-muted/30`) instead of hard borders, for a cleaner visual appearance.
- **Tool Call Block Spacing** — Tool call block uses `mb-4` bottom margin to create clear separation from the final response content below.

### Fixed

- **Multi-Round Thinking Display** — Thinking content now accumulates across all tool-call rounds instead of resetting on each round transition, so all rounds' thinking is visible during streaming.
- **Duplicate Loading Indicators** — Removed legacy standalone loading dots that conflicted with the new inline loading, preventing double indicators on empty streaming messages.
- **Rule Builder Extension Support** — Fixed validation in rule creation that blocked "Next" when selecting an extension as data source (only checked `device_id`, ignored `extension_id`). Fixed trigger building for extension conditions (was always empty `device_id`). Fixed `RuleAction::Set` on backend not routing to extension executor — Set actions targeting extensions now correctly execute via `ExtensionActionExecutor`.
- **Model Selector Overflow** — Added `max-h-[50vh] overflow-y-auto` to LLM model dropdown to prevent long model lists from overflowing the viewport.
- **Embedded Tool Call JSON in Display** — Small models (e.g. 4B) often output tool call JSON (`[{"name":"device",...}]`) as plain text mixed with markdown code blocks. Three-layer fix:
  - **Backend hold-back**: Streaming buffer now also detects `{"`, `{"name"`, and ```json``` patterns — not just `[` — to prevent partial JSON fragments from being yielded to the frontend.
  - **Backend storage cleaning**: `remove_tool_calls_from_response` applied at all 4 message storage points (main tool path, multimodal path, no-tool paths) and enhanced with ```json code block regex cleaning. `content_before_tools` is also cleaned before storing as round content.
  - **Frontend display cleaning**: `cleanToolCallJson()` applied to both `round_contents` and message content during rendering, covering streaming and persisted messages.

### Changed

- **Dead Chinese Prompt Code Removed** — Removed 481 lines of unused Chinese prompt constants (`*_ZH`) and associated methods from `builder.rs`. The `LANGUAGE_POLICY` header already instructs models to respond in the user's language, making separate Chinese prompts unnecessary. Only `CONVERSATION_CONTEXT_ZH` retained (still used by agent executor memory system).

---

## [v0.6.7] - 2025-04-14

### Added

- **Ollama Capabilities-Based Vision Detection** — Vision detection now prioritizes the Ollama API `capabilities` array (authoritative source) over `model_info` heuristic, with fallback for older Ollama versions.
- **qwen3.5 Multimodal Support** — Full qwen3.5 series (including `qwen3.5:4b` local models) now correctly detected as multimodal across all detection paths.
- **Agent Thinking Panel Collapsible** — Agent thinking panel now supports collapse/expand with a preview line, reducing visual clutter during execution monitoring.
- **Tauri Keyboard Fix** — Prevent Backspace/Delete from triggering browser back navigation in Tauri WebView.

### Changed

- **Agent Card Layout** — Simplified footer layout; executing status shown inline with spinner instead of separate thinking block.
- **Agent Detail Panel** — Executions are preloaded on agent selection instead of waiting for history tab; auto-reload on execution completion.
- **Unified Vision Detection** — All backend vision detection now uses `neomind-core`'s `detect_vision_capability()` for consistency.
- **Capability Upgrade Logic** — Backend capability detection only upgrades (false→true), never downgrades API-detected values that are already persisted.

### Fixed

- **Dashboard LineChart Stale Data** — Removed React.memo from LineChart component that prevented data updates.
- **DevicesPage Performance** — Grouped selectors with `shallow` equality to reduce unnecessary re-renders.
- **Telemetry Query Concurrency** — Added semaphore to limit concurrent telemetry queries to 16, preventing resource exhaustion.
- **Storage Performance** — Single DB query for device state instead of double lookup; paginated scan avoids loading all results; range query replaces full table scan.
- **UTF-8 Key Safety** — Safe `increment_prefix` for UTF-8 keys in storage, with semaphore error logging.

---

## [v0.6.6] - 2025-04-14

### Added

- **Token Usage Reporting & Context Summarization** — Agent streaming now reports token usage per turn. Sessions auto-summarize when context exceeds model limits, preserving conversation continuity across long sessions.
- **Context Summarization API** — New `POST /api/sessions/:id/summarize` endpoint for manual context compression.

### Changed

- **Agent Toolkit Consolidation** — Merged and simplified tool definitions, removed unused system tools (DSL, MDL, rule-gen) for cleaner agent context and faster tool resolution (~3400 lines removed).
- **Streaming Refactor** — Agent streaming handler restructured for better error recovery and token tracking.

### Fixed

- **Memory Compression Safety** — Compression now preserves high-importance entries instead of sending all entries to LLM. Only entries exceeding category limits are compressed, and the top half is always kept intact.
- **Over-Aggressive Merge Protection** — New safety threshold blocks compression when LLM returns fewer than 20% of the entries it was given, preventing catastrophic memory loss from small models over-merging.
- **Extract/Compress Decoupling** — `POST /api/memory/extract` no longer auto-triggers compression on all categories. Compression runs only via the scheduler or manual `POST /api/memory/compress` trigger.
- **Default Context Length** — Use 8192 as default `max_context` instead of 0, preventing context overflow on backends that don't report model limits.
- **Ollama Model Context Detection** — Correct context size detection for ministral and other models that report context length differently in the Ollama API.
- **Tauri Updater CI** — Fixed artifact paths and auto-generation of `latest-update.json` in GitHub Actions workflow.

---

## [v0.6.5] - 2025-04-13

### Added

- **Token-Based Context Management** — Conversation history managed using token counting instead of message count, with automatic context overflow retry for resilience across LLM backends.
- **Dashboard Grid Rewrite** — Ref-based `react-grid-layout` integration eliminates feedback loops between layout state and re-renders, fixing jitter and positioning bugs.
- **Config Data Refresh** — Component data updates immediately when editing data binding in config dialog, with `configVersion` tracking for live re-renders.
- **Chart Responsive Resize** — Chart components (LineChart, BarChart, PieChart, AreaChart) properly fill their container via flex-based layout.
- **New Component Default Size** — Dashboard components appear at correct default sizes instead of 1×1 minimum.
- **Aggregated Tool Enhancements** — Added `latest_execution` and `send_message` tool actions for agent execution monitoring and control.
- **Agent Execution Timeline** — Refactored timeline with tool thinking event support and improved event rendering.
- **React/Chat Dual-Path Execution** — Agents support both React reasoning loop and direct chat execution paths with background API.
- **Concise React Prompts** — Optimized agent React prompts and UTF-8 truncation safety.
- **Execution Detail Layout** — Improved execution detail dialog layout.

### Fixed

- **Streaming Tool Calls** — Fixed tool call streaming event handling in chat interface.
- **Sidebar Scroll** — Fixed sidebar scroll behavior and chat layout issues.
- **Scheduler Panic** — Fixed agent scheduler panic on concurrent access.
- **Thinking Model Compatibility** — Memory extraction and compression LLM calls now disable thinking (`thinking_enabled: Some(false)`), preventing token waste on reasoning models (qwen3.x, deepseek-r1).
- **Memory Config Alignment** — Backend `ExtractionConfig` now matches frontend Config UI fields.
- **Memory Extraction Returns Zero** — Fixed extraction returning 0 entries when using thinking-capable models.
- **llama.cpp Multimodal Detection** — Auto-detect vision, tool calling, and context size from `/props` endpoint.

# NeoMind — Edge AI Platform for IoT

Rust (Axum) backend + React 18 + Tauri 2.x desktop. Runs LLM agents on local hardware, connects devices via MQTT/BLE/Webhook, automates via a rule engine, visualizes on real-time dashboards.

The spine is an async **EventBus** (`neomind_core::eventbus`) — all subsystems (devices, agents, rules, dashboards, frontend WS/SSE) communicate via pub/sub events, not direct calls.

## Development Commands

```bash
cargo build && cargo test && cargo run -p neomind-cli -- serve   # API on :9375, swagger at /api/docs
cd web && npm install && npm run dev                              # frontend :5173
cd web && npm run tauri:dev                                       # desktop app
```

## Ecosystem Repositories

NeoMind is the **core platform**; three companion repos hold community content. This repo defines the **format contracts** each must follow.

| Repo | Contains | Loaded by this repo as |
|------|----------|------------------------|
| **[NeoMind](https://github.com/camthink-ai/NeoMind)** (this) | Backend + frontend + desktop app | — |
| **[NeoMind-Extensions](https://github.com/camthink-ai/NeoMind-Extensions)** | Official extension marketplace (weather, YOLO, OCR, face, HA/LoRaWAN/Modbus bridges, stream player…) | `.nep` packages → `data/extensions/<id>/` |
| **[NeoMind-DeviceTypes](https://github.com/camthink-ai/NeoMind-DeviceTypes)** | Standardized device type JSON templates (e.g. NE301, NE101 cameras) | JSON imported into the device registry |
| **[NeoMind-Dashboard-Components](https://github.com/camthink-ai/NeoMind-Dashboard-Components)** | Community dashboard widget marketplace (charts, gauges, maps…) | Bundles → `data/frontend-components/<id>/` |

### Extension package contract (`.nep`)
ZIP archive, parsed by `crates/neomind-core/src/extension/package.rs`. Install: `POST /api/extensions/install` or `neomind extension install file.nep`.

```
{extension-id}-{version}.nep
├── manifest.json                # format = "neomind-extension-package"
├── binaries/{darwin_aarch64,darwin_x86_64,linux_amd64,windows_amd64,wasm}/...
└── frontend/dist/{bundle.js, bundle.css}   # optional dashboard components
```

- **ABI version**: `CURRENT_ABI_VERSION = 3`, `MIN_ABI_VERSION = 3`. Bumping breaks every published extension.
- **SDK**: `crates/neomind-extension-sdk/` is public API — never auto-strip its re-exports. Authors use `neomind_export!()` + `ExtensionMetadata::new()`.
- **`frontend` field is polymorphic**: string (`"frontend/"`, legacy) OR struct (`{ components: [...] }`). `FrontendField` handles both — don't break either.
- **Extensions can bundle dashboard components** via `FrontendConfig.components: Vec<DashboardComponentDef>` — distinct from community widgets (next section).
- **Runtime**: process-isolated child process via `neomind-extension-runner`. Crash-loop detection auto-disables misbehaving extensions. Metrics → DataSources (`extension:<id>:<metric>`).
- **Hardware variant keys (CUDA/Jetson)**: variant discrimination lives **only** in marketplace `metadata.json` `builds` keys and release filenames — the `.nep` internal `manifest.binaries` key is always the plain OS+arch (e.g. `linux_arm64`), identical for CPU and Jetson builds. Download-side selection (`accel.rs::detect_variant` + `select_build_key`) resolves `linux-aarch64-jetson` → `linux-aarch64` → `wasm`. Detection: `/etc/nv_tegra_release` (Jetson, checked first) > `nvidia-smi` (CUDA) > CPU. Override with `NEOMIND_EXTENSION_VARIANT=cpu|cuda|jetson`. Adding a jetson build = upload `xxx-linux_arm64-jetson.nep` (standard internal structure) + add `linux-aarch64-jetson` entry to `metadata.json` `builds`. Do NOT add variant suffixes inside `.nep` `binaries/`.

### Device type template contract (JSON)
Maps to `DeviceTypeTemplate` (`crates/neomind-devices/src/registry.rs:159`). Required: `device_type` (unique id), `name`. Optional: `categories`, `mode` (`Simple` default), `metrics`, `commands`, `uplink_samples`, `default_offline_timeout_secs`.

- **`default_offline_timeout_secs`** = fallback for devices registered through this template; device-level override still wins (gotcha #4).
- **`mode: Simple`** = metrics/commands listed directly, no uplink/downlink wrapper. Historical separation was removed — don't reintroduce.
- **No code changes needed to add a device type** — a JSON file is enough. Persists into `devices.redb`.

### Community dashboard widget contract
Standalone frontend bundles (NOT inside a `.nep`) → `data/frontend-components/<id>/`. Each ships a manifest + JS bundle exposing a React component on a named global.

- **Distinct from extension-bundled components** (which live in a `.nep`'s `frontend/` dir).
- **Do NOT reject builtIn type names in `DynamicRegistry`/`CommunityRegistry`** — widgets/extensions may legitimately use similar type names. Importing `builtInTypes` from `Renderers` into those registries is forbidden.
- Routes: `/api/frontend-components/*` (install/list/load).

### Cross-repo working notes
- **ABI/schema bumps are breaking**: changing `.nep` ABI version, device type schema, or widget manifest schema breaks every published item. Bump major version and coordinate.
- **Canonical home**: new official extensions / device types / widgets go in the marketplace repo, not this one. This repo only ships loaders + the SDK.

## ⚠️ Critical Gotchas (silent bugs if violated)

Read these first — each one has caused real bugs:

1. **Dashboard DTO conversion**: Backend = snake_case (`data_source`), Frontend = camelCase (`dataSource`). Every dashboard API response MUST pass through `fromDashboardDTO()` in `web/src/store/persistence/types.ts`. New code loading dashboards from API that skips this breaks silently.
2. **`web/src-tauri/` is NOT in the cargo workspace**. It imports `edge_api::start_server()` (alias for `neomind_api`). `cargo build` at workspace root will NOT catch breakage there — when refactoring `neomind-api` public re-exports, also grep `web/src-tauri/src/`.
3. **Multimodal capability priority**: `user override > runtime API detection (/api/show) > LiteLLM registry > heuristic > false`. Never let runtime detection clobber a user override. Single entry point: `supports_multimodal()` in `crates/neomind-agent/src/llm.rs`.
4. **Per-device offline timeout**: heartbeat monitor (`start_heartbeat_monitor`) and `is_device_stale()` MUST use `DeviceService::effective_offline_timeout(device_id)` (priority: device > template > global), NOT the global `HeartbeatConfig::offline_timeout`. Using global silently ignores per-device overrides.
5. **`update_device_handler` uses direct assignment** `req.offline_timeout_secs` (NOT `.or(existing)`). This is intentional so JSON `null` clears the override. Don't "fix" it to `.or()`.
6. **Ollama**: use `/api/chat` (native), NOT `/v1/chat/completions`.
7. **Thinking models** (qwen3.x, deepseek-r1): set `thinking_enabled: Some(false)` for non-chat LLM calls (memory extraction, compression) — otherwise wasted tokens.
8. **Agent memory journal truncation** must match write↔read: outcome 300 chars; `action_taken` 150 chars/action × max 5 joined; read outcome 300 + action_taken 800 chars (write caps 300/~758, so read never truncates). Mismatch loses Chinese text silently.
9. **`web/src-tauri/` API base** is `http://localhost:9375/api` (already includes `/api`); WebSocket uses `ws://`. In frontend, `getApiBase()` already includes `/api` → call `/settings/retention`, NOT `/api/settings/retention`.
10. **Error-path journal writes**: failed agent executions MUST write a `success: false` journal entry in the outer `Err(e)` branch — otherwise the agent can't learn from failures across runs.
11. **Extension ComponentRenderer**: do NOT add `mountedRef` patterns or wrap with ErrorBoundary in `renderDashboardComponent` — React 18 StrictMode double-mount + async loading breaks. Do NOT reject builtIn type names in DynamicRegistry (extensions may use similar names).
12. **Event trigger dedup_key cleanup**: capture `dedup_key_clone` BEFORE `recent.insert(dedup_key, now)` moves the key. On persistent failure, clear the key so transient API errors don't lock out the 60s cooldown window.

## Canonical Source-of-Truth (read before modifying)

When changing X, the canonical location to read first:

| Modifying | Read first |
|-----------|-----------|
| Event bus types | `crates/neomind-core/src/event.rs` (`EventType` enum) |
| HTTP routes | `crates/neomind-api/src/server/router.rs` |
| App state / store wiring | `crates/neomind-api/src/server/types.rs` |
| Agent enums (`ScheduleType`/`ExecutionMode`/`AgentStatus`) | `crates/neomind-storage/src/agents.rs` |
| LLM backend capabilities | `crates/neomind-core/src/llm/capability.rs` + `registry.rs` |
| Multimodal image parsing | `crates/neomind-agent/src/image_utils.rs` (single source of truth) |
| Dashboard DTO conversion | `web/src/store/persistence/types.ts` |
| Frontend design system | [`web/DESIGN_SPEC.md`](web/DESIGN_SPEC.md) — **MUST read before any UI work** |
| Extension SDK public surface | `crates/neomind-extension-sdk/` (public API — never auto-strip re-exports) |

## Domain Essentials (non-obvious behavior)

### Device — 4-state connection model
Transport connection (MQTT client online) is tracked **independently** from data activity (last telemetry). `rmqtt` `DevicePresenceHook` fires `DeviceTransportOnline/Offline` on `ClientConnected/ClientDisconnected` regardless of data flow.

| UI state | `transport_connected` | recent data within `offline_timeout` |
|----------|----------------------|--------------------------------------|
| `online` | ✓ | ✓ |
| `connectedIdle` | ✓ | ✗ |
| `offline` | ✗ (stale) | ✗ |
| `disconnected` | ✗ | — |

Frontend: `getDeviceState()` in `web/src/lib/utils/deviceStatus.ts`, renders via `DeviceStatusBadge`. Falls back to legacy 3-state when `transport_connected` is undefined (older backend).

### Agent
- **ScheduleType**: `Event | Cron | Interval | Once` (Once = one-shot/manual-only: never auto-scheduled, manual invoke works, transitions to `AgentStatus::Completed` after success). **ExecutionMode**: `Focused` (bound resources, single-pass) | `Free` (LLM-driven multi-round tool calling, default schedule is `free` when no resources bound)
- **Status**: `Active | Executing | Paused | Error | Stopped | Completed` (Completed = one-shot finished). After server restart, `reload_active_agents()` loads only `Active` — `Error` agents stay dropped until manually reactivated.
- **Event agents**: trigger when bound resources match `event_filter`. Free-mode event agents **without a filter never fire**.
- **Two memory systems by design**: scheduled = `AgentMemory` (journal/knowledge_files/user_messages); chat = `MemorySnapshot` (user.md/knowledge.md) + conversation history.
- **Concurrency**: global semaphore (10), per-LLM-backend (2), tool_concurrency (6). `running_executions` HashSet prevents scheduler duplicate spawns. Global execution timeout: 5 min via `tokio::time::timeout` wrapping `execute_internal`.
- **RAII `StatusGuard`** resets status to `Active` on panic/drop/timeout — never leaves agent stuck in `Executing`.

### Rule (v2 — pure JSON)
No DSL parser. `POST /rules` body: `{name, condition, actions, trigger, cooldown, for_duration}`. `condition` is recursive (`Comparison | Range | Logical`); `actions` is `Notify | Execute | TriggerAgent`. `dsl_preview` is auto-generated read-only text. `notify` creates a Message → routed to all channels whose `ChannelFilter` accepts it (empty filter = accept all).

### DataSourceId
Format `{type}:{id}:{field}` — examples: `device:temp-sensor-001:temperature`, `extension:weather:temp`, `transform:avg_temp:value`. Transform output uses **dots** for dashboard binding (`extensionMetric: "<prefix>.<field>"`) but **colons** in DataSourceId — both correct for their context.

### LLM Backend Capabilities
Resolution chain: `user_override > runtime_api (Ollama /api/show) > registry (LiteLLM, 2988 entries) > heuristic > false`. Each `BackendCapabilities` tracks `multimodal_source` (`user_override | runtime_api | registry | heuristic`). `ensure_instance_capabilities` skips re-detection only for `user_override` and `runtime_api` sources — never let runtime clobber a user override.

### CLI In-Process Dispatch
The LLM's `shell` tool intercepts `neomind ...` commands and dispatches them **in-process** via `neomind_cli_ops::dispatch::dispatch(argv)` — no subprocess. `dispatch()` uses `try_parse_from` (bad args → `Parse` error, not `exit()`). Falls back to subprocess only for `Serve | Prompt | Chat | Logs | Health`. Failed commands return a `CliResponse` with `suggestion: Option<String>` recovery hints.

## Storage Layout

Each domain has its own redb file under `data/`:

| File | Domain |
|------|--------|
| `telemetry.redb` | Time-series — all metrics |
| `devices.redb` | Device registry, MQTT credentials, type templates |
| `agents.redb` | Agent defs, executions, memory |
| `rules.redb` / `rule_history.redb` | Rule defs + trigger history |
| `automations.redb` | Transforms, data-push bindings |
| `dashboards.redb` | Dashboard defs |
| `messages.redb` / `channels.redb` | Messages, alerts, notification channels |
| `sessions.redb` | Chat sessions |
| `llm_backends.redb` | LLM backends + active backend |
| `instances.redb` | Multi-instance manager |
| `extensions.redb` | Installed extensions |
| `settings.redb` / `users.redb` / `api_keys.redb` | Settings, users, API keys |
| `data-push.redb` | Push configs + logs |

Runtime subdirs: `extensions/` (installed packages), `frontend-components/` (community widgets), `memory/`, `skills/`, `logs/`.

## Key Data Flows

**Telemetry → dashboard**: device → MQTT → `neomind-devices` parses → writes `telemetry.redb` → publishes `DeviceMetric` on EventBus → WS/SSE → frontend `deviceSlice` → widget re-renders.

**Scheduled agent execution**: scheduler tick (1s) finds due agents → `running_executions` guard → executor collects fresh data + prefetches knowledge files → tool-calling loop (max 30 rounds, 128KB/result cap, base64 stripped >4KB) → executes actions → `update_memory` writes journal entry (success or failure). Event triggers deduped via 60s cooldown per `(agent_id, source_type, source_id)`.

**Rule trigger**: telemetry event or scheduled check → evaluate `condition` against current state → on match (respecting `cooldown`/`for_duration`) execute `actions` → `notify` → Message → matching channels → retry/backoff delivery → `MessageReceived` on bus.

## Frontend Hard Rules (the breaking ones)

Full spec in [`web/DESIGN_SPEC.md`](web/DESIGN_SPEC.md). Non-negotiable rules:

- **Colors**: only design tokens (`text-success`, `bg-error-light`). NEVER raw Tailwind palette (`bg-blue-500`). Text on colored bg = `text-primary-foreground`.
- **No `/` opacity on CSS-variable colors** (`bg-primary/10` silently fails). Use predefined tokens (`bg-muted-30`) or inline style.
- **Icons**: `lucide-react` only, mapped via `@/design-system/icons`. NEVER emoji.
- **Page loading**: skeleton screens (`LoadingState variant="page"`). `Loader2` spinner only for inline/button/dialog.
- **Dialogs**: `UnifiedFormDialog` (forms), `FullScreenDialog` (builders). Nested dialog inside FullScreenDialog MUST use `className="z-[110]"`. Never raw `Dialog`.
- **Layout**: `PageLayout` + `PageTabsBar`/`PageTabsContent`. Pagination default **10**; mobile infinite scroll via `hideOnMobile`.
- **Form inputs**: only `@/components/ui/` + `Field` component (auto a11y). No raw HTML form elements.
- **Fetch dedup**: store-level `fetchCache` (TTL 10s). Pattern: `shouldFetch` → `markFetching` → API → `markFetched`. Invalidate on mutations.
- **Notifications outside React**: `notifySuccess`/`notifyError` from `@/lib/notify`. Inside React: `useToast`.
- **Z-index**: popovers `z-[200]`, AlertDialog `z-[200]` (always top), full-screen dialogs `z-[100]`/`z-[110]`, overlays `z-50`.
- **i18n**: all visible text via `t()`. Never hardcode strings.
- **Portals**: all modals/popovers via `getPortalRoot()` from `@/lib/portal`.

## Code Hygiene Patterns

- **Dead re-export detection**: grep has a blind spot for multi-line brace imports (`neomind_crate::{\n TypeA,\n TypeB}`). Compiler-based is reliable: (1) strip all re-exports to empty, (2) `cargo build --tests 2>&1 | grep "unresolved import"`, (3) add back exactly what the compiler reports. Skip this for `neomind-extension-sdk` (public API).
- **Dead `Default` impl**: check 4 patterns — `X::default()`, `unwrap_or_default()` on `Option<X>`, `HashMap::entry().or_default()` with X value, `..Default::default()` in struct literals.
- **Subagent unreliability for dead code**: ~70% false-positive rate. Always verify candidates with manual greps before removing; prefer batch-remove + `cargo build` to catch errors.
- Rust: `cargo fmt` + `cargo clippy`. Frontend: run type checks after every change.

## In-Tree References

- [`web/DESIGN_SPEC.md`](web/DESIGN_SPEC.md) — frontend design system (33 sections), read before UI work
- [`CHANGELOG.md`](CHANGELOG.md) — version history

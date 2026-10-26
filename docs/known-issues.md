# Known Issues (internal)

Platform issues found by the eval harness (business-chain E2E, value-level
gate assertions, case authoring probes). Each entry: symptom, repro, impact,
suggested fix. Update status inline when fixed.

## 1. Internal MQTT adapter subscribes ~16s after /health — early telemetry silently dropped

- **Status:** open (2026-10-26, 1.2.3)
- **Found by:** `eval/e2e_business.py` (step "internal MQTT adapter subscribed")
- **Symptom:** embedded broker accepts connections ~300ms after /health 200,
  but `internal-mqtt` (created in the post-health background task,
  `server/mod.rs` → `bg_state.init_device_adapters`) connects ~16s later.
  Publishes in that window have no subscriber and vanish — no error, no queue.
- **Repro:** spawn `neomind serve`, wait for /health 200, publish
  `{"temperature":35.5}` to `device/<type>/<id>/uplink` within ~15s, then
  `GET /devices/<id>/telemetry` → empty.
- **Impact:** devices reporting right after server start lose first readings;
  tests publishing immediately after health flake.
- **Suggested fix:** move adapter init before health goes green, or buffer
  early broker messages until the adapter subscribes. Workaround used by the
  eval harness: poll `/mqtt/status` for `connected && subscriptions_count>0`.

## 2. Dashboard add-components: `position` mandatory with no default — bare component JSON 400s

- **Status:** open (2026-10-26, 1.2.3)
- **Found by:** gate case `dashboard-components` value-level assertion
  (`dashboard_component_count` stays 0 while command family matched).
- **Symptom:** `DashboardComponent.position`
  (`handlers/dashboards.rs`) is required; hand-written component JSON (models
  included, frontier-grade) omits it and gets 400
  `missing field position`. The CLI help's own example omits position too, so
  the documented shape fails.
- **Impact:** high failure rate for LLM agents and API-first users composing
  dashboards; failures invisible to command-family assertions.
- **Suggested fix:** default `position` server-side (append to next free grid
  slot) or add `--auto-place` to the CLI and fix the help example.

## 3. CLI `device get <id>` 404 prints "is the server running?" hint

- **Status:** open (2026-10-26, 1.2.3)
- **Found by:** case authoring probes.
- **Symptom:** `neomind device get <nonexistent>` exits 1 with
  `API error (404): Device not found / Hint: is the server running? Try:
  neomind health`. The server is fine; the id is wrong. The generic-404 hint
  misleads users and agents (wasted turns re-checking health instead of
  concluding the id is bad).
- **Suggested fix:** attach the connectivity hint only on connection-level
  errors; for 404 print `no device with id X (see: neomind device list)`.

## 4. Webhook ingestion 503 "still starting up" race (milder sibling of #1)

- **Status:** open (2026-10-26, 1.2.3)
- **Found by:** `eval/e2e_business.py` webhook steps.
- **Symptom:** `POST /api/devices/:id/webhook` returns 503
  `Device ingestion is still starting up` for the first seconds after spawn.
  Better than #1 (loud, retryable) but still a startup race.
- **Suggested fix:** same family as #1 — gate health on ingestion readiness,
  or document the warm-up in the onboarding wizard.

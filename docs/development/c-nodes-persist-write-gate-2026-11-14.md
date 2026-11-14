# §12 C — contentNodes persist + may_execute_write dispatch (2026-11-14)

## Scope
- Production chip composer → `contentNodes` on WS send (Chat + Panel).
- Persist `contentNodes` / `contentVersion` on `SessionMessage` ↔ `AgentMessage`.
- Wire G2 `may_execute_write` / `record_successful_write` at mutating tool dispatch (stream_core + multimodal).

## Not in this slice
- TipTap true in-sentence editor (still chip + trailing reference nodes).
- History UI re-render of chips from nodes (field is round-tripped; display still plain text).
- Browser/touch/5-person UX; live Spark pairing (138 holds `:29431`).

## Mechanism
1. Client: `chipsAndTextToContentNodes` + `ws.sendMessage(..., references, contentNodes)`.
2. API: after `prepare_chat_input`, `attach_pending_content_nodes` → agent pending → next user `push_message` consumes once. Legacy-only chips synthesize text+ref nodes.
3. Storage: camelCase wire keys `contentNodes` / `contentVersion`; JSON schema-compatible (old rows omit → None).
4. Write gate: `mutating_write_fingerprint(tool, args)` for shell mutations + `run_now`/`file_write`/`file_edit`/`image_edit`; block when cancelled or fingerprint already recorded; record on effective success.

## Gates (offline)
- `neomind-storage` `session_message_content_nodes_round_trip_camel_case` PASS
- `neomind-api` `nodes_for_persistence_*` PASS
- `neomind-agent` `task_continuity::tests` 10/10; `mutating_write_fingerprint_covers_*` PASS
- vitest `contentProtocol.test.ts` 4/4
- clippy `-D warnings` on agent/api/storage libs PASS
- 138 PID 89912 untouched

## Next
1. TipTap / true inline editor swap for production composer.
2. History chip rebuild from persisted nodes + resend draft UX polish.
3. Live pairing after 138 releases Spark (G1/G2 write-gate probes + E 10).
4. C a11y 5-person / D mobile / F tip138+Appliance.

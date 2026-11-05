# On-demand chat memory (2026-11-05)

User decision: long-term memory is available through explicit tool reads, not
injected into every chat. Current conversation history and compaction summaries
continue to work. No vector store, retrieval model, or keyword routing added.

## Changes

- Removed SessionManager's automatic USER/KNOWLEDGE/PROCEDURES snapshot loading
  and cache, and the Agent/LlmInterface snapshot injection paths. Memory content
  enters chat as a tool result only after a `memory` read/list request.
- Updated the slim prompt and tool description: read the relevant target on
  demand; “continue” follows the current conversation; historical facts do not
  authorize resuming old tasks. Temporary faults and assistant guesses are not
  durable facts to save.
- Added `MemoryConfig.auto_extract` (serde/default false), independent of memory
  tool access. Existing configuration files also default to false. Operators can
  explicitly opt in through the existing memory-config API; that retains the
  legacy extractor's limitations. Explicit add/replace/remove remain available.
- A chat gets its own memory-tool session handle and local enabled gate. Turning
  memory off hides the tool and rejects execution (including cached reads),
  without disabling another chat or scheduled agent. Session scratch notes no
  longer share the mutable global handle. Global tool-disable policy still applies.
- Unified API initialization/read helpers with `MemoryConfig.storage_path`, as
  already used by extraction. A running store still requires restart after a
  storage-path change; no live migration was introduced.

## Boundaries

Existing user memory files are preserved. Already-read tool results remain part
of that conversation's history; disabling memory is not retroactive erasure.
Operations already executing are not rolled back. Scheduled-agent journals and
periodic system-context maintenance are unchanged; `auto_extract` controls the
chat-turn extractor, not every maintenance job.

A model may still omit a useful memory read or read unnecessary memory. Tests
below establish request assembly and tool contracts, not universal prevention of
task drift or a Spark score improvement. No real-model paired eval is claimed.

## Verification

Artifacts: `/tmp/neomind-memory-demand-20261105/` on Mac.

- `memory_on_demand_session_requests` runs its child test in an isolated process
  with a temporary NEOMIND_DATA_DIR. It seeds old widget/user, knowledge, and SOP
  markers, captures real LLM requests from nonstream and stream session paths,
  asserts no unsolicited memory, retains the current task through “继续”, and
  observes a knowledge marker only after an explicit memory-tool read. A stale
  model tool call is rejected when the session's memory switch is off.
- `memory_toggle_is_session_local_and_survives_restore` checks cached-read and
  write rejection, re-enabling, DB restore, independent session notes, and the
  shared global disable policy.
- Storage config test verifies opt-in extraction for old and new configs.
- Targeted agent memory tests: 9 passed, 0 failed; the isolated child is invoked
  and asserted by the parent (its standalone entry is intentionally ignored).

### Takeover verification (2026-11-05 evening, Mac-local GLM)

- Agent lib 826/0 before the tool-gap fix below; full workspace
  `cargo test --workspace --locked --features neomind-agent/test-utils` green
  (all suites ok). One panicked thread in `builtin_llm_bootstrap`
  (bootstrap_spawns_vision_companion_alongside_main) under parallel load — the
  test passes standalone (4.8s); environmental contention, not a regression.
- Clippy `-D warnings` clean on neomind-agent/api/storage (the pre-existing
  `neomind-cli-ops` connector.rs nonminimal_bool lint predates this workstream
  and is not touched).

### Tool-gap fix after live use (2026-11-05 evening)

A real chat ("clean up memory") exposed three gaps: the model bypassed the
memory tool and rewrote a memory file with `file_write` — there was no
whole-file cleanup action, `list` previews (50 chars) were too short to clean
from, and file tools could reach `<data_dir>/memory`. Fixed in-worktree:

- `memory(action="rewrite", target, content)`: whole-file replacement for
  user/knowledge/procedures/session/custom, routed through the store so the
  configured char budgets are enforced (overflow rejected naming the limit).
- `list` previews extended to 200 chars (full text remains one `read` away).
- `PathValidator` denies file_read/file_write/file_edit under `<data_dir>/memory`
  with a pointer to the memory tool; only the exact `memory` directory is
  reserved (`memory-notes.md`, `memory-backup/` stay allowed). Custom storage
  paths outside the default directory are outside the file tools' sandbox.
- Tests: rewrite replace/budget/invalid-target, preview length, memory-dir
  denial vs adjacent names — agent lib 830/0, integration 46/0, fmt+clippy clean.

Committed with user authorization 2026-11-05 (memory workstream only; the
unrelated capability_providers model-override hunk remains uncommitted).
No push, deployment, production-service restart, or memory-file migration.

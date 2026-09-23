// Shared editor constants — split from AgentEditorFullScreen.tsx

// ============================================================================
// Constants
// ============================================================================

/**
 * How far back collection reaches for a source that has no explicit
 * `time_range_minutes` — the "Look back" control in the selected-resource card.
 *
 * MUST stay equal to `DEFAULT_TIME_RANGE_MINUTES` in
 * `crates/neomind-agent/src/ai_agent/executor/data_collector.rs`. The editor
 * displays this number; the executor is what actually applies it to a resource
 * saved without one — if the two disagree, the editor silently lies about what
 * the agent will read. A drift test locks the pair, in `mod tests` of that
 * same Rust file.
 */
export const DEFAULT_LOOKBACK_MINUTES = 60

/**
 * How many tool rounds a new agent starts with.
 *
 * MUST stay equal to `DEFAULT_MAX_CHAIN_DEPTH` in
 * `crates/neomind-storage/src/agents.rs` — for the same reason as the lookback
 * above, plus one: the editor also uses this number as its "unchanged"
 * sentinel. A create request that leaves the field here sends nothing, and the
 * API applies its own default. Drift, and the form shows one budget while the
 * agent is created with another. A drift test locks the pair, in `mod tests`
 * of that same Rust file.
 */
export const DEFAULT_MAX_CHAIN_DEPTH = 10

/**
 * The notification routing every new agent gets when the caller does not name
 * one, mirrored from `default_notify()` in
 * `crates/neomind-api/src/handlers/agents.rs`.
 *
 * The editor initials its notify card from these two, because the API applies
 * this floor to any create request that omits `notify` — UI-created, CLI-
 * created and API-created alike. Without the mirror the form reads "no
 * notification configured" while the agent that gets created routes to IM on
 * failure. A drift test holds the pair, in `crates/neomind-api/tests/handlers/
 * agents.rs`.
 */
export const DEFAULT_NOTIFY_CHANNEL = 'IM'
export const DEFAULT_NOTIFY_ON = 'failure'

export const INTERVALS = [5, 10, 15, 30, 60]
export const HOURS = Array.from({ length: 24 }, (_, i) => i)


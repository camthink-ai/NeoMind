/**
 * Decision-layer runtime configuration — GET/PUT /api/decisions/config.
 *
 * Everything here is hot-reloadable server-side (no restart, no env vars);
 * `engine.int8` and `budgets.sidecar_ms` take effect at the next
 * decision-layer refresh (PUT triggers it).
 */

import { fetchAPI } from "./client"

export interface DecisionGates {
  domain: number
  other: number
}

export interface DecisionBudgets {
  shortcircuit_ms: number
  ask_ms: number
  sidecar_ms: number
}

export interface DecisionConfig {
  /** null = no decision service resolved at all (extension uninstalled /
   *  no backend) — the persisted setting is intact but nothing is running.
   *  The status card shows "Not enabled" in that state instead of a
   *  misleading posture badge. */
  shadow: boolean | null
  shortcircuit: boolean
  gates: DecisionGates
  budgets: DecisionBudgets
  engine: { int8: boolean }
  backend: string | null
  backends: {
    native: { available: boolean; note: string }
    extension: { available: boolean; note: string }
    env_url: { available: boolean; note: string }
  }
}

export interface DecisionConfigUpdate {
  shadow?: boolean
  shortcircuit?: boolean
  gates?: { domain?: number; other?: number }
  budgets?: {
    shortcircuit_ms?: number
    ask_ms?: number
    sidecar_ms?: number
  }
  engine?: { int8?: boolean }
}

export interface DecisionStatus {
  enabled: boolean
  available?: boolean
  shadow?: boolean
  backend?: string
  decides_total?: number
  cache_hits_total?: number
  errors_total?: number
  reason?: string
}

export const decisionApi = {
  getConfig: () => fetchAPI<DecisionConfig>("/decisions/config"),
  updateConfig: (update: DecisionConfigUpdate) =>
    fetchAPI<DecisionConfig>("/decisions/config", {
      method: "PUT",
      body: JSON.stringify(update),
    }),
  getStatus: () => fetchAPI<DecisionStatus>("/decisions/status"),
}

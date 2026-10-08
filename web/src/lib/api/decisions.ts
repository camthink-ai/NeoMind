/** decisions api — the LAYA decision layer's status surface. */
import { fetchAPI } from "./client"

/** GET /decisions/status — configuration, availability and counters.
 *  `enabled: false` (no artifacts / non-decision build) carries a `reason`. */
export interface DecisionStatus {
  enabled: boolean
  reason?: string
  available?: boolean
  shadow?: boolean
  backend?: string
  decides_total?: number
  cache_hits_total?: number
  errors_total?: number
}

export const decisionsApi = {
  getDecisionStatus: () => fetchAPI<DecisionStatus>("/decisions/status"),
}

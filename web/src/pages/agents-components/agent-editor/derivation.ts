// The editor no longer asks the customer to pick an execution mode. It asks
// what the agent should do — what it watches, what it records, whether it acts
// — and derives the mode from those answers. The rule lives here so it can be
// asserted, and so the form has one source for it.

import type { AgentExecutionMode } from '@/types'

export interface DerivationInput {
  /** The user bound device commands: the agent has to act, not just observe. */
  hasDeviceCommands: boolean
  /**
   * The user said the agent may work things out itself. This is the only input
   * that separates "investigate" from "summarise" — both bind nothing and
   * record nothing, so no resource or field can tell them apart.
   */
  canActAutonomously: boolean
  /** The user defined output fields: the run has to end in a fixed shape. */
  hasOutputContract: boolean
}

/**
 * - commands → `free`: acting takes rounds (call, verify, retry).
 * - output contract → `structured`: one constrained inference, fields out.
 * - neither → `focused`: look at the data and answer.
 *
 * Commands win over the contract. An agent that acts still publishes its
 * fields — via the post-run output-contract step — rather than being
 * downgraded to a single constrained inference and losing its ability to act.
 */
export function deriveExecutionMode({
  hasDeviceCommands,
  canActAutonomously,
  hasOutputContract,
}: DerivationInput): AgentExecutionMode {
  if (hasDeviceCommands || canActAutonomously) return 'free'
  if (hasOutputContract) return 'structured'
  return 'focused'
}

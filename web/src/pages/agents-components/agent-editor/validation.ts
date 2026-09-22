// Editor-level rules that the server also enforces — failing here beats
// failing at execution time, after the user has already left the dialog.

import type { OperatorField } from '@/types'

/**
 * A structured (L0) agent publishes one data source per output field
 * (`ai:{agent_id}:{field}`). With no named field there is nothing to publish,
 * and `execute_structured` rejects every run with "add fields in the editor" —
 * so the editor must not let one be saved in that state.
 */
export function hasOutputContract(outputSchema: OperatorField[]): boolean {
  return outputSchema.some((field) => field.name.trim() !== '')
}

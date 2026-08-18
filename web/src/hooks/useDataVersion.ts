/**
 * Subscribe to a data domain's change counter.
 *
 * The counter is bumped whenever a DataChanged event arrives for the domain
 * (AI agent action, another client, background job — see useDataChangeEvents).
 * Pages that load data locally add the returned value to their fetch-effect
 * deps to refetch without a manual reload.
 *
 * Pass several related domains to follow the max of them, e.g.
 * `useDataVersion('agents', 'skills')` for the agents page tabs.
 */
import { useStore } from '@/store'

export function useDataVersion(...domains: string[]): number {
  return useStore((s) => {
    let max = 0
    for (const d of domains) {
      const v = s.dataVersions[d] ?? 0
      if (v > max) max = v
    }
    return max
  })
}

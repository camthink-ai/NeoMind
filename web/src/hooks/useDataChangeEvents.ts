/**
 * Global DataChanged listener — the client half of the server's data-change
 * middleware. Whenever a mutating request succeeds on a data domain (the AI
 * agent driving the neomind CLI, another client, a background job), the
 * backend publishes NeoMindEvent::DataChanged on the event bus; this hook
 * receives it and refreshes:
 *
 * - store-gated domains: invalidate the fetch cache + rerun the store loader
 * - everything else: bump the domain's dataVersion so pages that load data
 *   locally (via useDataVersion) refetch
 *
 * Events are debounced per domain — an agent command burst produces one
 * refetch, not ten. Dashboard echoes of our own saves are skipped (they
 * would overwrite in-progress edits, the same echo the CRUD slice
 * suppresses for DashboardUpdated SSE).
 */
import { useEffect, useRef } from 'react'
import { useStore } from '@/store'
import { getEventsConnection } from '@/lib/events'
import { fetchCache } from '@/lib/utils/async'
import { hasAnyRecentSelfSync } from '@/store/slices/dashboardCrudSlice'

const DEBOUNCE_MS = 400

/** fetchCache keys to invalidate per domain (store-gated loaders). */
const CACHE_KEYS: Record<string, string[]> = {
  devices: ['devices', 'deviceTypes', 'devicesCurrentBatch'],
  'device-types': ['deviceTypes'],
  extensions: ['extensions', 'extensionTypes'],
  instances: ['instances'],
  llm: ['llmBackends', 'llmBackendTypes'],
  sessions: ['sessions'],
}

export function useDataChangeEvents() {
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>())

  useEffect(() => {
    // Unfiltered connection (no category) so DataChanged reaches us.
    const conn = getEventsConnection('data-changes')

    const off = conn.on('DataChanged', (event) => {
      // The events channel wraps payloads: { id, type, timestamp, source,
      // data: { domain, method, path } }. Accept flat too, just in case.
      const raw = event as unknown as { domain?: string; data?: { domain?: string } }
      const domain = raw.domain ?? raw.data?.domain
      if (!domain) return

      // Skip echoes of our own dashboard saves — refetching mid-edit would
      // clobber in-progress layout changes (same suppression as SSE echo).
      if (domain === 'dashboards' && hasAnyRecentSelfSync()) return

      const existing = timers.current.get(domain)
      if (existing) clearTimeout(existing)
      timers.current.set(
        domain,
        setTimeout(() => {
          timers.current.delete(domain)
          const s = useStore.getState()

          for (const key of CACHE_KEYS[domain] ?? []) fetchCache.invalidate(key)
          switch (domain) {
            case 'devices':
              void s.fetchDevices()
              void s.fetchDeviceTypes()
              break
            case 'device-types':
              void s.fetchDeviceTypes()
              break
            case 'extensions':
              void s.fetchExtensions()
              break
            case 'instances':
              void s.fetchInstances()
              break
            case 'llm':
              void s.loadBackends()
              break
            case 'sessions':
              void s.loadSessions()
              break
            case 'dashboards':
              void s.fetchDashboards()
              break
          }

          // Always bump so page-local loaders (useDataVersion) refetch too
          s.bumpDataVersion(domain)
        }, DEBOUNCE_MS)
      )
    })

    // Builtin model download completion: the wizard/indicator observe status,
    // but gated surfaces (chat empty state, agents banner) read llmBackends
    // from the store — refresh it so they flip without a manual reload. The
    // server also auto-activates, so onboarding's own 5s poll flips too.
    const offBuiltin = conn.on('ModelDownloadProgress', (event) => {
      const d = (event as { data?: { status?: string } }).data
      if (d?.status !== 'complete') return
      const s = useStore.getState()
      fetchCache.invalidate('llmBackends')
      void s.loadBackends()
    })

    return () => {
      off()
      offBuiltin()
      timers.current.forEach((t) => clearTimeout(t))
      timers.current.clear()
    }
  }, [])
}

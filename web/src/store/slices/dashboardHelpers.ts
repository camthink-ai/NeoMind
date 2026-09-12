/**
 * Shared helpers for dashboard slices.
 * Extracted from dashboardSlice.ts to avoid duplication across split slices.
 */

import type {
  Dashboard,
  DashboardComponent,
  DashboardTemplate,
} from '@/types/dashboard'

/** Combined dashboard store type for StateCreator generics.
 *  Includes only the fields accessed by dashboard slices — enough for
 *  type-safe StateCreator usage without circular imports. */
export type DashboardStore = {
  // CrudSlice
  dashboards: Dashboard[]
  currentDashboard: Dashboard | null
  currentDashboardId: string | null
  templates: DashboardTemplate[]
  _fetchId: number | null
  dashboardsLoading: boolean
  scheduleSync: (dashboard: Dashboard) => void
  flushSync: () => Promise<void>
  fetchDashboards: () => Promise<void>
  // UISlice
  editMode: boolean
  selectedComponent: string | null
  configComponentId: string | null
  componentLibraryOpen: boolean
  configPanelOpen: boolean
  templateDialogOpen: boolean
  // DeviceSlice
  devices: import('@/types').Device[]
  // ExtensionSlice
  extensions: import('@/types').Extension[]
}

// Re-export shared generateId from lib/id to avoid duplicating the identical
// crypto.randomUUID() + Math.random() fallback implementation.
export { generateId } from '@/lib/id'

let pendingCleanupCount = 0
const MAX_CONCURRENT_CLEANUPS = 3

/** Delete the associated AI Agent when an ai-analyst component is removed */
export function cleanupAgentForComponent(component: DashboardComponent | undefined) {
  if (!component || component.type !== 'ai-analyst') return
  const agentId = (component.config?.agentId as string | undefined)
  if (!agentId) return
  if (pendingCleanupCount >= MAX_CONCURRENT_CLEANUPS) {
    console.warn('[Dashboard] Skipping agent cleanup — too many concurrent cleanups')
    return
  }
  pendingCleanupCount++
  import('@/lib/api').then(({ api }) => {
    return api.deleteAgent(agentId)
  }).catch((err) => {
    console.warn('[Dashboard] Failed to delete agent', agentId, err)
  }).finally(() => {
    pendingCleanupCount--
  })
}

/** Update currentDashboard and dashboards array atomically */
export function updateDashboardInState(
  dashboards: Dashboard[],
  targetId: string,
  updates: Partial<Dashboard>,
  currentDashboardId: string | null,
): { dashboards: Dashboard[]; currentDashboard: Dashboard | null } {
  const updatedDashboards = dashboards.map((d) =>
    d.id === targetId ? { ...d, ...updates } : d,
  )
  const currentDashboard = updatedDashboards.find((d) => d.id === currentDashboardId) || null
  return { dashboards: updatedDashboards, currentDashboard }
}

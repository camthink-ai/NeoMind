/**
 * Dashboard Slice (Refactored)
 *
 * Simplified state management using the persistence layer.
 * Removes code duplication and separates concerns.
 */

import type { StateCreator } from 'zustand'
import type {
  Dashboard,
  DashboardComponent,
  DashboardTemplate,
  ComponentPosition,
  DashboardLayout,
} from '@/types/dashboard'
import { createDashboardStorage, type DashboardStorage } from '../persistence'
import { logError } from '@/lib/errors'

// ============================================================================
// Default Layout
// ============================================================================

export const DEFAULT_LAYOUT: DashboardLayout = {
  columns: 12,
  rows: 'auto',
  breakpoints: {
    lg: 1200,
    md: 996,
    sm: 768,
    xs: 480,
  },
}

// ============================================================================
// Dashboard State
// ============================================================================

export interface DashboardState {
  // Data
  dashboards: Dashboard[]
  currentDashboard: Dashboard | null
  currentDashboardId: string | null
  templates: DashboardTemplate[]

  // Internal: Track current fetch request to prevent race conditions
  _fetchId: number | null

  // UI State
  dashboardsLoading: boolean
  editMode: boolean
  selectedComponent: string | null

  // Panels
  componentLibraryOpen: boolean
  configPanelOpen: boolean
  configComponentId: string | null
  templateDialogOpen: boolean

  // Actions
  // Dashboard management
  fetchDashboards: () => Promise<void>
  createDashboard: (dashboard: Omit<Dashboard, 'id' | 'createdAt' | 'updatedAt'>) => Promise<string>
  updateDashboard: (id: string, updates: Partial<Dashboard>) => Promise<void>
  deleteDashboard: (id: string) => Promise<void>
  setCurrentDashboard: (id: string | null) => void
  setDefaultDashboard: (id: string) => Promise<void>
  clearDashboards: () => void

  // Component management
  addComponent: (component: Omit<DashboardComponent, 'id'>) => void
  updateComponent: (id: string, updates: Partial<DashboardComponent>, persist?: boolean) => void
  removeComponent: (id: string) => void
  removeComponentsByExtension: (extensionId: string) => void
  moveComponent: (id: string, position: ComponentPosition) => void
  duplicateComponent: (id: string) => void

  // Persistence
  persistDashboard: (id?: string) => Promise<void>

  // UI state
  setEditMode: (edit: boolean) => void
  setSelectedComponent: (id: string | null) => void
  setComponentLibraryOpen: (open: boolean) => void
  setConfigPanelOpen: (open: boolean, componentId?: string) => void
  setTemplateDialogOpen: (open: boolean) => void

  // Templates
  applyTemplate: (template: DashboardTemplate) => void
  fetchTemplates: () => Promise<void>
}

// ============================================================================
// Default Templates
// ============================================================================

export const DEFAULT_TEMPLATES: DashboardTemplate[] = [
  {
    id: 'overview',
    name: 'Overview',
    description: 'System overview with devices, agents, and events',
    category: 'overview',
    icon: 'LayoutDashboard',
    layout: DEFAULT_LAYOUT,
    components: [],
    requiredResources: { devices: 1 },
  },
  {
    id: 'blank',
    name: 'Blank Canvas',
    description: 'Start from scratch with an empty dashboard',
    category: 'custom',
    icon: 'Square',
    layout: DEFAULT_LAYOUT,
    components: [],
  },
]

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Update currentDashboard and dashboards array atomically
 */
function updateDashboardInState(
  dashboards: Dashboard[],
  targetId: string,  // ID of the dashboard to update
  updates: Partial<Dashboard>,
  currentDashboardId: string | null  // Current active dashboard ID
): { dashboards: Dashboard[]; currentDashboard: Dashboard | null } {
  const updatedDashboards = dashboards.map((d) =>
    d.id === targetId ? { ...d, ...updates } : d
  )

  // If the updated dashboard is the current one, update currentDashboard too
  const currentDashboard = updatedDashboards.find((d) => d.id === currentDashboardId) || null

  return { dashboards: updatedDashboards, currentDashboard }
}

/**
 * Generate unique ID
 */
function generateId(): string {
  // Use crypto.randomUUID() if available, otherwise fallback to a simple random string generator
  if (typeof crypto !== 'undefined' && crypto.randomUUID && typeof crypto.randomUUID === 'function') {
    try {
      return crypto.randomUUID()
    } catch {
      // Fall through to fallback
    }
  }
  // Fallback: generate a random string using Math.random()
  return 'id_' + Date.now().toString(36) + '_' + Math.random().toString(36).substring(2, 15)
}

/**
 * Deep clone utility using structuredClone with fallback
 * Preserves more types than JSON.stringify (Date, Map, Set, etc.)
 */
function deepClone<T>(obj: T): T {
  if (typeof structuredClone !== 'undefined') {
    try {
      return structuredClone(obj)
    } catch {
      // Fallback for circular references or unsupported types
    }
  }
  // Fallback to JSON method for older browsers
  return JSON.parse(JSON.stringify(obj))
}

// ============================================================================
// Create Slice
// ============================================================================

export const createDashboardSlice: StateCreator<
  DashboardState,
  [],
  [],
  DashboardState
> = (set, get) => {
  // Clear old localStorage dashboard data on initialization
  try {
    localStorage.removeItem('neomind_dashboards')
    localStorage.removeItem('neomind_current_dashboard_id')
    
  } catch (e) {
    // Ignore
  }

  // Initialize storage - use API as primary with localStorage fallback for caching
  const storage: DashboardStorage = createDashboardStorage({ type: 'hybrid', cacheEnabled: true })

  return {
    // Initial state
    dashboards: [],
    currentDashboard: null,
    currentDashboardId: null,
    templates: DEFAULT_TEMPLATES,
    _fetchId: null,

    dashboardsLoading: false,
    editMode: false,
    selectedComponent: null,

    componentLibraryOpen: false,
    configPanelOpen: false,
    configComponentId: null,
    templateDialogOpen: false,

    // ========================================================================
    // Dashboard CRUD
    // ========================================================================

    fetchDashboards: async () => {
      // Generate unique ID for this fetch request
      const fetchId = Date.now()
      set({ _fetchId: fetchId, dashboardsLoading: true })

      try {
        const result = await storage.load()

        // Check if this is still the latest fetch request
        const currentState = get()
        if (currentState._fetchId !== fetchId) {
          // A newer fetch request has been initiated, discard this result
          
          return
        }

        if (result.error) {
          console.warn('[DashboardSlice] Failed to load dashboards:', result.error.message)
          // Set empty array when loading fails
          set({ dashboards: [], dashboardsLoading: false })
        } else if (result.data) {
          // Migration: Move dataSource from config to component level
          // This handles legacy data where dataSource was stored inside config
          const migratedDashboards = result.data.map((dashboard: Dashboard) => {
            return {
              ...dashboard,
              components: dashboard.components.map((component: any) => {
                const config = component.config || {}
                const configDataSource = config.dataSource

                // If dataSource is in config but not at component level, migrate it
                if (configDataSource) {
                  // Remove from config and add to component level
                  const { dataSource, ...configWithoutDataSource } = config
                  return {
                    ...component,
                    config: configWithoutDataSource,
                    dataSource: configDataSource,
                  }
                }

                return component
              }),
            }
          })

          // Final check before updating state
          const finalState = get()
          if (finalState._fetchId !== fetchId) {
            
            return
          }

          set({ dashboards: migratedDashboards })

          // Set current dashboard if not set
          const savedId = (storage as any).getCurrentDashboardId?.()

          // Determine which dashboard to set as current - use MIGRATED data, not original
          let dashboardToSet: Dashboard | null = null

          if (savedId && migratedDashboards.length > 0) {
            // First try to use the saved ID from localStorage - use MIGRATED dashboards
            const savedDashboard = migratedDashboards.find((d) => d.id === savedId)
            if (savedDashboard) {
              dashboardToSet = savedDashboard
            }
          }

          // If no saved dashboard or saved ID not found, use default or first - use MIGRATED dashboards
          if (!dashboardToSet && !currentState.currentDashboardId && migratedDashboards.length > 0) {
            dashboardToSet = migratedDashboards.find((d) => d.isDefault) || migratedDashboards[0]
          }

          // Only update if we found a dashboard and don't already have one set
          if (dashboardToSet && !currentState.currentDashboard) {
            set({
              currentDashboardId: dashboardToSet.id,
              currentDashboard: dashboardToSet,
            })
            
          }
        } else {
          // No data and no error - treat as empty
          set({ dashboards: [] })
        }
      } catch (err) {
        logError(err, { operation: 'Load dashboards' })
        // Only update state if this is still the latest fetch
        const currentState = get()
        if (currentState._fetchId === fetchId) {
          // Set empty array on unexpected errors
          set({ dashboards: [] })
        }
      } finally {
        // Only clear loading if this is still the latest fetch
        const currentState = get()
        if (currentState._fetchId === fetchId) {
          set({ dashboardsLoading: false })
        }
      }
    },

    createDashboard: async (dashboard) => {
      // Always create locally first (fast, reliable)
      const localDashboard: Dashboard = {
        ...dashboard,
        id: generateId(),
        createdAt: Date.now(),
        updatedAt: Date.now(),
      }

      // Update state immediately
      set((state) => ({
        dashboards: [...state.dashboards, localDashboard],
        currentDashboardId: localDashboard.id,
        currentDashboard: localDashboard,
      }))

      // Persist to storage (localStorage + try API)
      // For local dashboards, we wait for API to get the server ID
      const isLocalDashboard = !localDashboard.id.startsWith('dashboard_')
      if (isLocalDashboard) {
        try {
          const result = await storage.sync(localDashboard)
          if (result.data && result.data.id !== localDashboard.id) {
            // Server assigned a new ID - update state
            
            const { dashboards: currentDashboards } = get()
            const newDashboards = currentDashboards.map((d) =>
              d.id === localDashboard.id ? result.data : d
            ).filter((d): d is Dashboard => d !== null)
            set({
              dashboards: newDashboards,
              currentDashboard: result.data,
              currentDashboardId: result.data?.id,
            })
            return result.data.id
          }
        } catch (err) {
          // Sync failed - but dashboard is already saved locally
          console.warn('[DashboardSlice] Background sync failed:', err)
        }
      } else {
        // For server dashboards, sync in background
        storage.sync(localDashboard).catch((err) => {
          console.warn('[DashboardSlice] Background sync failed:', err)
        })
      }

      return localDashboard.id
    },

    updateDashboard: async (id, updates) => {
      const { dashboards, currentDashboardId } = get()

      const updated = {
        ...updates,
        updatedAt: Date.now(),
      } as Partial<Dashboard>

      const { dashboards: updatedDashboards, currentDashboard } =
        updateDashboardInState(dashboards, id, updated, currentDashboardId)

      // Update state immediately
      set({
        dashboards: updatedDashboards,
        currentDashboard: currentDashboardId === id ? currentDashboard : get().currentDashboard,
      })

      // Persist to storage in background
      const targetDashboard = updatedDashboards.find(d => d.id === id)
      if (targetDashboard) {
        storage.sync(targetDashboard).catch((err) => {
          // Sync failed - but state is already saved locally
          console.warn('[DashboardSlice] Background sync failed:', err)
        })
      }
    },

    deleteDashboard: async (id) => {
      const { dashboards, currentDashboardId } = get()

      const updated = dashboards.filter((d) => d.id !== id)
      const newCurrentId = currentDashboardId === id ? (updated[0]?.id || null) : currentDashboardId

      set({
        dashboards: updated,
        currentDashboardId: newCurrentId,
        currentDashboard: updated.find((d) => d.id === newCurrentId) || null,
      })

      await storage.delete(id)
    },

    setCurrentDashboard: (id) => {
      const { dashboards } = get()
      const dashboard = id
        ? dashboards.find((d) => d.id === id) || null
        : dashboards[0] || null

      set({
        currentDashboardId: id,
        currentDashboard: dashboard,
        editMode: false,
        selectedComponent: null,
      })

      // Save to localStorage
      ;(storage as any).setCurrentDashboardId?.(id)
    },

    setDefaultDashboard: async (id) => {
      const { dashboards } = get()

      const updated = dashboards.map((d) => ({
        ...d,
        isDefault: d.id === id,
      }))

      set({ dashboards: updated })

      await storage.save(updated)
    },

    clearDashboards: () => {
      // Clear storage and reset state
      storage.clear()
      set({
        dashboards: [],
        currentDashboard: null,
        currentDashboardId: null,
      })
    },

    persistDashboard: async (id) => {
      const { currentDashboard, dashboards } = get()
      const dashboardToPersist = id
        ? dashboards.find((d) => d.id === id)
        : currentDashboard

      if (!dashboardToPersist) {
        console.warn('[persistDashboard] No dashboard to persist')
        return
      }

      await storage.sync(dashboardToPersist)
    },

    addComponent(component) {
      const { currentDashboard, dashboards, currentDashboardId } = get()
      if (!currentDashboard) {
        console.error('[DashboardSlice] addComponent: No current dashboard')
        return
      }

      const newComponent = { ...component, id: generateId() }
      const updatedDashboard = {
        ...currentDashboard,
        components: [...currentDashboard.components, newComponent],
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
      })

      // Sync to storage and handle ID change if dashboard was just created on server
      storage.sync(updatedDashboard).then((result) => {
        if (result.data && result.data.id !== updatedDashboard.id) {
          // Server assigned a new ID - update state
          
          const { dashboards: currentDashboards, currentDashboardId: currId } = get()
          const newDashboards = currentDashboards.map((d) =>
            d.id === updatedDashboard.id ? result.data : d
          ).filter((d): d is Dashboard => d !== null)
          set({
            dashboards: newDashboards,
            currentDashboard: result.data,
            currentDashboardId: result.data?.id,
          })
        }
      }).catch((err) => {
        console.warn('[DashboardSlice] Failed to sync after addComponent:', err)
      })
    },

    updateComponent(id, updates, persist = true) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) {
        console.error('[DashboardSlice] updateComponent: No current dashboard')
        return
      }

      // Validate component exists
      const componentExists = currentDashboard.components.some((c) => c.id === id)
      if (!componentExists) {
        console.warn('[DashboardSlice] updateComponent: Component not found:', id)
        return
      }

      const updatedDashboard = {
        ...currentDashboard,
        components: currentDashboard.components.map((c) =>
          c.id === id ? { ...c, ...updates } : c
        ),
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
      })

      // Only persist if persist=true (default for backward compatibility)
      if (persist) {
        storage.sync(updatedDashboard).then((result) => {
          if (result.data && result.data.id !== updatedDashboard.id) {
            // Server assigned a new ID - update state
            
            const { dashboards: currentDashboards } = get()
            const newDashboards = currentDashboards.map((d) =>
              d.id === updatedDashboard.id ? result.data : d
            ).filter((d): d is Dashboard => d !== null)
            set({
              dashboards: newDashboards,
              currentDashboard: result.data,
              currentDashboardId: result.data?.id,
            })
          }
        }).catch((err) => {
          console.warn('[DashboardSlice] Failed to sync after updateComponent:', err)
        })
      }
    },

    removeComponent(id) {
      const { currentDashboard, dashboards, selectedComponent, configComponentId } = get()
      if (!currentDashboard) return

      const updatedDashboard = {
        ...currentDashboard,
        components: currentDashboard.components.filter((c) => c.id !== id),
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      // Clear selection if the deleted component was selected
      const newSelectedComponent = selectedComponent === id ? null : selectedComponent
      const newConfigComponentId = configComponentId === id ? null : configComponentId

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
        selectedComponent: newSelectedComponent,
        configComponentId: newConfigComponentId,
      })

      // Sync and handle ID change if dashboard was just created on server
      storage.sync(updatedDashboard).then((result) => {
        if (result.data && result.data.id !== updatedDashboard.id) {
          // Server assigned a new ID - update state
          
          const { dashboards: currentDashboards } = get()
          const newDashboards = currentDashboards.map((d) =>
            d.id === updatedDashboard.id ? result.data : d
          ).filter((d): d is Dashboard => d !== null)
          set({
            dashboards: newDashboards,
            currentDashboard: result.data,
            currentDashboardId: result.data?.id,
          })
        }
      }).catch(() => {})
    },

    removeComponentsByExtension(extensionId: string) {
      const { currentDashboard, dashboards, selectedComponent, configComponentId } = get()
      if (!currentDashboard) return

      // Find all components that belong to this extension
      const componentsToRemove = currentDashboard.components.filter(
        (comp) => {
          // Check if component type matches extension's component pattern
          // Extension components typically have types like "weather-card" or contain extension_id in dataSource
          const dataSource = 'dataSource' in comp ? (comp.dataSource as any) : undefined
          return (
            comp.type.startsWith(`${extensionId}:`) ||
            comp.type.includes(`-${extensionId}-`) ||
            dataSource?.extensionId === extensionId ||
            dataSource?.extension_id === extensionId
          )
        }
      )

      if (componentsToRemove.length === 0) return

      const componentIdsToRemove = new Set(componentsToRemove.map((c) => c.id))

      const updatedDashboard = {
        ...currentDashboard,
        components: currentDashboard.components.filter((c) => !componentIdsToRemove.has(c.id)),
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      // Clear selection if the deleted component was selected
      const newSelectedComponent =
        selectedComponent && componentIdsToRemove.has(selectedComponent) ? null : selectedComponent
      const newConfigComponentId =
        configComponentId && componentIdsToRemove.has(configComponentId) ? null : configComponentId

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
        selectedComponent: newSelectedComponent,
        configComponentId: newConfigComponentId,
      })

      // Persist changes
      storage.sync(updatedDashboard).catch(() => {})

      console.log(
        `[DashboardSlice] Removed ${componentsToRemove.length} components from extension ${extensionId}`
      )
    },

    moveComponent(id, position) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) {
        console.error('[DashboardSlice] moveComponent: No current dashboard')
        return
      }

      // Validate component exists
      const componentExists = currentDashboard.components.some((c) => c.id === id)
      if (!componentExists) {
        console.warn('[DashboardSlice] moveComponent: Component not found:', id)
        return
      }

      const updatedDashboard = {
        ...currentDashboard,
        components: currentDashboard.components.map((c) =>
          c.id === id
            ? { ...c, position: { ...c.position, ...position } }
            : c
        ),
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
      })

      // Debounced sync - only sync after 500ms of inactivity
      // This prevents excessive API calls during drag operations
      if ((window as any).__dashboardSyncTimeout) {
        clearTimeout((window as any).__dashboardSyncTimeout)
      }
      ;(window as any).__dashboardSyncTimeout = setTimeout(() => {
        storage.sync(updatedDashboard).then((result) => {
          if (result.data && result.data.id !== updatedDashboard.id) {
            // Server assigned a new ID - update state

            const { dashboards: currentDashboards } = get()
            const newDashboards = currentDashboards.map((d) =>
              d.id === updatedDashboard.id ? result.data : d
            ).filter((d): d is Dashboard => d !== null)
            set({
              dashboards: newDashboards,
              currentDashboard: result.data,
              currentDashboardId: result.data?.id,
            })
          }
        }).catch((err) => {
          console.warn('[DashboardSlice] Failed to sync after moveComponent:', err)
        })
      }, 500)
    },

    duplicateComponent(id) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) return

      const original = currentDashboard.components.find((c) => c.id === id)
      if (!original) return

      const newComponent = {
        ...deepClone(original),
        id: generateId(),
        position: {
          ...original.position,
          y: original.position.y + original.position.h,
        },
      } as DashboardComponent

      const updatedDashboard = {
        ...currentDashboard,
        components: [...currentDashboard.components, newComponent],
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
      })

      // Sync and handle ID change if dashboard was just created on server
      storage.sync(updatedDashboard).then((result) => {
        if (result.data && result.data.id !== updatedDashboard.id) {
          // Server assigned a new ID - update state
          
          const { dashboards: currentDashboards } = get()
          const newDashboards = currentDashboards.map((d) =>
            d.id === updatedDashboard.id ? result.data : d
          ).filter((d): d is Dashboard => d !== null)
          set({
            dashboards: newDashboards,
            currentDashboard: result.data,
            currentDashboardId: result.data?.id,
          })
        }
      }).catch(() => {})
    },

    // ========================================================================
    // UI State
    // ========================================================================

    setEditMode: (edit) => set({ editMode: edit, selectedComponent: null }),

    setSelectedComponent: (id) => {
      const { currentDashboard } = get()
      // Validate that the component exists before selecting it
      if (id && currentDashboard?.components) {
        const componentExists = currentDashboard.components.some((c) => c.id === id)
        if (!componentExists) {
          // Component no longer exists, clear selection
          set({ selectedComponent: null })
          return
        }
      }
      set({ selectedComponent: id })
    },

    setComponentLibraryOpen: (open) => set({ componentLibraryOpen: open }),

    setConfigPanelOpen: (open, componentId) => {
      const { currentDashboard } = get()
      // Validate that the component exists before opening config
      if (open && componentId && currentDashboard?.components) {
        const componentExists = currentDashboard.components.some((c) => c.id === componentId)
        if (!componentExists) {
          // Component no longer exists, don't open config panel
          set({ configPanelOpen: false, configComponentId: null })
          return
        }
      }
      set({
        configPanelOpen: open,
        configComponentId: componentId || null,
      })
    },

    setTemplateDialogOpen: (open) => set({ templateDialogOpen: open }),

    // ========================================================================
    // Templates
    // ========================================================================

    fetchTemplates: async () => {
      try {
        const api = (await import('@/lib/api')).api
        const data = await api.getDashboardTemplates()
        const validTemplates = data.filter((t: any) =>
          ['overview', 'monitoring', 'automation', 'agents', 'custom'].includes(t.category)
        ) as DashboardTemplate[]
        set({ templates: [...DEFAULT_TEMPLATES, ...validTemplates] })
      } catch {
        set({ templates: DEFAULT_TEMPLATES })
      }
    },

    applyTemplate: (template) => {
      const newDashboard: Dashboard = {
        id: generateId(),
        name: template.name,
        layout: template.layout,
        components: template.components.map((c) => ({
          ...c,
          id: generateId(),
        })) as DashboardComponent[],
        createdAt: Date.now(),
        updatedAt: Date.now(),
      }

      set((state) => ({
        dashboards: [...state.dashboards, newDashboard],
        currentDashboardId: newDashboard.id,
        currentDashboard: newDashboard,
      }))

      // Sync and handle ID change if dashboard was created on server
      storage.sync(newDashboard).then((result) => {
        if (result.data && result.data.id !== newDashboard.id) {
          // Server assigned a new ID - update state
          
          const { dashboards: currentDashboards } = get()
          const newDashboards = currentDashboards.map((d) =>
            d.id === newDashboard.id ? result.data : d
          ).filter((d): d is Dashboard => d !== null)
          set({
            dashboards: newDashboards,
            currentDashboard: result.data,
            currentDashboardId: result.data?.id,
          })
        }
      }).catch(() => {})
    },
  }
}

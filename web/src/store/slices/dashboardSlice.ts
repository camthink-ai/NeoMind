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
  updateComponent: (id: string, updates: Partial<DashboardComponent>) => void
  removeComponent: (id: string) => void
  moveComponent: (id: string, position: ComponentPosition) => void
  duplicateComponent: (id: string) => void

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
  currentDashboardId: string | null,
  updates: Partial<Dashboard>
): { dashboards: Dashboard[]; currentDashboard: Dashboard | null } {
  const updatedDashboards = dashboards.map((d) =>
    d.id === currentDashboardId ? { ...d, ...updates, updatedAt: Date.now() } : d
  )

  const currentDashboard = updatedDashboards.find((d) => d.id === currentDashboardId) || null

  return { dashboards: updatedDashboards, currentDashboard }
}

/**
 * Generate unique ID
 */
function generateId(): string {
  return crypto.randomUUID()
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
  // Initialize storage - use localStorage only since backend API is not fully implemented
  const storage: DashboardStorage = createDashboardStorage({ type: 'local' })

  return {
    // Initial state
    dashboards: [],
    currentDashboard: null,
    currentDashboardId: null,
    templates: DEFAULT_TEMPLATES,

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
      set({ dashboardsLoading: true })

      try {
        const result = await storage.load()

        if (result.error) {
          console.warn('[DashboardSlice] Failed to load dashboards:', result.error.message)
        } else if (result.data) {
          set({ dashboards: result.data })

          // Set current dashboard if not set
          const savedId = (storage as any).getCurrentDashboardId?.()
          if (!get().currentDashboardId && result.data.length > 0) {
            const defaultDashboard = result.data.find((d) => d.isDefault) || result.data[0]
            if (defaultDashboard) {
              set({
                currentDashboardId: defaultDashboard.id,
                currentDashboard: defaultDashboard,
              })
            }
          } else if (savedId) {
            const savedDashboard = result.data.find((d) => d.id === savedId)
            if (savedDashboard) {
              set({
                currentDashboardId: savedDashboard.id,
                currentDashboard: savedDashboard,
              })
            }
          }
        }
      } finally {
        set({ dashboardsLoading: false })
      }
    },

    createDashboard: async (dashboard) => {
      const newDashboard: Dashboard = {
        ...dashboard,
        id: generateId(),
        createdAt: Date.now(),
        updatedAt: Date.now(),
      }

      // Update local state immediately
      set((state) => ({
        dashboards: [...state.dashboards, newDashboard],
        currentDashboardId: newDashboard.id,
        currentDashboard: newDashboard,
      }))

      // Persist
      await storage.sync(newDashboard)

      return newDashboard.id
    },

    updateDashboard: async (id, updates) => {
      const { dashboards, currentDashboardId } = get()

      const updated = {
        ...updates,
        updatedAt: Date.now(),
      } as Partial<Dashboard>

      const { dashboards: updatedDashboards, currentDashboard } =
        updateDashboardInState(dashboards, id, updated)

      set({
        dashboards: updatedDashboards,
        currentDashboard: currentDashboardId === id ? currentDashboard : get().currentDashboard,
      })

      // Persist
      if (currentDashboard) {
        await storage.sync(currentDashboard)
      } else {
        const targetDashboard = dashboards.find((d) => d.id === id)
        if (targetDashboard) {
          await storage.sync({ ...targetDashboard, ...updated })
        }
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

    addComponent(component) {
      const { currentDashboard, dashboards, currentDashboardId } = get()
      if (!currentDashboard) return

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

      storage.sync(updatedDashboard).catch(() => {})
    },

    updateComponent(id, updates) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) return

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

      storage.sync(updatedDashboard).catch(() => {})
    },

    removeComponent(id) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) return

      const updatedDashboard = {
        ...currentDashboard,
        components: currentDashboard.components.filter((c) => c.id !== id),
        updatedAt: Date.now(),
      }

      const updatedDashboards = dashboards.map((d) =>
        d.id === currentDashboard.id ? updatedDashboard : d
      )

      set({
        dashboards: updatedDashboards,
        currentDashboard: updatedDashboard,
      })

      storage.sync(updatedDashboard).catch(() => {})
    },

    moveComponent(id, position) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) return

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

      storage.sync(updatedDashboard).catch(() => {})
    },

    duplicateComponent(id) {
      const { currentDashboard, dashboards } = get()
      if (!currentDashboard) return

      const original = currentDashboard.components.find((c) => c.id === id)
      if (!original) return

      const newComponent = {
        ...JSON.parse(JSON.stringify(original)),
        id: generateId(),
        position: {
          ...original.position,
          x: original.position.x + original.position.w,
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

      storage.sync(updatedDashboard).catch(() => {})
    },

    // ========================================================================
    // UI State
    // ========================================================================

    setEditMode: (edit) => set({ editMode: edit, selectedComponent: null }),

    setSelectedComponent: (id) => set({ selectedComponent: id }),

    setComponentLibraryOpen: (open) => set({ componentLibraryOpen: open }),

    setConfigPanelOpen: (open, componentId) => set({
      configPanelOpen: open,
      configComponentId: componentId || null,
    }),

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

      storage.sync(newDashboard).catch(() => {})
    },
  }
}

/**
 * Dashboard Layout Slice
 *
 * Component layout operations: add, move, batchUpdatePositions,
 * remove, duplicate. Accesses currentDashboard via get().
 * Uses scheduleSync from DashboardCrudSlice via get().
 */

import type { StateCreator } from 'zustand'
import type {
  DashboardComponent,
  ComponentPosition,
  DataSource,
} from '@/types/dashboard'
import {
  generateId,
  cleanupAgentForComponent,
} from './dashboardHelpers'
import type { DashboardStore } from './dashboardHelpers'

export interface DashboardLayoutSlice {
  addComponent: (component: Omit<DashboardComponent, 'id'>) => void
  moveComponent: (id: string, position: ComponentPosition) => void
  batchUpdatePositions: (positions: Array<{ id: string; position: ComponentPosition }>) => void
  removeComponent: (id: string) => void
  removeComponentsByDevice: (deviceId: string) => void
  duplicateComponent: (id: string) => void
}

export const createDashboardLayoutSlice: StateCreator<
  DashboardStore, [], [], DashboardLayoutSlice
> = (set, get) => {
  function commitDashboard(components: DashboardComponent[], extra?: Record<string, unknown>): void {
    const { currentDashboard, dashboards } = get()
    if (!currentDashboard) return
    const updatedDashboard = { ...currentDashboard, components, updatedAt: Date.now() }
    const updatedDashboards = dashboards.map((d) => d.id === currentDashboard.id ? updatedDashboard : d)
    set({ dashboards: updatedDashboards, currentDashboard: updatedDashboard, ...extra })
    get().scheduleSync(updatedDashboard)
  }

  return {
    addComponent(component) {
      const { currentDashboard } = get()
      if (!currentDashboard) return
      const newComponent = { ...component, id: generateId() } as DashboardComponent
      commitDashboard([...currentDashboard.components, newComponent])
    },

    moveComponent(id, position) {
      const { currentDashboard } = get()
      if (!currentDashboard) return
      if (!currentDashboard.components.some((c) => c.id === id)) return

      // Validate position values: clamp negative coords and enforce minimum dimensions
      const validated = { ...position }
      if (validated.x !== undefined && validated.x < 0) validated.x = 0
      if (validated.y !== undefined && validated.y < 0) validated.y = 0
      if (validated.w !== undefined && validated.w < 1) validated.w = 1
      if (validated.h !== undefined && validated.h < 1) validated.h = 1

      commitDashboard(currentDashboard.components.map((c) =>
        c.id === id ? { ...c, position: { ...c.position, ...validated } } : c,
      ))
    },

    batchUpdatePositions(positions) {
      const { currentDashboard } = get()
      if (!currentDashboard || positions.length === 0) return
      const posMap = new Map(positions.map((p) => [p.id, p.position]))
      let changed = false
      const updatedComponents = currentDashboard.components.map((c) => {
        const newPos = posMap.get(c.id)
        if (newPos) { changed = true; return { ...c, position: newPos } }
        return c
      })
      if (changed) commitDashboard(updatedComponents)
    },

    removeComponent(id) {
      const { currentDashboard, selectedComponent, configComponentId } = get()
      if (!currentDashboard) return
      const removed = currentDashboard.components.find((c) => c.id === id)
      cleanupAgentForComponent(removed)
      // Generic resource cleanup: components may store resource IDs in config
      if (removed) {
        const compConfig = (removed as any).config as Record<string, unknown> | undefined
        if (compConfig?._transformId && typeof compConfig._transformId === 'string') {
          const neomind = (window as any).neomind
          if (neomind?.deleteTransform) {
            neomind.deleteTransform(compConfig._transformId).catch(() => {})
          }
        }
      }
      commitDashboard(
        currentDashboard.components.filter((c) => c.id !== id),
        {
          selectedComponent: selectedComponent === id ? null : selectedComponent,
          configComponentId: configComponentId === id ? null : configComponentId,
        },
      )
    },

    removeComponentsByDevice(deviceId) {
      const { currentDashboard, selectedComponent, configComponentId } = get()
      if (!currentDashboard) return
      const matches = (comp: DashboardComponent): boolean => {
        const ds = comp.dataSource
        if (!ds) return false
        const sources: DataSource[] = Array.isArray(ds) ? ds : [ds]
        return sources.some((s) => s.sourceId === deviceId || (s.type === 'device' && s.property && s.sourceId === deviceId))
      }
      const toKeep: DashboardComponent[] = []
      let removedSelected = false
      let removedConfig = false
      for (const comp of currentDashboard.components) {
        if (matches(comp)) {
          cleanupAgentForComponent(comp)
          if (selectedComponent === comp.id) removedSelected = true
          if (configComponentId === comp.id) removedConfig = true
        } else {
          toKeep.push(comp)
        }
      }
      if (toKeep.length === currentDashboard.components.length) return
      commitDashboard(toKeep, {
        selectedComponent: removedSelected ? null : selectedComponent,
        configComponentId: removedConfig ? null : configComponentId,
      })
    },

    duplicateComponent(id) {
      const { currentDashboard } = get()
      if (!currentDashboard) return
      const original = currentDashboard.components.find((c) => c.id === id)
      if (!original) return
      const newComponent = {
        ...structuredClone(original),
        id: generateId(),
        position: { ...original.position, y: original.position.y + original.position.h },
      } as DashboardComponent
      commitDashboard([...currentDashboard.components, newComponent])
    },
  }
}

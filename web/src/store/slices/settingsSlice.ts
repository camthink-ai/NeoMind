/**
 * Settings Slice
 *
 * Handles general system settings (config import/export, etc.).
 * LLM/MQTT/Device settings are now managed via the Plugin system.
 */

import type { StateCreator } from 'zustand'
import type { SettingsState, SettingsSection } from '../types'
import { api } from '@/lib/api'
import { logError } from '@/lib/errors'

export interface SettingsSlice extends SettingsState {
  // Dialog actions
  openSettings: (section?: SettingsSection) => void
  closeSettings: () => void

  // System Config actions
  exportConfig: () => Promise<{ config: Record<string, unknown> }>
  importConfig: (config: Record<string, unknown>, merge?: boolean) => Promise<{ imported: number; skipped?: number; errors?: Array<{ error: string }> }>
  validateConfig: (config: Record<string, unknown>) => Promise<{ valid: boolean; errors?: string[] }>
}

export const createSettingsSlice: StateCreator<
  SettingsSlice,
  [],
  [],
  SettingsSlice
> = (set) => ({
  // Initial state
  settingsDialogOpen: false,
  settingsSection: "preferences",

  // Dialog actions — openSettings(section?) opens the settings dialog on the
  // given section (falls back to current/last). Any page can call it; the
  // dialog is mounted once at the app root.
  openSettings: (section) =>
    set((state) => ({
      settingsDialogOpen: true,
      settingsSection: section ?? state.settingsSection,
    })),
  closeSettings: () => set({ settingsDialogOpen: false }),

  // System Config - Export
  exportConfig: async () => {
    try {
      const result = await api.exportConfig()
      return result
    } catch (error) {
      logError(error, { operation: 'Export config' })
      throw error
    }
  },

  // System Config - Import
  importConfig: async (config, merge = false) => {
    try {
      const result = await api.importConfig(config, merge)
      return result
    } catch (error) {
      logError(error, { operation: 'Import config' })
      throw error
    }
  },

  // System Config - Validate
  validateConfig: async (config) => {
    try {
      const result = await api.validateConfig(config)
      return result
    } catch (error) {
      logError(error, { operation: 'Validate config' })
      return { valid: false, errors: ['验证失败'] }
    }
  },
})

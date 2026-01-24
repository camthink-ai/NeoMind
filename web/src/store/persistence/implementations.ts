/**
 * Persistence Layer - Storage Implementations
 *
 * Concrete implementations of DashboardStorage for different backends.
 */

import type {
  DashboardStorage,
  StorageResult,
  DashboardDTO,
  CreateDashboardDTO,
  UpdateDashboardDTO,
} from './types'
import type { Dashboard } from '@/types/dashboard'
import {
  toDashboardDTO,
  fromDashboardDTO,
  toCreateDashboardDTO,
  toUpdateDashboardDTO,
} from './types'

// ============================================================================
// LocalStorage Storage
// ============================================================================

const LOCAL_STORAGE_KEY = 'neotalk_dashboards'
const CURRENT_DASHBOARD_KEY = 'neotalk_current_dashboard_id'

export class LocalStorageDashboardStorage implements DashboardStorage {
  private storageKey: string

  constructor(storageKey: string = LOCAL_STORAGE_KEY) {
    this.storageKey = storageKey
  }

  async load(): Promise<StorageResult<Dashboard[]>> {
    try {
      const stored = localStorage.getItem(this.storageKey)
      if (!stored) {
        return { data: [], error: null, source: 'local' }
      }

      const dashboards = JSON.parse(stored) as Dashboard[]
      return { data: dashboards, error: null, source: 'local' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to load from localStorage'),
        source: 'local',
      }
    }
  }

  async save(dashboards: Dashboard[]): Promise<StorageResult<void>> {
    try {
      localStorage.setItem(this.storageKey, JSON.stringify(dashboards))
      return { data: undefined, error: null, source: 'local' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to save to localStorage'),
        source: 'local',
      }
    }
  }

  async sync(dashboard: Dashboard): Promise<StorageResult<Dashboard>> {
    try {
      // Load existing, update, and save back
      const result = await this.load()
      const dashboards = result.data || []

      const index = dashboards.findIndex(d => d.id === dashboard.id)
      if (index >= 0) {
        dashboards[index] = dashboard
      } else {
        dashboards.push(dashboard)
      }

      await this.save(dashboards)
      return { data: dashboard, error: null, source: 'local' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to sync to localStorage'),
        source: 'local',
      }
    }
  }

  async delete(id: string): Promise<StorageResult<void>> {
    try {
      const result = await this.load()
      const dashboards = (result.data || []).filter(d => d.id !== id)
      await this.save(dashboards)
      return { data: undefined, error: null, source: 'local' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to delete from localStorage'),
        source: 'local',
      }
    }
  }

  isAvailable(): boolean {
    try {
      localStorage.setItem('test', 'test')
      localStorage.removeItem('test')
      return true
    } catch {
      return false
    }
  }

  getType(): string {
    return 'local'
  }

  // Current dashboard helpers
  getCurrentDashboardId(): string | null {
    return localStorage.getItem(CURRENT_DASHBOARD_KEY)
  }

  setCurrentDashboardId(id: string | null): void {
    if (id) {
      localStorage.setItem(CURRENT_DASHBOARD_KEY, id)
    } else {
      localStorage.removeItem(CURRENT_DASHBOARD_KEY)
    }
  }

  clear(): void {
    localStorage.removeItem(this.storageKey)
    localStorage.removeItem(CURRENT_DASHBOARD_KEY)
  }
}

// ============================================================================
// API Storage
// ============================================================================

export class ApiDashboardStorage implements DashboardStorage {
  private api: any

  constructor() {
    // Import api module dynamically to avoid circular deps
    this.api = null
  }

  private async getApi() {
    if (!this.api) {
      const module = await import('@/lib/api')
      this.api = module.api
    }
    return this.api
  }

  async load(): Promise<StorageResult<Dashboard[]>> {
    try {
      const api = await this.getApi()
      const response = await api.getDashboards()

      // Handle different response formats
      const dashboards = 'dashboards' in response
        ? (response as { dashboards: DashboardDTO[] }).dashboards.map(fromDashboardDTO)
        : Array.isArray(response)
          ? response.map(fromDashboardDTO)
          : []

      return { data: dashboards, error: null, source: 'api' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to load from API'),
        source: 'api',
      }
    }
  }

  async save(dashboards: Dashboard[]): Promise<StorageResult<void>> {
    // API doesn't support bulk save, so we sync each one
    // This is a no-op for API storage - use sync() instead
    return { data: undefined, error: null, source: 'api' }
  }

  async sync(dashboard: Dashboard): Promise<StorageResult<Dashboard>> {
    try {
      const api = await this.getApi()

      // Check if dashboard exists
      const existing = await api.getDashboard(dashboard.id).catch(() => null)

      if (existing) {
        // Update existing
        const updateDto = toUpdateDashboardDTO(dashboard)
        await api.updateDashboard(dashboard.id, updateDto)
      } else {
        // Create new
        const createDto = toCreateDashboardDTO(dashboard)
        await api.createDashboard(createDto)
      }

      return { data: dashboard, error: null, source: 'api' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to sync to API'),
        source: 'api',
      }
    }
  }

  async delete(id: string): Promise<StorageResult<void>> {
    try {
      const api = await this.getApi()
      await api.deleteDashboard(id)
      return { data: undefined, error: null, source: 'api' }
    } catch (error) {
      return {
        data: null,
        error: error instanceof Error ? error : new Error('Failed to delete from API'),
        source: 'api',
      }
    }
  }

  isAvailable(): boolean {
    // API is always considered available if we have network
    // Errors will be caught during operations
    return typeof window !== 'undefined' && navigator.onLine
  }

  getType(): string {
    return 'api'
  }

  clear(): void {
    // API storage doesn't have local data to clear
    // This is a no-op for API-only storage
  }
}

// ============================================================================
// Hybrid Storage (API with localStorage fallback)
// ============================================================================

export class HybridDashboardStorage implements DashboardStorage {
  private apiStorage: ApiDashboardStorage
  private localStorage: LocalStorageDashboardStorage
  private cacheEnabled: boolean

  constructor(options: { cacheEnabled?: boolean } = {}) {
    this.apiStorage = new ApiDashboardStorage()
    this.localStorage = new LocalStorageDashboardStorage()
    this.cacheEnabled = options.cacheEnabled ?? true
  }

  async load(): Promise<StorageResult<Dashboard[]>> {
    // Try API first, fall back to localStorage
    const apiResult = await this.apiStorage.load()

    if (apiResult.error || !apiResult.data) {
      console.warn('[HybridStorage] API load failed, falling back to localStorage:', apiResult.error?.message)
      return this.localStorage.load()
    }

    // Cache to localStorage if enabled
    if (this.cacheEnabled && apiResult.data) {
      this.localStorage.save(apiResult.data).catch(() => {
        // Ignore cache save errors
      })
    }

    return apiResult
  }

  async save(dashboards: Dashboard[]): Promise<StorageResult<void>> {
    // Always save to localStorage immediately for responsiveness
    const localResult = await this.localStorage.save(dashboards)

    // Try to sync to API in background
    this.syncToApi(dashboards).catch(() => {
      // API sync failed, but local save succeeded
      console.warn('[HybridStorage] Background API sync failed')
    })

    return localResult
  }

  async sync(dashboard: Dashboard): Promise<StorageResult<Dashboard>> {
    // Sync to localStorage first (fast, reliable)
    const localResult = await this.localStorage.sync(dashboard)

    // Try to sync to API in background
    this.apiStorage.sync(dashboard).catch(() => {
      // API sync failed
      console.warn('[HybridStorage] API sync failed for dashboard:', dashboard.id)
    })

    return localResult
  }

  async delete(id: string): Promise<StorageResult<void>> {
    // Delete from localStorage first
    const localResult = await this.localStorage.delete(id)

    // Try to delete from API in background
    this.apiStorage.delete(id).catch(() => {
      console.warn('[HybridStorage] API delete failed for dashboard:', id)
    })

    return localResult
  }

  isAvailable(): boolean {
    return this.localStorage.isAvailable() || this.apiStorage.isAvailable()
  }

  getType(): string {
    return 'hybrid'
  }

  // Helper to sync all dashboards to API
  private async syncToApi(dashboards: Dashboard[]): Promise<void> {
    for (const dashboard of dashboards) {
      await this.apiStorage.sync(dashboard)
    }
  }

  // Expose current dashboard helpers from localStorage
  getCurrentDashboardId(): string | null {
    return this.localStorage.getCurrentDashboardId()
  }

  setCurrentDashboardId(id: string | null): void {
    this.localStorage.setCurrentDashboardId(id)
  }

  clear(): void {
    this.localStorage.clear()
  }
}

// ============================================================================
// Factory
// ============================================================================

export interface CreateStorageOptions {
  type?: 'local' | 'api' | 'hybrid'
  cacheEnabled?: boolean
}

export function createDashboardStorage(options: CreateStorageOptions = {}): DashboardStorage {
  const { type = 'hybrid', cacheEnabled = true } = options

  switch (type) {
    case 'local':
      return new LocalStorageDashboardStorage()
    case 'api':
      return new ApiDashboardStorage()
    case 'hybrid':
      return new HybridDashboardStorage({ cacheEnabled })
    default:
      return new HybridDashboardStorage({ cacheEnabled })
  }
}

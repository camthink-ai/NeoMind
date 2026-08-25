/**
 * Instance Slice
 *
 * Manages remote NeoMind backend instances with CRUD operations,
 * health testing, and runtime instance switching.
 *
 * Switching strategy: full-screen overlay + page reload.
 * When the user selects a different instance we persist the target
 * config to localStorage and reload the page.  This eliminates all
 * race-conditions from in-flight requests / stale tokens.
 */

import type { StateCreator } from 'zustand'
import { api, getApiBase, setApiBase, setApiKey, clearApiKey } from '@/lib/api'
import { tokenManager } from '@/lib/auth'
import { logError } from '@/lib/errors'
import { INSTANCE_CACHE_KEY, CURRENT_INSTANCE_KEY, PENDING_SWITCH_KEY } from '@/lib/instance-constants'
import { fetchCache } from '@/lib/utils/async'

// ============================================================================
// API key decryption — LEGACY CACHE MIGRATION ONLY
// ============================================================================
// The backend no longer returns the full key (a hardcoded-XOR "encryption"
// round-trip leaked every instance's credential to anyone who could list
// instances — the cipher lives in the open-source repo). This decoder remains
// solely to migrate keys still sitting in an old localStorage instance cache
// into the per-browser key store below; once migrated, entries are stripped.

const LEGACY_KEY_CIPHER = 'NeoMind2024!@#'

/** Decrypt XOR+hex encoded API key from a pre-0.9.20 backend. */
function decryptLegacyApiKey(encrypted: string): string {
  const keyBytes = new TextEncoder().encode(LEGACY_KEY_CIPHER)
  const bytes: number[] = []
  for (let i = 0; i < encrypted.length; i += 2) {
    bytes.push(parseInt(encrypted.substring(i, i + 2), 16))
  }
  return bytes
    .map((b, i) => String.fromCharCode(b ^ keyBytes[i % keyBytes.length]))
    .join('')
}

// ============================================================================
// Per-browser instance key store
// ============================================================================
// The backend keeps the full key for its own health checks but never returns
// it. The browser keeps its copy here from the moment the user entered it
// (add/edit instance) so later switches don't need the key back from the API.

const INSTANCE_KEYS_STORAGE = 'neomind_instance_keys'

function readKeyStore(): Record<string, string> {
  try {
    const raw = localStorage.getItem(INSTANCE_KEYS_STORAGE)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function writeKeyStore(map: Record<string, string>) {
  try {
    localStorage.setItem(INSTANCE_KEYS_STORAGE, JSON.stringify(map))
  } catch { /* ignore storage errors */ }
}

/** Save full API key for an instance (persists across reloads). */
function saveInstanceKey(instanceId: string, apiKey: string) {
  const map = readKeyStore()
  map[instanceId] = apiKey
  writeKeyStore(map)
}

/** Remove API key for an instance. */
function removeInstanceKey(instanceId: string) {
  const map = readKeyStore()
  delete map[instanceId]
  writeKeyStore(map)
}

/** Get the full API key for an instance, if this browser ever stored one. */
export function getFullApiKey(instanceId: string): string | undefined {
  return readKeyStore()[instanceId]
}

// ============================================================================
// Types
// ============================================================================

export interface InstanceInfo {
  id: string
  name: string
  url: string
  /** Masked key from backend (e.g. "nmk_abc1****"). The full key is never returned. */
  api_key?: string
  /**
   * @deprecated Removed from backend responses in 0.9.20. Present only in
   * old localStorage caches; `getCachedInstances` migrates these into the
   * key store and strips them. Do not use in new code.
   */
  encrypted_key?: string
  is_local: boolean
  last_status: string
  last_checked_at: number | null
  created_at: number
}

export interface InstanceTestResult {
  success: boolean
  latency_ms: number
  status: string
  error?: string
}

export type SwitchingState = 'idle' | 'switching' | 'success' | 'error'

export interface InstanceSlice {
  // State
  instances: InstanceInfo[]
  currentInstanceId: string
  instanceLoading: boolean
  switchingState: SwitchingState
  switchingError: string | null
  previousInstanceId: string | null

  // CRUD
  fetchInstances: () => Promise<void>
  addInstance: (data: { name: string; url: string; api_key?: string }) => Promise<string>
  updateInstance: (id: string, data: { name?: string; url?: string; api_key?: string }) => Promise<void>
  deleteInstance: (id: string) => Promise<void>

  // Testing
  testInstance: (id: string) => Promise<InstanceTestResult>

  // Switching
  switchInstance: (id: string) => Promise<void>
  revertSwitch: () => void
  clearSwitchingError: () => void

  // Helpers
  getCurrentInstance: () => InstanceInfo | null
  isRemoteInstance: () => boolean
}

// ============================================================================
// API Helpers
// ============================================================================

async function fetchInstancesApi(): Promise<InstanceInfo[]> {
  const data = await api.get<{ instances: InstanceInfo[] }>('/instances')
  return data.instances || []
}

async function createInstanceApi(data: { name: string; url: string; api_key?: string }): Promise<InstanceInfo> {
  return api.post<InstanceInfo>('/instances', data)
}

async function updateInstanceApi(id: string, data: { name?: string; url?: string; api_key?: string }): Promise<InstanceInfo> {
  return api.put<InstanceInfo>(`/instances/${id}`, data)
}

async function deleteInstanceApi(id: string): Promise<void> {
  await api.delete(`/instances/${id}`)
}

async function testInstanceApi(id: string): Promise<InstanceTestResult> {
  return api.post<InstanceTestResult>(`/instances/${id}/test`, {})
}

/** Read cached instance list from localStorage (available before any API call).
 * One-time migration: entries still carrying a legacy `encrypted_key` (written
 * by a pre-0.9.20 backend) are decrypted into the per-browser key store and
 * stripped from the cache. */
function getCachedInstances(): InstanceInfo[] {
  try {
    const raw = localStorage.getItem(INSTANCE_CACHE_KEY)
    if (!raw) return []
    const instances: InstanceInfo[] = JSON.parse(raw)
    let migrated = false
    for (const inst of instances) {
      if (inst.encrypted_key) {
        try {
          saveInstanceKey(inst.id, decryptLegacyApiKey(inst.encrypted_key))
        } catch { /* undecodable legacy value — drop it */ }
        delete inst.encrypted_key
        migrated = true
      }
    }
    if (migrated) {
      try {
        localStorage.setItem(INSTANCE_CACHE_KEY, JSON.stringify(instances))
      } catch { /* ignore storage errors */ }
    }
    return instances
  } catch {
    return []
  }
}

/** Sync instance list to localStorage cache (strips API keys for security). */
function syncCache(instances: InstanceInfo[]) {
  try {
    // Strip masked api_key (display-only) and any legacy encrypted_key
    // before caching
    const safe = instances.map(({ api_key: _, encrypted_key: __, ...rest }) => rest)
    localStorage.setItem(INSTANCE_CACHE_KEY, JSON.stringify(safe))
  } catch { /* ignore storage errors */ }
}

// ============================================================================
// Boot-time: apply pending switch BEFORE anything else
// ============================================================================

interface PendingSwitch {
  targetId: string
  previousId: string
  apiUrl: string
  apiKey: string
}

/**
 * Apply a pending instance switch synchronously at module-load time.
 * This runs BEFORE React renders anything, so the API base is already
 * correct when components mount and ProtectedRoute checks auth.
 */
function applyPendingSwitch(): PendingSwitch | null {
  let pending: PendingSwitch | null = null
  try {
    const raw = localStorage.getItem(PENDING_SWITCH_KEY)
    pending = raw ? JSON.parse(raw) : null
  } catch { /* ignore */ }

  if (!pending) {
    // No pending switch — reset to local only on truly fresh start.
    // Keep remote if there's a JWT token (user logged in on remote) or
    // an API key in sessionStorage (page refresh preserved it).
    try {
      const currentId = localStorage.getItem(CURRENT_INSTANCE_KEY)
      if (currentId && currentId !== 'local-default') {
        const hasJwt = !!localStorage.getItem('neomind_token')
        const activeKey = sessionStorage.getItem('neomind_api_key')
        if (!hasJwt && !activeKey) {
          // Fresh start — reset to local instance
          localStorage.setItem(CURRENT_INSTANCE_KEY, 'local-default')
          setApiBase('')
          clearApiKey()
        } else if (!activeKey) {
          // Has JWT but no API key — restore API base from cached instances
          const cached = getCachedInstances()
          const inst = cached.find(i => i.id === currentId)
          if (inst && !inst.is_local) {
            setApiBase(`${inst.url}/api`)
          }
        }
      }
    } catch { /* ignore */ }
    return null
  }

  // Apply API base
  if (pending.apiUrl) {
    setApiBase(pending.apiUrl)
  } else {
    setApiBase('')
  }

  // Always clear JWT on instance switch — a token from one instance
  // is never valid on another (different signing keys / user databases).
  tokenManager.clearToken()

  // Apply API key if provided (for remote instances with key-based auth).
  // When set, ProtectedRoute allows access without JWT login.
  if (pending.apiKey) {
    setApiKey(pending.apiKey)
    // Restore to in-memory map so subsequent switchInstance calls can find it
    saveInstanceKey(pending.targetId, pending.apiKey)
  } else {
    clearApiKey()
  }

  // DON'T clear the flag yet — the overlay needs it to show the spinner
  return pending
}

// Execute synchronously at import time (before React)
const _pendingSwitch = applyPendingSwitch()

// ============================================================================
// Slice
// ============================================================================

export const createInstanceSlice: StateCreator<
  InstanceSlice,
  [],
  [],
  InstanceSlice
> = (set, get) => ({
  // Initial state
  instances: getCachedInstances(),
  currentInstanceId: (() => {
    try {
      return localStorage.getItem(CURRENT_INSTANCE_KEY) || 'local-default'
    } catch {
      return 'local-default'
    }
  })(),
  instanceLoading: false,
  switchingState: _pendingSwitch ? 'switching' : 'idle',
  switchingError: null,
  previousInstanceId: _pendingSwitch?.previousId ?? null,

  // Fetch all instances — only works when connected to local instance
  fetchInstances: async () => {
    if (!fetchCache.shouldFetch('instances')) return
    fetchCache.markFetching('instances')
    const { isRemoteInstance } = get()
    // On remote instance, skip API call — use cached list
    if (isRemoteInstance()) {
      const cached = getCachedInstances()
      if (cached.length > 0) {
        set({ instances: cached, instanceLoading: false })
      }
      fetchCache.markFetched('instances')
      return
    }

    set({ instanceLoading: true })
    try {
      const instances = await fetchInstancesApi()
      // The backend returns masked keys only; full keys live in the
      // per-browser key store (saved when the user entered them).
      try {
        localStorage.setItem(INSTANCE_CACHE_KEY, JSON.stringify(instances))
      } catch { /* ignore storage errors */ }

      // Self-heal: if currentInstanceId points to a non-existent instance,
      // reset to local-default (stale state from a previous failed switch)
      const { currentInstanceId } = get()
      if (currentInstanceId !== 'local-default' &&
          !instances.some((i) => i.id === currentInstanceId)) {
        localStorage.setItem(CURRENT_INSTANCE_KEY, 'local-default')
        set({ instances, currentInstanceId: 'local-default', instanceLoading: false })
      } else {
        set({ instances, instanceLoading: false })
      }
      fetchCache.markFetched('instances')
    } catch (err) {
      console.error('[fetchInstances] failed:', err)
      logError(err, { operation: 'fetchInstances' })
      fetchCache.invalidate('instances')
      set({ instanceLoading: false })
    }
  },

  // Add a new instance
  addInstance: async (data) => {
    fetchCache.invalidate('instances')
    const instance = await createInstanceApi(data)
    // The user just entered the key — persist it in the per-browser store so
    // later switches don't need the backend to hand it back.
    if (data.api_key) {
      saveInstanceKey(instance.id, data.api_key)
    }
    const instances = [...get().instances, instance]
    set({ instances })
    syncCache(instances)
    return instance.id
  },

  // Update an existing instance
  updateInstance: async (id, data) => {
    fetchCache.invalidate('instances')
    const updated = await updateInstanceApi(id, data)
    if (data.api_key !== undefined) {
      if (data.api_key) {
        saveInstanceKey(id, data.api_key)
      } else {
        removeInstanceKey(id)
      }
    }
    const instances = get().instances.map((i) => (i.id === id ? updated : i))
    set({ instances })
    syncCache(instances)
  },

  // Delete an instance
  deleteInstance: async (id) => {
    fetchCache.invalidate('instances')
    await deleteInstanceApi(id)
    const instances = get().instances.filter((i) => i.id !== id)
    set({ instances })
    syncCache(instances)
  },

  // Test instance connectivity
  testInstance: async (id) => {
    return testInstanceApi(id)
  },

  // Switch to a different instance — validate first, then overlay + reload
  switchInstance: async (id) => {
    const { instances, currentInstanceId, switchingState } = get()
    if (id === currentInstanceId) return
    if (switchingState === 'switching') return

    // Look up target in current instances, fall back to localStorage cache
    let targetInstance = instances.find((i) => i.id === id)
    if (!targetInstance) {
      const cached = getCachedInstances()
      targetInstance = cached.find((i) => i.id === id)
    }
    if (!targetInstance) {
      set({ switchingState: 'error', switchingError: 'Instance not found' })
      return
    }

    // Resolve full API key: prefer in-memory store (backend returns masked key)
    const fullApiKey = !targetInstance.is_local
      ? (getFullApiKey(id) || '') // from add/edit flow
      : ''

    // Show the switch overlay immediately — on every page, including login —
    // so the user sees "Connecting to {name}..." during the validation below.
    // (Local + remote-no-apikey paths previously didn't set 'switching' until
    // after validation, and reload fired immediately after, so the overlay
    // never painted — the login page looked frozen.)
    set({ switchingState: 'switching', switchingError: null })

    // For remote instances with API key: validate the key before switching
    if (!targetInstance.is_local && fullApiKey) {
      set({ switchingState: 'switching', switchingError: null })
      try {
        const apiUrl = targetInstance.url.replace(/\/+$/, '') + '/api'
        const res = await fetch(`${apiUrl}/auth/verify`, {
          headers: { 'X-API-Key': fullApiKey },
          signal: AbortSignal.timeout(8000),
        })
        if (res.status === 401) {
          set({
            switchingState: 'error',
            switchingError: 'apiKeyRejected',
          })
          return
        }
        // Any non-401 response means auth passed
      } catch {
        set({
          switchingState: 'error',
          switchingError: 'unreachable',
        })
        return
      }
    } else if (!targetInstance.is_local) {
      // Remote without API key — verify the server is reachable at least
      try {
        const res = await fetch(`${targetInstance.url.replace(/\/+$/, '')}/api/setup/status`, {
          signal: AbortSignal.timeout(8000),
        })
        if (!res.ok) {
          set({
            switchingState: 'error',
            switchingError: `Cannot reach ${targetInstance.url}. Please check the address.`,
          })
          return
        }
      } catch {
        set({
          switchingState: 'error',
          switchingError: 'unreachable',
        })
        return
      }
    } else {
      // Local instance — verify the app's own backend is running BEFORE
      // committing the switch. Without this, a crashed local backend (e.g.
      // the rustls startup panic) sent the user through window.location.reload()
      // and then made them wait the full StartupLoading timeout (was 30s)
      // before failing. Fail fast instead.
      try {
        const localUrl = (targetInstance.url || 'http://localhost:9375').replace(/\/+$/, '')
        const res = await fetch(`${localUrl}/api/setup/status`, {
          signal: AbortSignal.timeout(5000),
        })
        if (!res.ok) {
          set({
            switchingState: 'error',
            switchingError: 'Local backend not running. Please restart the app.',
          })
          return
        }
      } catch {
        set({
          switchingState: 'error',
          switchingError: 'unreachable',
        })
        return
      }
    }

    // Show overlay
    set({ switchingState: 'switching', switchingError: null })

    // Write pending-switch so applyPendingSwitch() picks it up on reload
    localStorage.setItem(CURRENT_INSTANCE_KEY, id)
    localStorage.setItem(PENDING_SWITCH_KEY, JSON.stringify({
      targetId: id,
      previousId: currentInstanceId,
      apiUrl: targetInstance.is_local ? '' : `${targetInstance.url}/api`,
      apiKey: targetInstance.is_local ? '' : fullApiKey,
    }))

    // Reload — the simplest and most reliable way to switch
    window.location.reload()
  },

  // Revert to previous instance (called from overlay after a failed switch)
  revertSwitch: () => {
    let pending: PendingSwitch | null = null
    try {
      const raw = localStorage.getItem(PENDING_SWITCH_KEY)
      pending = raw ? JSON.parse(raw) : null
    } catch { /* ignore */ }

    if (!pending?.previousId) return

    // Find previous instance from store, fall back to cache
    const { instances } = get()
    let prevInstance = instances.find((i) => i.id === pending.previousId)
    if (!prevInstance) {
      const cached = getCachedInstances()
      prevInstance = cached.find((i) => i.id === pending.previousId)
    }
    if (!prevInstance) return

    // Resolve full API key from memory
    const prevApiKey = prevInstance.is_local ? '' : (getFullApiKey(pending.previousId) || '')

    localStorage.setItem(CURRENT_INSTANCE_KEY, pending.previousId)
    localStorage.setItem(PENDING_SWITCH_KEY, JSON.stringify({
      targetId: pending.previousId,
      previousId: pending.targetId,
      apiUrl: prevInstance.is_local ? '' : `${prevInstance.url}/api`,
      apiKey: prevApiKey,
    }))

    window.location.reload()
  },

  // Clear switching error / dismiss the overlay
  clearSwitchingError: () => {
    // Revert currentInstanceId to previous instance so next reload doesn't
    // try to connect to the failed target
    const { previousInstanceId } = get()
    if (previousInstanceId) {
      localStorage.setItem(CURRENT_INSTANCE_KEY, previousInstanceId)
    }
    localStorage.removeItem(PENDING_SWITCH_KEY)
    set({ switchingState: 'idle', switchingError: null, previousInstanceId: null })
  },

  // Get current instance info
  getCurrentInstance: () => {
    const { instances, currentInstanceId } = get()
    return instances.find((i) => i.id === currentInstanceId) || null
  },

  // Check if currently connected to a remote instance
  // Must verify BOTH stored ID AND actual API base — stale localStorage
  // can have a remote ID while we're actually connected locally.
  isRemoteInstance: () => {
    if (get().currentInstanceId === 'local-default') return false
    const apiBase = getApiBase()
    if (!apiBase || apiBase === '/api') return false
    if (apiBase.includes('localhost') || apiBase.includes('127.0.0.1')) return false
    return true
  },
})

/**
 * Called from InstanceSwitchOverlay on mount to clear the pending flag
 * after the app has successfully booted with the new instance.
 */
export function clearPendingSwitch() {
  localStorage.removeItem(PENDING_SWITCH_KEY)
}

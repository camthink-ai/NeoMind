/**
 * Canonical server URL resolution for DISPLAY purposes (webhook URLs, curl examples).
 *
 * Why this exists:
 * In Tauri mode, `getServerOrigin()` returns `http://localhost:9375` — correct for
 * frontend→backend API calls but WRONG to show users (devices can't reach localhost).
 * In browser mode, `window.location.origin` is usually correct (user accesses via
 * domain or LAN IP), EXCEPT when the user accesses via `http://localhost:9375`
 * (e.g. SSH-tunnel to a remote server) — in that case it's also wrong for display.
 *
 * The backend `/api/system/network-info` endpoint returns the device-reachable URL
 * via `resolve_server_url()` which considers NEOMIND_SERVER_URL env, X-Forwarded-Host,
 * and auto-detects LAN IP.
 *
 * Resolution priority for DISPLAY:
 *   1. Backend-resolved URL (from prefetch, runs when env is "localhost-like")
 *   2. window.location.origin (when user is accessing via a non-localhost URL —
 *      that URL is exactly what devices should use too)
 *   3. getServerOrigin() fallback
 *
 * Usage:
 *   - For display (webhook URL, curl example) → use `useServerUrl()` hook
 *   - For frontend→backend calls (fetch, WebSocket) → keep using `getServerOrigin()` / `getApiBase()`
 *   - Call `prefetchServerUrl()` once at app startup to warm the cache
 */

import { useSyncExternalStore } from 'react'
import { api, getServerOrigin, isTauriEnv } from './api'

// ============================================================================
// Helpers
// ============================================================================

/** True if origin's hostname is a loopback address. */
function isLocalhostOrigin(origin: string): boolean {
  try {
    const u = new URL(origin)
    return (
      u.hostname === 'localhost' ||
      u.hostname === '127.0.0.1' ||
      u.hostname === '::1'
    )
  } catch {
    return false
  }
}

/**
 * Decide whether we need to consult the backend for a canonical URL.
 *
 * - Tauri: always (there's no window.location)
 * - Browser via localhost/127.0.0.1: yes — the user's `localhost` is THEIR machine,
 *   not the server's, so it's wrong as a webhook URL for devices
 * - Browser via domain or LAN IP: no — what the user typed is what devices should use
 */
function shouldConsultBackend(): boolean {
  if (isTauriEnv()) return true
  if (typeof window === 'undefined') return false
  return isLocalhostOrigin(window.location.origin)
}

// ============================================================================
// Store: module-level cache + subscriber list (cross-component)
// ============================================================================

let cachedServerUrl: string | null = null
let prefetchStarted = false
const listeners = new Set<() => void>()

/**
 * Compute the synchronous value returned to React.
 *
 * - Browser accessing via domain/LAN IP → use that directly (no fetch needed).
 * - Tauri OR browser-via-localhost → prefer cache, else `getServerOrigin()` until
 *   prefetch resolves.
 */
function getSyncSnapshot(): string {
  // Browser mode with non-localhost origin: that URL is already correct.
  if (!isTauriEnv() && typeof window !== 'undefined') {
    if (!isLocalhostOrigin(window.location.origin)) {
      return window.location.origin
    }
  }
  // Tauri OR browser-on-localhost: prefer the prefetched canonical URL.
  return cachedServerUrl ?? getServerOrigin()
}

function notifyAll() {
  listeners.forEach((l) => l())
}

function subscribe(cb: () => void): () => void {
  listeners.add(cb)
  return () => {
    listeners.delete(cb)
  }
}

// ============================================================================
// Prefetch: call once at app startup (e.g. in App.tsx useEffect)
// ============================================================================

/**
 * Fetch the canonical (device-reachable) server URL from the backend and cache it.
 * Safe to call multiple times — only the first call actually fetches.
 *
 * Skipped when the user is already accessing via a non-localhost URL in a browser
 * (that URL is already what devices should use, so the backend hint is unnecessary).
 */
export async function prefetchServerUrl(): Promise<void> {
  // No need to fetch if window.location.origin is already a real address.
  if (!shouldConsultBackend()) return
  if (prefetchStarted) return
  prefetchStarted = true

  try {
    const resp = await api.get<{
      server_url?: string
      lan_reachable?: boolean
    }>('/system/network-info')
    const url = resp?.server_url
    if (url && /^https?:\/\//.test(url)) {
      cachedServerUrl = url
      // Absent field (older backend) → assume reachable rather than nag.
      cachedLanReachable = resp?.lan_reachable ?? true
      notifyAll()
      return
    }
  } catch {
    // fall through to fallback
  }
  // On failure, cache the fallback so subscribers stop seeing the loading state.
  cachedServerUrl = getServerOrigin()
  notifyAll()
}

/** Alias for call sites that want the resolved URL as a promise. */
export async function fetchServerUrl(): Promise<string> {
  if (!shouldConsultBackend()) {
    return typeof window !== 'undefined' ? window.location.origin : getServerOrigin()
  }
  if (!cachedServerUrl) {
    await prefetchServerUrl()
  }
  return cachedServerUrl ?? getServerOrigin()
}

// ============================================================================
// React hook: useSyncExternalStore gives tear-free reads with live updates
// ============================================================================

/**
 * Returns the canonical server URL for DISPLAY purposes (webhook URLs, curl examples).
 *
 * - Browser accessing via domain/LAN IP → returns `window.location.origin` synchronously.
 * - Tauri or browser-on-localhost → returns cached canonical URL once prefetch resolves;
 *   `getServerOrigin()` (localhost) on first render until then.
 *
 * Call `prefetchServerUrl()` in App.tsx to warm the cache before any webhook
 * display renders.
 */
export function useServerUrl(): string {
  return useSyncExternalStore(
    subscribe,
    getSyncSnapshot,
    getSyncSnapshot, // server snapshot = same as client (no SSR here)
  )
}

// ============================================================================
// LAN reachability — rides the same prefetch/network-info call
// ============================================================================

/**
 * `null` = unknown (not consulted — browser via LAN IP/domain, or prefetch
 * pending/failed). `false` = the server is bound to loopback only: the URL is
 * the LAN address traffic should aim at, but nothing can connect until the
 * server is rebound (NEOMIND_HOST=0.0.0.0).
 */
let cachedLanReachable: boolean | null = null

/**
 * Returns whether LAN devices can reach the server (`null` = unknown).
 *
 * - Browser accessing via non-localhost origin → `true` (that URL works).
 * - Tauri / browser-via-localhost → from the prefetched network-info
 *   (`lan_reachable`); `null` until it resolves.
 */
export function useServerLanReachable(): boolean | null {
  return useSyncExternalStore(
    subscribe,
    () => {
      if (!isTauriEnv() && typeof window !== 'undefined') {
        if (!isLocalhostOrigin(window.location.origin)) return true
      }
      return cachedLanReachable
    },
    () => cachedLanReachable,
  )
}

/** Synchronous read of the cached backend URL, or `null` if not resolved. */
export function getCachedServerUrl(): string | null {
  return cachedServerUrl
}

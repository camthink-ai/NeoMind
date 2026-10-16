/**
 * Hook for loading extension-contributed pages (contribution points).
 *
 * Extensions may declare standalone pages in their manifest
 * (`frontend.pages`) that the core app mounts as routes + nav entries.
 */

import { useEffect, useState, useCallback } from 'react'
import { getApiBase } from '@/lib/api'

export interface ExtensionPage {
  extension_id: string
  type: string
  title: string
  description: string
  /** Route path inside the main app, e.g. "/devices/wifi-onboarding" */
  route: string
  /** Nav placement: "main" appends at the end, a section id (e.g. "devices")
   *  inserts after that item; null = route only, no nav entry */
  nav_section: string | null
  nav_label: string | null
  /** Lucide icon name (e.g. "wifi"); host maps unknown names to Puzzle */
  icon: string | null
  bundle_url: string
  export_name: string
  global_name: string | null
}

export interface ExtensionPagesResult {
  loading: boolean
  pages: ExtensionPage[]
  refresh: () => Promise<void>
}

const API_BASE = getApiBase()

export function useExtensionPages(options?: {
  autoSync?: boolean
  syncInterval?: number
}): ExtensionPagesResult {
  const { autoSync = true, syncInterval = 30000 } = options ?? {}
  const [pages, setPages] = useState<ExtensionPage[]>([])
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    try {
      const res = await fetch(`${API_BASE}/extensions/pages`)
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const body = await res.json()
      const list: ExtensionPage[] = Array.isArray(body?.data)
        ? body.data
        : Array.isArray(body)
          ? body
          : []
      setPages(list)
    } catch {
      // Keep the previous list on transient errors — clearing nav on a
      // momentary API failure is worse than showing stale entries.
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  // Periodic sync keeps nav/dialog tabs in step with installs/uninstalls.
  useEffect(() => {
    if (!autoSync) return
    const timer = setInterval(() => void refresh(), syncInterval)
    const onVisible = () => {
      if (document.visibilityState === 'visible') void refresh()
    }
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(timer)
      document.removeEventListener('visibilitychange', onVisible)
    }
  }, [autoSync, syncInterval, refresh])

  return { loading, pages, refresh }
}

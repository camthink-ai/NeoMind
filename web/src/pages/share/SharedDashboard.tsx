/**
 * Shared Dashboard Page
 *
 * Public page for viewing shared dashboards without authentication.
 * Intercepts ALL fetch calls at the window level to route through
 * the backend share proxy — works for fetchAPI, direct fetch, and
 * extension UMD bundle fetch calls alike.
 */

import { useEffect, useState, useMemo, useRef, useCallback, Component } from 'react'
import type { ReactNode, ErrorInfo } from 'react'
import { useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import i18n from '@/i18n/config'
import { fetchAPI } from '@/lib/api'
import { Loader2, AlertTriangle, Eye, Zap, EyeOff } from 'lucide-react'
import { LoadingState } from '@/components/shared/LoadingState'
import { ThemeToggle } from '@/components/layout/ThemeToggle'
import { DashboardGrid } from '@/components/dashboard/DashboardGrid'
import { renderDashboardComponent } from '@/pages/dashboard-components/Renderers'
import { fromDashboardDTO } from '@/store/persistence/types'
import { communityRegistry } from '@/components/dashboard/registry/CommunityRegistry'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import { useStore } from '@/store'
import type { Dashboard } from '@/types/dashboard'
import type { FrontendComponentMeta } from '@/types/frontend-component'
import { textNano } from '@/design-system/tokens/typography'

// ============================================================================
// Error boundary for graceful degradation of unsupported components
// ============================================================================

interface ErrorBoundaryProps {
  children: ReactNode
  fallback?: ReactNode
}

interface ErrorBoundaryState {
  hasError: boolean
}

class ComponentErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { hasError: false }

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { hasError: true }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.warn('[SharedDashboard] Component failed to render:', error.message)
  }

  render() {
    if (this.state.hasError) {
      return this.props.fallback || (
        <div className="flex flex-col items-center justify-center h-full min-h-[120px] p-4 text-center">
          <EyeOff className="h-5 w-5 text-muted-foreground mb-2" />
          <p className="text-xs text-muted-foreground">{i18n.t('dashboardComponents:sharedDashboard.componentUnavailable')}</p>
        </div>
      )
    }
    return this.props.children
  }
}

// ============================================================================
// Types
// ============================================================================

interface SharedDashboardData {
  dashboard: Dashboard
  permissions: {
    allow_interactive: boolean
  }
  expires_at: number | null
}

// ============================================================================
// Window fetch interception for share proxy
// ============================================================================

/**
 * Install a global fetch interceptor that rewrites all /api/... URLs
 * to /api/share/:token/proxy/... — catches fetchAPI, direct fetch,
 * and extension UMD bundle calls.
 *
 * Returns a cleanup function to restore the original fetch.
 */
function installShareProxy(token: string): () => void {
  const originalFetch = window.fetch
  const proxyPrefix = `/api/share/${token}/proxy/`

  window.fetch = function (input: RequestInfo | URL, init?: RequestInit) {
    let url: string

    if (typeof input === 'string') {
      url = input
    } else if (input instanceof URL) {
      url = input.toString()
    } else if (input instanceof Request) {
      url = input.url
    } else {
      return originalFetch.call(this, input, init)
    }

    // Only rewrite relative URLs starting with /api/ — never rewrite absolute
    // URLs (http://...) or URLs that merely contain /api/ in their path.
    // This prevents accidental interception of third-party API calls.
    if (url.startsWith('/api/')) {
      const afterApi = url.slice(5) // skip '/api/'
      // Don't double-rewrite share proxy paths or the share data endpoint
      if (!afterApi.startsWith('share/')) {
        const newUrl = proxyPrefix + afterApi
        return originalFetch.call(this, newUrl, init)
      }
    }

    return originalFetch.call(this, input, init)
  }

  return () => {
    window.fetch = originalFetch
  }
}

// ============================================================================
// Shared Dashboard Component
// ============================================================================

export function SharedDashboard() {
  const { token } = useParams<{ token: string }>()
  const { t } = useTranslation('dashboardComponents')
  const [data, setData] = useState<SharedDashboardData | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const proxyCleanupRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    if (!token) {
      setError(t('sharedDashboard.noShareToken'))
      setLoading(false)
      return
    }

    let mounted = true

    // Load dashboard data using direct fetch (no proxy needed for /share/:token)
    const init = async () => {
      try {
        const result = await fetchAPI<SharedDashboardData>(`/share/${token}`, {
          skipAuth: true,
          skipGlobalError: true,
          skipErrorToast: true,
        })
        if (mounted) {
          setData({
            dashboard: convertShareResponse(result),
            permissions: result.permissions,
            expires_at: result.expires_at,
          })
          // Install global fetch proxy AFTER dashboard data is loaded
          proxyCleanupRef.current = installShareProxy(token)
        }
      } catch (e: any) {
        if (mounted) {
          const msg = e.message || ''
          if (msg.includes('doctype') || msg.includes('Unexpected token')) {
            setError(t('sharedDashboard.serverError'))
          } else {
            setError(msg || t('sharedDashboard.failedToLoad'))
          }
        }
      } finally {
        if (mounted) setLoading(false)
      }
    }

    init()
    return () => {
      mounted = false
      proxyCleanupRef.current?.()
      proxyCleanupRef.current = null
    }
  }, [token])

  // Loading state
  if (loading) {
    return (
      <div className="h-screen flex items-center justify-center bg-background">
        <LoadingState size="lg" text={t('sharedDashboard.loading')} />
      </div>
    )
  }

  // Error state
  if (error || !data) {
    return (
      <div className="h-screen flex items-center justify-center bg-background">
        <div className="flex flex-col items-center gap-3 text-center max-w-md px-4">
          <AlertTriangle className="h-10 w-10 text-warning" />
          <h2 className="text-lg font-semibold">{error || t('sharedDashboard.dashboardNotFound')}</h2>
          <p className="text-sm text-muted-foreground">
            {error?.includes('expired') ? t('sharedDashboard.shareLinkExpired') : t('sharedDashboard.couldNotLoad')}
          </p>
        </div>
      </div>
    )
  }

  const isInteractive = data.permissions.allow_interactive

  return (
    <div className="flex h-screen flex-col bg-background">
      {/* Header */}
      <header className="shrink-0 flex items-center justify-between px-4 py-2.5  bg-background z-10">
        <div className="flex items-center gap-2">
          <h1 className="text-sm font-semibold">{data.dashboard.name}</h1>
          <span className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded ${textNano} font-medium bg-muted text-muted-foreground`}>
            {isInteractive ? (
              <><Zap className="h-3 w-3" /> {t('sharedDashboard.interactive')}</>
            ) : (
              <><Eye className="h-3 w-3" /> {t('sharedDashboard.readOnly')}</>
            )}
          </span>
        </div>
        <div className="flex items-center gap-1">
          <ThemeToggle />
          <span className="text-xs text-muted-foreground">{t('sharedDashboard.poweredBy')}</span>
        </div>
      </header>

      {/* Dashboard Content - reuse same rendering pipeline */}
      <div className="flex-1 overflow-auto p-4 relative">
        {data.dashboard.components && data.dashboard.components.length > 0 ? (
          <SharedDashboardContent dashboard={data.dashboard} />
        ) : (
          <div className="flex items-center justify-center h-64 text-muted-foreground text-sm">
            {t('sharedDashboard.noComponents')}
          </div>
        )}
      </div>
    </div>
  )
}

// ============================================================================
// Dashboard Content (reuses renderDashboardComponent from main dashboard)
// ============================================================================

interface SharedDashboardContentProps {
  dashboard: Dashboard
}

function SharedDashboardContent({ dashboard }: SharedDashboardContentProps) {
  const [ready, setReady] = useState(false)

  // Get store actions for fetching device data
  const fetchDevices = useStore(s => s.fetchDevices)
  const fetchDeviceTypes = useStore(s => s.fetchDeviceTypes)
  const fetchDevicesCurrentBatch = useStore(s => s.fetchDevicesCurrentBatch)

  // Fetch installed components and device data
  useEffect(() => {
    let mounted = true

    const loadData = async () => {
      // Phase 1: fetch registries and base data in parallel
      await Promise.allSettled([
        fetchAPI<{ components: FrontendComponentMeta[] }>('/frontend-components', { skipAuth: true })
          .then(res => {
            if (mounted && res.components?.length) {
              communityRegistry.syncFromApi(res.components)
            }
          })
          .catch(e => console.warn('[SharedDashboard] Community components:', e)),

        fetchAPI<{ components: any[] }>('/extensions/dashboard-components', { skipAuth: true })
          .then(res => {
            if (mounted && res.components?.length) {
              for (const comp of res.components) {
                dynamicRegistry.register(comp.extension_id || 'unknown', comp.extension_name || '', comp)
              }
            }
          })
          .catch(e => console.warn('[SharedDashboard] Extension components:', e)),

        // Devices + device types (must complete before current-batch fetch)
        fetchDevices().catch(e => console.warn('[SharedDashboard] Devices:', e)),
        fetchDeviceTypes().catch(e => console.warn('[SharedDashboard] Device types:', e)),
      ])

      // Phase 2: fetch current values for all bound devices
      if (mounted) {
        try {
          const devices = useStore.getState().devices
          if (devices.length > 0) {
            await fetchDevicesCurrentBatch(devices.map(d => d.id))
          }
        } catch (e) {
          console.warn('[SharedDashboard] Device current values:', e)
        }
      }

      if (mounted) setReady(true)
    }

    loadData()
    return () => { mounted = false }
  }, [fetchDevices, fetchDeviceTypes, fetchDevicesCurrentBatch])

  // Polling: refresh device current values every 30s for real-time updates
  // (shared dashboard has no WebSocket, so we poll instead)
  useEffect(() => {
    if (!ready) return
    const interval = setInterval(async () => {
      try {
        const devices = useStore.getState().devices
        if (devices.length > 0) {
          await fetchDevicesCurrentBatch(devices.map(d => d.id))
        }
      } catch {
        // silently ignore polling errors
      }
    }, 30_000)
    return () => clearInterval(interval)
  }, [ready, fetchDevicesCurrentBatch])

  const gridComponents = useMemo(
    () =>
      dashboard.components.map((comp) => ({
        id: comp.id,
        position: comp.position,
        children: (
          <ComponentErrorBoundary>
            <div className="relative h-full">
              <div className="h-full w-full flex flex-col">
                {renderDashboardComponent(comp, false)}
              </div>
            </div>
          </ComponentErrorBoundary>
        ),
      })),
    [dashboard.components],
  )

  if (!ready) {
    return (
      <div className="flex items-center justify-center h-64">
        <LoadingState size="md" />
      </div>
    )
  }

  return <DashboardGrid components={gridComponents} editMode={false} />
}

/**
 * Convert raw API share response to internal Dashboard format.
 * The backend returns snake_case (data_source) but components expect camelCase (dataSource).
 */
function convertShareResponse(raw: any): Dashboard {
  return fromDashboardDTO(raw.dashboard ?? raw)
}

export default SharedDashboard

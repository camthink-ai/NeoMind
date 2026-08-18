import { lazy, Suspense, useEffect, useState, useRef } from "react"
import { Routes, Route, Navigate, useLocation } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { ErrorBoundary } from "@/components/shared/ErrorBoundary"
import { BrandGradientDef } from "@/components/shared/BrandGradientDef"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { TopBar } from "@/components/layout/TopBar"
import { AppSidebar } from "@/components/layout/AppSidebar"
import { MobileNav } from "@/components/layout/MobileNav"
import { SwipeNavigation } from "@/components/layout/SwipeNavigation"
import { NavigationProgress } from "@/components/layout/NavigationProgress"
import { useIsMobile } from "@/hooks/useMobile"
import { Toaster } from "@/components/ui/toaster"
import { Confirmer } from "@/components/ui/confirmer"
import { tokenManager, getApiBase, isTauriEnv, setApiBase, getApiKey } from "@/lib/api"
import { prefetchServerUrl } from "@/lib/server-url"
import { StartupLoading } from "@/components/StartupLoading"
import { LoadingState } from "@/components/shared/LoadingState"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useExtensionComponents } from "@/hooks/useExtensionComponents"
import { UpdateDialog } from '@/components/update'
import { InstanceSwitchOverlay } from '@/components/layout/InstanceSwitchOverlay'
import { GlobalChatFab } from '@/components/chat/GlobalChatFab'
import { SettingsDialog } from "@/components/settings/SettingsDialog"
import { useUpdateCheck } from '@/hooks/useUpdateCheck'
import { listen } from "@tauri-apps/api/event"
import type { ConnectionState } from "@/lib/websocket"
import { BackendUnavailableOverlay } from "@/components/BackendUnavailableOverlay"

// Performance optimization: Lazy load route components to reduce initial bundle size
// Each page is loaded on-demand, reducing Time to Interactive by ~70%
const LoginPage = lazy(() => import('@/pages/login').then(m => ({ default: m.LoginPage })))
const SetupPage = lazy(() => import('@/pages/setup').then(m => ({ default: m.SetupPage })))
const ChatPage = lazy(() => import('@/pages/chat').then(m => ({ default: m.ChatPage })))
const VisualDashboard = lazy(() =>
  import('@/pages/dashboard-components/VisualDashboard').then(m => ({ default: m.VisualDashboard }))
)
const DataExplorerPage = lazy(() => import('@/pages/data-explorer').then(m => ({ default: m.DataExplorerPage }))
)
const DevicesPage = lazy(() => import('@/pages/devices').then(m => ({ default: m.DevicesPage })))
const AutomationPage = lazy(() => import('@/pages/automation').then(m => ({ default: m.AutomationPage })))
const AgentsPage = lazy(() => import('@/pages/agents').then(m => ({ default: m.AgentsPage })))
const MessagesPage = lazy(() => import('@/pages/messages').then(m => ({ default: m.default })))
const ExtensionsPage = lazy(() => import('@/pages/extensions').then(m => ({ default: m.ExtensionsPage })))
const SystemPage = lazy(() => import('@/pages/SystemPage'))
const SharedDashboardPage = lazy(() => import('@/pages/share/SharedDashboard'))
const NotFound = lazy(() => import('@/pages/NotFound'))
// Suppress only the specific Radix UI Portal cleanup error during fast page transitions
// Known issue: React 18 + Radix UI race condition where removeChild fails on unmounted portals
const originalError = console.error
const originalWarn = console.warn
const SUPPRESSED_ERROR = "NotFoundError: Failed to execute 'removeChild' on 'Node'"
const SUPPRESSED_RECHARTS = "The width(-1) and height(-1) of chart should be greater than 0"
console.error = (...args) => {
  const message = args[0]
  // Only suppress the exact Radix UI removeChild error — nothing else
  if (typeof message === 'string' && message.includes(SUPPRESSED_ERROR)) {
    return
  }
  originalError.apply(console, args)
}
// Suppress recharts width/height -1 warning when charts render in hidden containers
console.warn = (...args) => {
  const message = args[0]
  if (typeof message === 'string' && message.includes(SUPPRESSED_RECHARTS)) {
    return
  }
  originalWarn.apply(console, args)
}

// Suppress only the exact global error from the same Radix UI race condition
window.addEventListener('error', (event) => {
  if (event.message?.includes(SUPPRESSED_ERROR)) {
    event.preventDefault()
    return false
  }
})

window.addEventListener('unhandledrejection', (event) => {
  if (event.reason?.message?.includes(SUPPRESSED_ERROR)) {
    event.preventDefault()
    return false
  }
})

// Set the macOS Tauri title-bar inset globally (login/setup pages don't have TopNav).
const _isMacTauri = typeof window !== 'undefined' && '__TAURI__' in window && /Mac/i.test(navigator.platform || navigator.userAgent)
if (_isMacTauri) {
  document.documentElement.style.setProperty('--titlebar-inset', '24px')
}

// Protected Route component
// Checks authentication first, then setup status in background
/** Legacy /settings deep links: settings now lives in the SettingsDialog
 * (mounted once at the app root). Open the dialog, then redirect home so
 * closing it never leaves a blank page behind. */
function SettingsRoute() {
  const openSettings = useStore((state) => state.openSettings)
  useEffect(() => {
    openSettings()
  }, [openSettings])
  return <Navigate to="/" replace />
}

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const [setupRequired, setSetupRequired] = useState<boolean | false>(false)

  // Warm the canonical server URL cache so webhook URL displays don't flash
  // localhost before the backend-provided LAN IP arrives (Tauri mode only).
  useEffect(() => {
    prefetchServerUrl().catch(() => {})
  }, [])

  useEffect(() => {
    // Check setup status in background - don't block rendering
    const checkSetup = async (): Promise<void> => {
      const apiBase = getApiBase()
      try {
        const response = await fetch(`${apiBase}/setup/status`, {
          signal: AbortSignal.timeout(3000),
        })
        if (response.ok) {
          const data = await response.json() as { setup_required: boolean }
          if (data.setup_required) {
            setSetupRequired(true)
          }
        }
      } catch {
        // On error, don't redirect - let user continue
      }
    }

    checkSetup()
  }, [])

  // Check token on every render (not in useEffect) to respond immediately to login
  const token = tokenManager.getToken()
  const apiKey = getApiKey()

  // Not authenticated — no JWT and no API key
  if (!token && !apiKey) {
    return <Navigate to="/login" replace />
  }

  // Setup required - redirect to setup page
  if (setupRequired) {
    return <Navigate to="/setup" replace />
  }

  return <>{children}</>
}

// Setup Route component
// Only accessible when setup is required (no users exist yet)
// If setup is already completed, redirects to login page
function SetupRoute({ children }: { children: React.ReactNode }) {
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState(false)

  useEffect(() => {
    const checkSetup = async (): Promise<boolean> => {
      const apiBase = getApiBase()
      try {
        const response = await fetch(`${apiBase}/setup/status`, {
          signal: AbortSignal.timeout(5000),
        })
        if (response.ok) {
          const data = await response.json() as { setup_required: boolean }
          return data.setup_required
        }
      } catch {
        // On error, allow access to setup (for offline scenarios)
        return true
      }
      return false
    }

    checkSetup().then(result => {
      setSetupRequired(result)
      setLoading(false)
    }).catch(() => {
      setSetupRequired(true)
      setLoading(false)
    })

    // Fallback timeout to prevent indefinite loading
    const fallbackTimer = setTimeout(() => {
      setLoading(false)
      setError(true)
    }, 6000)

    return () => clearTimeout(fallbackTimer)
  }, [])

  // Show loading state during initial check
  if (loading && !error) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="animate-pulse text-muted-foreground">Loading...</div>
      </div>
    )
  }

  // Setup already completed - redirect to login
  if (setupRequired === false) {
    return <Navigate to="/login" replace />
  }

  // Show setup page (either setup_required is true, or we encountered an error and allow access)
  return <>{children}</>
}

// Loading component for lazy-loaded routes

// Prevent Backspace/Delete from triggering browser back navigation in Tauri WebView
function handleTauriBackspace(e: KeyboardEvent) {
  if (e.key === 'Backspace' || e.key === 'Delete') {
    const target = e.target as HTMLElement
    const isEditable = target instanceof HTMLInputElement ||
      target instanceof HTMLTextAreaElement ||
      target.isContentEditable
    if (!isEditable) {
      e.preventDefault()
    }
  }
}
function PageLoading() {
  return (
    <div className="min-h-screen flex items-center justify-center bg-background">
      <LoadingState size="lg" text="Loading..." />
    </div>
  )
}

function App() {
  const isMobile = useIsMobile()
  const { t } = useTranslation("common")
  const extensionComponents = useExtensionComponents({ autoSync: true, syncInterval: 60000 })
  const extensionSyncRef = useRef(extensionComponents.sync)
  
  // 更新 ref 当 sync 函数变化时
  useEffect(() => {
    extensionSyncRef.current = extensionComponents.sync
  }, [extensionComponents.sync])

  // Expose a scoped stream-URL helper to extension UMD bundles.
  // Extensions cannot import host modules, and we deliberately avoid
  // exposing the raw JWT via a window getter. The helper returns a
  // ready-to-use ws:// URL only, keeping the token embedded in the URL
  // (same exposure surface as any fetch with ?token=).
  useEffect(() => {
    const buildStreamUrl = (extensionId: string): string | null => {
      const token = tokenManager.getToken()
      if (!token) return null
      const apiBase = getApiBase()
      try {
        // getApiBase() already includes the '/api' prefix in all modes
        // (Tauri: 'http://localhost:9375/api', web: '/api'). Don't add another.
        const httpUrl = `${apiBase}/extensions/${encodeURIComponent(extensionId)}/stream?token=${encodeURIComponent(token)}`
        return httpUrl.replace(/^http:/, 'ws:').replace(/^https:/, 'wss:')
      } catch {
        return null
      }
    }
    ;(window as any).NeoMindStream = { urlFor: buildStreamUrl }
    return () => {
      delete (window as any).NeoMindStream
    }
  }, [])
  const { isAuthenticated, checkAuthStatus, setWsConnected, updateDialogOpen } = useStore((s) => ({
    isAuthenticated: s.isAuthenticated,
    checkAuthStatus: s.checkAuthStatus,
    setWsConnected: s.setWsConnected,
    updateDialogOpen: s.updateDialogOpen,
  }), shallow)
  
  // Global auto-update check with system notification
  useUpdateCheck({
    autoCheck: true,
    checkInterval: 24 * 60 * 60 * 1000, // 24 hours
    showNotification: true,
  })
  const location = useLocation()
  const [backendReady, setBackendReady] = useState(false)
  const [isTauri, setIsTauri] = useState(false)
  const [initialCheckDone, setInitialCheckDone] = useState(false)
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null)
  // Backend startup failure surfaced from Rust (Tauri "backend-start-failed"
  // event, e.g. port 9375 already in use). null while healthy.
  const [backendError, setBackendError] = useState<{ error: string; port_conflict: boolean } | null>(null)
  // WebSocket connection state, used for the fallback error overlay (web build
  // or a missed Tauri event).
  const [wsConnectionState, setWsConnectionState] = useState<ConnectionState>({ status: 'disconnected' })

  // Surface a backend startup failure (port conflict, crash at startup) as a
  // clear error page instead of letting the user stare at endless
  // "Reconnecting". Tauri-only — the web build relies on the WS fallback below.
  useEffect(() => {
    if (!isTauriEnv()) return
    let unlisten: (() => void) | undefined
    listen<{ error: string; port_conflict: boolean }>("backend-start-failed", (e) => {
      setBackendError(e.payload)
    }).then((fn) => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [])

  // Subscribe to WS state for the fallback path: if the socket never connected
  // and gave up retrying, show the same error overlay (covers web/non-Tauri
  // and the case where the Tauri event was missed).
  useEffect(() => {
    let unsub: (() => void) | undefined
    import("@/lib/websocket").then(({ ws }) => {
      unsub = ws.onStateChange(setWsConnectionState)
    })
    return () => {
      unsub?.()
    }
  }, [])

  // Reset viewport and scroll when route changes (fix mobile keyboard dismissal issues)
  useEffect(() => {
    // Force viewport reset to clear any lingering keyboard state
    forceViewportReset()

    // Reset body scroll lock styles that might have been left behind
    document.body.style.overflow = ''
    document.body.style.position = ''
    document.body.style.top = ''
    document.body.style.width = ''
    // Defensive: clear any stray transforms that iOS PWA may have applied to
    // ancestors during input focus inside position:fixed containers.
    document.body.style.transform = ''
    document.documentElement.style.transform = ''

    // Force scroll to top across every channel iOS PWA might use. html has
    // `overflow: hidden` (see index.css) so the document never scrolls, but
    // keep this as a defensive reset for older browsers / edge cases.
    window.scrollTo(0, 0)
    document.body.scrollTop = 0
    document.documentElement.scrollTop = 0
    if (document.scrollingElement) {
      ;(document.scrollingElement as HTMLElement).scrollTop = 0
    }

    // Force layout recalculation
    void document.body.offsetHeight
  }, [location.pathname])

  // Initialize instance: restore API base URL from saved instance selection
  // Only runs when there's no pending switch (applyPendingSwitch handles that case).
  // For normal refresh, we restore the remote URL from the cached instance list.
  useEffect(() => {
    // Skip if applyPendingSwitch already handled initialization (switching state)
    if (useStore.getState().switchingState === 'switching') return

    try {
      const savedId = localStorage.getItem('currentInstanceId')
      if (savedId && savedId !== 'local-default') {
        // Restore API base from cached instances (available synchronously)
        const cached = JSON.parse(localStorage.getItem('neomind_instance_cache') || '[]')
        const instance = cached.find((i: { id: string }) => i.id === savedId)
        if (instance && !instance.is_local) {
          setApiBase(`${instance.url}/api`)
          // API key is restored from sessionStorage by urls.ts module init
        }
      }
    } catch {
      // Failed to restore — use default (local) URL
    }
  }, [])

  // Track path changes (keep existing logic for other parts of app)
  const [currentPath, setCurrentPath] = useState(() => window.location.pathname)
  useEffect(() => {
    const handleLocationChange = () => setCurrentPath(window.location.pathname)
    window.addEventListener('popstate', handleLocationChange)
    // Also check on pushState/replaceState
    const originalPushState = history.pushState
    const originalReplaceState = history.replaceState
    history.pushState = function(...args) {
      originalPushState.apply(this, args)
      handleLocationChange()
    }
    history.replaceState = function(...args) {
      originalReplaceState.apply(this, args)
      handleLocationChange()
    }
    return () => {
      window.removeEventListener('popstate', handleLocationChange)
      history.pushState = originalPushState
      history.replaceState = originalReplaceState
    }
  }, [])

  // Check if running in Tauri environment
  useEffect(() => {
    const tauri = isTauriEnv()
    setIsTauri(tauri)
    if (tauri) {
      window.addEventListener('keydown', handleTauriBackspace)
    }
    return () => {
      if (tauri) {
        window.removeEventListener('keydown', handleTauriBackspace)
      }
    }
  }, [])

  // Initial setup check - runs before routes are rendered
  useEffect(() => {
    const checkInitialSetup = async () => {
      const apiBase = getApiBase()
      try {
        const response = await fetch(`${apiBase}/setup/status`, {
          signal: AbortSignal.timeout(5000),
        })
        if (response.ok) {
          const data = await response.json() as { setup_required: boolean }
          setSetupRequired(data.setup_required)
        }
      } catch {
        // On error, assume setup is not required
        setSetupRequired(false)
      } finally {
        setInitialCheckDone(true)
      }
    }

    // Only check after backend is ready in Tauri
    if (!isTauri || backendReady) {
      checkInitialSetup()
    }
  }, [isTauri, backendReady])

  // Check authentication status on mount.
  // Re-check when navigating away from /login or /setup (post-login/setup redirect).
  // Uses a ref to avoid redundant API calls during normal navigation.
  const prevPathRef = useRef(currentPath)
  useEffect(() => {
    const prevPath = prevPathRef.current
    prevPathRef.current = currentPath
    const cameFromAuth = prevPath === '/login' || prevPath === '/setup'
    const isOnProtectedPage = currentPath !== '/login' && currentPath !== '/setup'
    // Check on mount (prevPath === currentPath) or after leaving auth pages
    if (isOnProtectedPage && (prevPath === currentPath || cameFromAuth)) {
      checkAuthStatus()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentPath])

  // Unified WebSocket + SSE connection management
  // Connects when authenticated (not on setup), disconnects otherwise
  useEffect(() => {
    if (isAuthenticated && currentPath !== '/setup') {
      import('@/lib/websocket').then(({ ws }) => {
        // Register connection handler for state tracking + extension re-sync
        const cleanup = ws.onConnection((connected, isReconnect) => {
          setWsConnected(connected)
          if (connected && isReconnect) {
            extensionSyncRef.current?.()
          }
        })
        setWsConnected(ws.isConnected())
        // Connect chat WebSocket (idempotent — skips if already connected)
        ws.connect()

        return cleanup
      })
      import('@/lib/events').then(({ refreshEventConnections }) => {
        refreshEventConnections()
      })
      // Sync extension components immediately on auth
      extensionSyncRef.current?.()
    } else if (!isAuthenticated || currentPath === '/setup') {
      import('@/lib/websocket').then(({ ws }) => {
        ws.disconnect()
      })
      import('@/lib/events').then(({ closeAllEventsConnections }) => {
        closeAllEventsConnections()
      })
    }
  }, [isAuthenticated, setWsConnected, currentPath])


  // Backend never came up — show a clear error page above everything else
  // (overrides startup loading / login) so the user knows the backend is
  // unreachable. `gaveUp` is set only after the fast-retry budget is exhausted
  // WITHOUT ever connecting (port conflict, crashed/misconfigured backend, or a
  // slow-booting edge box) — NOT on every transient connect error, which avoids
  // a full-screen flash per failed attempt. The WS keeps slow-polling in the
  // background, so this clears automatically once the backend actually answers.
  if (backendError || wsConnectionState.gaveUp) {
    return (
      <BackendUnavailableOverlay
        portConflict={backendError?.port_conflict}
        error={backendError?.error}
        onRetry={() => {
          setBackendError(null)
          import("@/lib/websocket").then(({ ws }) => {
            ws.manualReconnect()
          })
        }}
      />
    )
  }

  // Show loading screen in Tauri until backend is ready
  if (isTauri && !backendReady) {
    return <StartupLoading onReady={() => setBackendReady(true)} />
  }

  // Show loading while checking initial setup status
  if (!initialCheckDone) {
    return <PageLoading />
  }

  // Auto-redirect to setup if required (fresh install)
  // Check if current path is not already /setup to avoid redirect loop
  // Also don't redirect if we're already on login page
  if (setupRequired && currentPath !== '/setup' && currentPath !== '/login') {
    return <Navigate to="/setup" replace />
  }

  return (
    <>
      <NavigationProgress />
      <BrandGradientDef />
      <Suspense fallback={<PageLoading />}>
        <Routes>
          {/* Setup route - protected, only accessible when setup required */}
          <Route
            path="/setup"
            element={
              <SetupRoute>
                <SetupPage />
              </SetupRoute>
            }
          />

          {/* Login route */}
          <Route path="/login" element={<LoginPage />} />

          {/* Shared dashboard (public, no auth required) */}
          <Route path="/share/:token" element={<SharedDashboardPage />} />

          {/* Protected routes */}
          <Route
            path="/*"
            element={
              <ProtectedRoute>
                <div className="flex" style={{height: 'var(--app-height, 100vh)'}}>
                  {!isMobile && <AppSidebar />}
                  {/* Full-height slot for page-owned sidebars (chat sessions,
                      dashboard list) — they portal in here to sit level with
                      the AppSidebar, so the TopBar spans only the content
                      area. Empty (0 width) on pages that don't use it. */}
                  {!isMobile && <div id="page-sidebar-slot" className="flex shrink-0" />}
                  <div className="flex flex-col flex-1 min-w-0">
                    {!isMobile && <TopBar />}
                    <MobileNav />
                    {/* Skip link — keyboard users tab past the nav straight to content.
                        Visually hidden until focused. */}
                    <a
                      href="#main-content"
                      className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-[400] focus:rounded-md focus:bg-background focus:px-3 focus:py-2 focus:text-sm focus:shadow-lg focus:border focus:border-border"
                    >
                      {t("skipToContent")}
                    </a>
                    <main
                      id="main-content"
                      tabIndex={-1}
                      className="relative flex flex-1 min-h-0 overflow-hidden focus:outline-none"
                    >
                    <div className="w-full h-full overflow-hidden" id="main-scroll-container">
                    <ErrorBoundary>
                    <div key={location.pathname.split('/')[1] || 'root'} className="animate-page-enter w-full h-full overflow-hidden">
                    <Routes>
                      <Route path="/" element={<ChatPage />} />
                      <Route path="/chat" element={<ChatPage />} />
                      {/* Session-based routes */}
                      <Route path="/chat/:sessionId" element={<ChatPage />} />
                      <Route path="/visual-dashboard" element={<VisualDashboard />} />
                      <Route path="/visual-dashboard/:dashboardId" element={<VisualDashboard />} />
                      {/* Data Explorer */}
                      <Route path="/data" element={<DataExplorerPage />} />
                      {/* Devices with tab routes */}
                      <Route path="/devices" element={<DevicesPage />} />
                      <Route path="/devices/:id" element={<DevicesPage />} />
                      <Route path="/devices/types" element={<DevicesPage />} />
                      <Route path="/devices/drafts" element={<DevicesPage />} />
                      {/* Automation with tab routes */}
                      <Route path="/automation" element={<AutomationPage />} />
                      <Route path="/automation/transforms" element={<AutomationPage />} />
                      {/* Agents with tab routes */}
                      <Route path="/agents" element={<AgentsPage />} />
                      <Route path="/agents/memory" element={<AgentsPage />} />
                      <Route path="/agents/skills" element={<AgentsPage />} />
                      <Route path="/agents/tools" element={<AgentsPage />} />
                      <Route path="/settings" element={<SettingsRoute />} />
                      {/* Messages with tab routes */}
                      <Route path="/messages" element={<MessagesPage />} />
                      <Route path="/messages/channels" element={<MessagesPage />} />
                      <Route path="/messages/im" element={<MessagesPage />} />
                      {/* Extensions */}
                      <Route path="/extensions" element={<ExtensionsPage />} />
                      <Route path="/plugins" element={<Navigate to="/extensions" replace />} />
                      {/* System container page (mobile primary nav target) */}
                      <Route path="/system" element={<SystemPage />} />
                      {/* Catch all - 404 page */}
                      <Route path="*" element={<NotFound />} />
                    </Routes>
                    </div>
                    </ErrorBoundary>
                    </div>
                  </main>
                  <SettingsDialog />
                  <SwipeNavigation />
                  {!isMobile && <GlobalChatFab />}
                  </div>
                </div>
              </ProtectedRoute>
            }
          />
        </Routes>
      </Suspense>
      {/* Show toaster and confirmer on login page too */}
      <Toaster />
      <Confirmer />
      {/* Global Update Dialog */}
      <UpdateDialog
        open={updateDialogOpen}
        onClose={() => useStore.setState({ updateDialogOpen: false })}
      />
      {/* Global Instance Switch Overlay */}
      <InstanceSwitchOverlay />
    </>
  )
}

export default App

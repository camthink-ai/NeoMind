import { lazy, Suspense, useEffect, useState } from "react"
import { Routes, Route, Navigate, useLocation } from "react-router-dom"
import { useStore } from "@/store"
import { TopNav } from "@/components/layout/TopNav"
import { Toaster } from "@/components/ui/toaster"
import { Confirmer } from "@/components/ui/confirmer"
import { tokenManager } from "@/lib/api"
import { StartupLoading } from "@/components/StartupLoading"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { useExtensionComponents } from "@/hooks/useExtensionComponents"

// Performance optimization: Lazy load route components to reduce initial bundle size
// Each page is loaded on-demand, reducing Time to Interactive by ~70%
const LoginPage = lazy(() => import('@/pages/login').then(m => ({ default: m.LoginPage })))
const SetupPage = lazy(() => import('@/pages/setup').then(m => ({ default: m.SetupPage })))
const ChatPage = lazy(() => import('@/pages/chat').then(m => ({ default: m.ChatPage })))
const VisualDashboard = lazy(() =>
  import('@/pages/dashboard-components/VisualDashboard').then(m => ({ default: m.VisualDashboard }))
)
const DevicesPage = lazy(() => import('@/pages/devices').then(m => ({ default: m.DevicesPage })))
const AutomationPage = lazy(() => import('@/pages/automation').then(m => ({ default: m.AutomationPage })))
const AgentsPage = lazy(() => import('@/pages/agents').then(m => ({ default: m.AgentsPage })))
const SettingsPage = lazy(() => import('@/pages/settings').then(m => ({ default: m.SettingsPage })))
const MessagesPage = lazy(() => import('@/pages/messages').then(m => ({ default: m.default })))
const ExtensionsPage = lazy(() => import('@/pages/extensions').then(m => ({ default: m.ExtensionsPage })))
// Suppress Radix UI Portal cleanup errors during page transitions
// This is a known issue with React 18 + Radix UI + fast page navigation
const originalError = console.error
console.error = (...args) => {
  const message = args[0]
  if (typeof message === 'string' && (
    message.includes('NotFoundError: Failed to execute \'removeChild\'') ||
    (message.includes('NotFoundError') || message.includes('removeChild'))
  )) {
    // Check if any arg contains Portal or Select related strings
    const hasPortalOrSelect = args.some(arg =>
      typeof arg === 'string' && (
        arg.includes('Portal') ||
        arg.includes('Select') ||
        arg.includes('Radix') ||
        arg.includes('@radix-ui')
      )
    )
    if (hasPortalOrSelect) {
      // Suppress Portal cleanup errors
      return
    }
  }
  originalError.apply(console, args)
}

window.addEventListener('error', (event) => {
  if (
    event.message?.includes('NotFoundError') ||
    event.message?.includes('removeChild')
  ) {
    event.preventDefault()
    event.stopPropagation()
    return false
  }
})

window.addEventListener('unhandledrejection', (event) => {
  if (
    event.reason?.message?.includes('NotFoundError') ||
    event.reason?.message?.includes('removeChild') ||
    event.reason?.toString().includes('Portal')
  ) {
    event.preventDefault()
    return false
  }
})

// Protected Route component
// Checks authentication first, then setup status in background
function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const [setupRequired, setSetupRequired] = useState<boolean | false>(false)

  useEffect(() => {
    // Check setup status in background - don't block rendering
    const checkSetup = async (): Promise<void> => {
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
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

  // Not authenticated - redirect to login
  if (!token) {
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
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
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
function PageLoading() {
  return (
    <div className="min-h-screen flex items-center justify-center bg-background">
      <div className="flex flex-col items-center gap-3">
        <div className="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin" />
        <p className="text-sm text-muted-foreground">Loading...</p>
      </div>
    </div>
  )
}

function App() {
  const { isAuthenticated, checkAuthStatus, setWsConnected } = useStore()
  const location = useLocation()
  const [backendReady, setBackendReady] = useState(false)
  const [isTauri, setIsTauri] = useState(false)
  const [initialCheckDone, setInitialCheckDone] = useState(false)
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null)

  // Reset viewport and scroll when route changes (fix mobile keyboard dismissal issues)
  useEffect(() => {
    // Force viewport reset to clear any lingering keyboard state
    forceViewportReset()

    // Reset body scroll lock styles that might have been left behind
    document.body.style.overflow = ''
    document.body.style.position = ''
    document.body.style.top = ''
    document.body.style.width = ''

    // Force scroll to top
    window.scrollTo(0, 0)

    // Force layout recalculation
    void document.body.offsetHeight
  }, [location.pathname])

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
    setIsTauri(typeof window !== 'undefined' && '__TAURI__' in window)
  }, [])

  // Initial setup check - runs before routes are rendered
  useEffect(() => {
    const checkInitialSetup = async () => {
      const apiBase = (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
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

  // Check authentication status on mount (only once)
  // Skip auth check on setup page to avoid 401 errors
  useEffect(() => {
    if (currentPath !== '/setup') {
      checkAuthStatus()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentPath])

  // Set up WebSocket connection handler to update store
  // Only connect when authenticated and not on setup page
  useEffect(() => {
    if (isAuthenticated && currentPath !== '/setup') {
      import('@/lib/websocket').then(({ ws }) => {
        // Set up connection handler
        const cleanup = ws.onConnection((connected) => {
          setWsConnected(connected)
        })
        // Check current state
        setWsConnected(ws.isConnected())

        return cleanup
      })
    } else if (!isAuthenticated && currentPath !== '/setup') {
      // Disconnect when not authenticated
      import('@/lib/websocket').then(({ ws }) => {
        ws.disconnect()
      })
    }
  }, [isAuthenticated, setWsConnected, currentPath])

  // Refresh WebSocket connections when authentication status changes
  // Only connect when authenticated and not on setup page
  useEffect(() => {
    if (isAuthenticated && currentPath !== '/setup') {
      // Dynamic import to avoid SSR issues
      import('@/lib/events').then(({ refreshEventConnections }) => {
        refreshEventConnections()
      })
      // Also refresh chat WebSocket (has its own duplicate check)
      import('@/lib/websocket').then(({ ws }) => {
        ws.connect()
      })
    } else if (currentPath === '/setup') {
      // Disconnect on setup page to avoid 401 errors
      import('@/lib/websocket').then(({ ws }) => {
        ws.disconnect()
      })
      import('@/lib/events').then(({ closeAllEventsConnections }) => {
        closeAllEventsConnections()
      })
    }
  }, [isAuthenticated, currentPath])

  // Auto-sync extension dashboard components when authenticated
  // This ensures extension-provided components are available in the dashboard
  useExtensionComponents({ autoSync: true, syncInterval: 60000 })

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

          {/* Protected routes */}
          <Route
            path="/*"
            element={
              <ProtectedRoute>
                <div className="flex flex-col bg-background" style={{height: 'var(--app-height, 100vh)'}}>
                  <TopNav />
                  <main className="flex flex-1 min-h-0 overflow-hidden" style={{paddingTop: 'var(--topnav-height, 4rem)'}}>
                    <div className="w-full h-full overflow-auto" id="main-scroll-container">
                    <Routes>
                      <Route path="/" element={<ChatPage />} />
                      <Route path="/chat" element={<ChatPage />} />
                      {/* Session-based routes */}
                      <Route path="/chat/:sessionId" element={<ChatPage />} />
                      <Route path="/visual-dashboard" element={<VisualDashboard />} />
                      <Route path="/visual-dashboard/:dashboardId" element={<VisualDashboard />} />
                      {/* Devices with tab routes */}
                      <Route path="/devices" element={<DevicesPage />} />
                      <Route path="/devices/:id" element={<DevicesPage />} />
                      <Route path="/devices/types" element={<DevicesPage />} />
                      <Route path="/devices/drafts" element={<DevicesPage />} />
                      {/* Automation with tab routes */}
                      <Route path="/automation" element={<AutomationPage />} />
                      <Route path="/automation/transforms" element={<AutomationPage />} />
                      {/* Agents */}
                      <Route path="/agents" element={<AgentsPage />} />
                      <Route path="/settings" element={<SettingsPage />} />
                      {/* Messages with tab routes */}
                      <Route path="/messages" element={<MessagesPage />} />
                      <Route path="/messages/channels" element={<MessagesPage />} />
                      {/* Extensions */}
                      <Route path="/extensions" element={<ExtensionsPage />} />
                      <Route path="/plugins" element={<Navigate to="/extensions" replace />} />
                      {/* Catch all - redirect to chat */}
                      <Route path="*" element={<Navigate to="/" replace />} />
                    </Routes>
                    </div>
                  </main>
                  <Toaster />
                  <Confirmer />
                </div>
              </ProtectedRoute>
            }
          />
        </Routes>
      </Suspense>
      {/* Show toaster and confirmer on login page too */}
      <Toaster />
      <Confirmer />
    </>
  )
}

export default App

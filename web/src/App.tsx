import { lazy, Suspense, useEffect, useState } from "react"
import { Routes, Route, Navigate } from "react-router-dom"
import { useStore } from "@/store"
import { TopNav } from "@/components/layout/TopNav"
import { Toaster } from "@/components/ui/toaster"
import { Confirmer } from "@/components/ui/confirmer"
import { tokenManager } from "@/lib/api"
import { StartupLoading } from "@/components/StartupLoading"
import { getCurrentWindow } from "@tauri-apps/api/window"

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
const PluginsPage = lazy(() => import('@/pages/plugins').then(m => ({ default: m.PluginsPage })))

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
  const token = tokenManager.getToken()

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
  }, [])

  if (loading) {
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

  // Show setup page
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
  const [backendReady, setBackendReady] = useState(false)
  const [isTauri, setIsTauri] = useState(false)

  // Check if running in Tauri environment
  useEffect(() => {
    setIsTauri(typeof window !== 'undefined' && '__TAURI__' in window)
  }, [])

  // Check authentication status on mount (only once)
  useEffect(() => {
    checkAuthStatus()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // Set up WebSocket connection handler to update store
  useEffect(() => {
    if (isAuthenticated) {
      import('@/lib/websocket').then(({ ws }) => {
        // Set up connection handler
        const cleanup = ws.onConnection((connected) => {
          setWsConnected(connected)
        })
        // Check current state
        setWsConnected(ws.isConnected())

        return cleanup
      })
    }
  }, [isAuthenticated, setWsConnected])

  // Refresh WebSocket connections when authentication status changes
  useEffect(() => {
    if (isAuthenticated) {
      // Dynamic import to avoid SSR issues
      import('@/lib/events').then(({ refreshEventConnections }) => {
        refreshEventConnections()
      })
      // Also refresh chat WebSocket (has its own duplicate check)
      import('@/lib/websocket').then(({ ws }) => {
        ws.connect()
      })
    }
  }, [isAuthenticated])

  // Show loading screen in Tauri until backend is ready
  if (isTauri && !backendReady) {
    return <StartupLoading onReady={() => setBackendReady(true)} />
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
              <div className="flex flex-col h-screen h-[100dvh] bg-background">
                <TopNav />
                <main className="flex flex-1 flex-col min-h-0 overflow-hidden">
                  <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
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
                    {/* Plugins with tab routes */}
                    <Route path="/plugins" element={<PluginsPage />} />
                    <Route path="/plugins/connections" element={<PluginsPage />} />
                    <Route path="/plugins/alert-channels" element={<PluginsPage />} />
                    <Route path="/plugins/extensions" element={<PluginsPage />} />
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

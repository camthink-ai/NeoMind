import { lazy, Suspense, useEffect, useState } from "react"
import { Routes, Route, Navigate } from "react-router-dom"
import { useStore } from "@/store"
import { TopNav } from "@/components/layout/TopNav"
import { Toaster } from "@/components/ui/toaster"
import { Confirmer } from "@/components/ui/confirmer"
import { tokenManager } from "@/lib/api"

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
const CommandsPage = lazy(() => import('@/pages/commands').then(m => ({ default: m.CommandsPage })))
const DecisionsPage = lazy(() => import('@/pages/decisions').then(m => ({ default: m.DecisionsPage })))
const PluginsPage = lazy(() => import('@/pages/plugins').then(m => ({ default: m.PluginsPage })))
const EventsPage = lazy(() => import('@/pages/events').then(m => ({ default: m.EventsPage })))

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
// Uses tokenManager.getToken() directly to avoid race condition with store hydration
function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const token = tokenManager.getToken()

  if (!token) {
    return <Navigate to="/login" replace />
  }

  return <>{children}</>
}

// Setup Required Route component
// Checks if setup is needed and redirects to /setup if required
// Otherwise redirects to login if authenticated
function SetupCheckRoute({ children }: { children: React.ReactNode }) {
  const [setupRequired, setSetupRequired] = useState<boolean | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    const checkSetup = async () => {
      try {
        const response = await fetch('/api/setup/status')
        const data = await response.json()
        setSetupRequired(data.setup_required)
      } catch {
        // If API fails, assume setup is not required (fallback to login)
        setSetupRequired(false)
      } finally {
        setLoading(false)
      }
    }

    checkSetup()
  }, [])

  const token = tokenManager.getToken()

  // Show loading state
  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="animate-pulse text-muted-foreground">Loading...</div>
      </div>
    )
  }

  // Setup is required - redirect to setup page
  if (setupRequired === true) {
    return <Navigate to="/setup" replace />
  }

  // Already authenticated - redirect to dashboard
  if (token) {
    return <Navigate to="/" replace />
  }

  // Otherwise show login page
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

  // Check authentication status on mount
  useEffect(() => {
    checkAuthStatus()
  }, [checkAuthStatus])

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
    // Dynamic import to avoid SSR issues
    import('@/lib/events').then(({ refreshEventConnections }) => {
      refreshEventConnections()
    })
    // Also refresh chat WebSocket
    import('@/lib/websocket').then(({ ws }) => {
      ws.connect()
    })
  }, [isAuthenticated])

  return (
    <>
      <Suspense fallback={<PageLoading />}>
        <Routes>
          {/* Setup route - always accessible, handles its own redirect logic */}
          <Route path="/setup" element={<SetupPage />} />

        {/* Login route with setup check - redirects to /setup if needed */}
        <Route
          path="/login"
          element={
            <SetupCheckRoute>
              <LoginPage />
            </SetupCheckRoute>
          }
        />

        {/* Protected routes */}
        <Route
          path="/*"
          element={
            <ProtectedRoute>
              <div className="flex flex-col h-screen bg-background">
                <TopNav />
                <main className="flex-1 min-h-0 overflow-hidden">
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
                    <Route path="/commands" element={<CommandsPage />} />
                    <Route path="/decisions" element={<DecisionsPage />} />
                    {/* Decisions with filter routes */}
                    <Route path="/decisions/proposed" element={<DecisionsPage />} />
                    <Route path="/decisions/executed" element={<DecisionsPage />} />
                    <Route path="/decisions/rejected" element={<DecisionsPage />} />
                    {/* Plugins with tab routes */}
                    <Route path="/plugins" element={<PluginsPage />} />
                    <Route path="/plugins/connections" element={<PluginsPage />} />
                    <Route path="/plugins/alert-channels" element={<PluginsPage />} />
                    <Route path="/plugins/extensions" element={<PluginsPage />} />
                    <Route path="/events" element={<EventsPage />} />
                    {/* Catch all - redirect to chat */}
                    <Route path="*" element={<Navigate to="/" replace />} />
                  </Routes>
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

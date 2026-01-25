import { useEffect } from "react"
import { Routes, Route, Navigate } from "react-router-dom"
import { useStore } from "@/store"
import { TopNav } from "@/components/layout/TopNav"
import { LoginPage } from "@/pages/login"
import { DashboardPage } from "@/pages/dashboard"
import { VisualDashboard } from "@/pages/dashboard-components/VisualDashboard"
import { DevicesPage } from "@/pages/devices"
import { AutomationPage } from "@/pages/automation"
import { AgentsPage } from "@/pages/agents"
import { SettingsPage } from "@/pages/settings"
import { CommandsPage } from "@/pages/commands"
import { DecisionsPage } from "@/pages/decisions"
import { PluginsPage } from "@/pages/plugins"
import { EventsPage } from "@/pages/events"
import { Toaster } from "@/components/ui/toaster"
import { Confirmer } from "@/components/ui/confirmer"
import { tokenManager } from "@/lib/api"

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

// Public Route component (redirect to dashboard if already authenticated)
// Uses tokenManager.getToken() directly to avoid race condition with store hydration
function PublicRoute({ children }: { children: React.ReactNode }) {
  const token = tokenManager.getToken()

  if (token) {
    return <Navigate to="/" replace />
  }

  return <>{children}</>
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
      <Routes>
        {/* Public routes */}
        <Route
          path="/login"
          element={
            <PublicRoute>
              <LoginPage />
            </PublicRoute>
          }
        />

        {/* Protected routes */}
        <Route
          path="/*"
          element={
            <ProtectedRoute>
              <div className="flex flex-col h-screen bg-background">
                <TopNav />
                <main className="flex-1 min-h-0 overflow-y-auto">
                  <Routes>
                    <Route path="/" element={<DashboardPage />} />
                    <Route path="/chat" element={<DashboardPage />} />
                    {/* Session-based routes */}
                    <Route path="/chat/:sessionId" element={<DashboardPage />} />
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
                    {/* Catch all - redirect to dashboard */}
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
      {/* Show toaster and confirmer on login page too */}
      <Toaster />
      <Confirmer />
    </>
  )
}

export default App

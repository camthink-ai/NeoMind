import { useEffect, useRef } from "react"
import { useStore } from "@/store"
import { Sidebar } from "@/components/layout/sidebar"
import { Header } from "@/components/layout/header"
import { LoginPage } from "@/pages/login"
import { DashboardPage } from "@/pages/dashboard"
import { DevicesPage } from "@/pages/devices"
import { AlertsPage } from "@/pages/alerts"
import { AutomationPage } from "@/pages/automation"
import { SettingsPage } from "@/pages/settings"
import { CommandsPage } from "@/pages/commands"
import { DecisionsPage } from "@/pages/decisions"
import { PluginsPage } from "@/pages/plugins"
import { Toaster } from "@/components/ui/toaster"

function App() {
  const { isAuthenticated, checkAuthStatus } = useStore()
  const currentPage = useStore((state) => state.currentPage)
  const previousAuthRef = useRef(isAuthenticated)

  // Check authentication status on mount
  useEffect(() => {
    checkAuthStatus()
  }, [checkAuthStatus])

  // Refresh WebSocket connections when authentication status changes
  useEffect(() => {
    // Only refresh when transitioning from unauthenticated to authenticated
    if (isAuthenticated && !previousAuthRef.current) {
      // Dynamic import to avoid SSR issues
      import('@/lib/events').then(({ refreshEventConnections }) => {
        refreshEventConnections()
      })
      // Also refresh chat WebSocket
      import('@/lib/websocket').then(({ ws }) => {
        ws.connect()
      })
    }
    previousAuthRef.current = isAuthenticated
  }, [isAuthenticated])

  const renderPage = () => {
    switch (currentPage) {
      case "dashboard":
        return <DashboardPage />
      case "devices":
        return <DevicesPage />
      case "alerts":
        return <AlertsPage />
      case "automation":
        return <AutomationPage />
      case "settings":
        return <SettingsPage />
      case "commands":
        return <CommandsPage />
      case "decisions":
        return <DecisionsPage />
      case "plugins":
        return <PluginsPage />
      default:
        return <DashboardPage />
    }
  }

  // Show login page if not authenticated
  // Note: user can be null when using API key authentication (dev mode)
  if (!isAuthenticated) {
    return (
      <>
        <LoginPage />
        <Toaster />
      </>
    )
  }

  return (
    <div className="flex h-screen bg-background">
      <Sidebar />
      <div className="flex flex-1 flex-col min-h-0">
        <Header />
        <main className="flex-1 overflow-y-auto">
          {renderPage()}
        </main>
      </div>
      <Toaster />
    </div>
  )
}

export default App

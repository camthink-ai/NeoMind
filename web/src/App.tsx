import { useEffect, useRef } from "react"
import { useStore } from "@/store"
import { TopNav } from "@/components/layout/TopNav"
import { LoginPage } from "@/pages/login"
import { DashboardPage } from "@/pages/dashboard"
import { DevicesPage } from "@/pages/devices"
import { AutomationPage } from "@/pages/automation"
import { SettingsPage } from "@/pages/settings"
import { CommandsPage } from "@/pages/commands"
import { DecisionsPage } from "@/pages/decisions"
import { PluginsPage } from "@/pages/plugins"
import { EventsPage } from "@/pages/events"
import { Toaster } from "@/components/ui/toaster"

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

function App() {
  const { isAuthenticated, checkAuthStatus, setWsConnected } = useStore()
  const currentPage = useStore((state) => state.currentPage)
  const previousAuthRef = useRef(isAuthenticated)
  const previousPageRef = useRef(currentPage)

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

  // Clean up portal content when page changes
  // This prevents Radix UI Portal cleanup errors
  useEffect(() => {
    if (previousPageRef.current !== currentPage) {
      // Small delay to ensure old page has unmounted
      const timer = setTimeout(() => {
        import('@/lib/portal').then(({ cleanupPortalContent }) => {
          cleanupPortalContent()
        })
      }, 100)
      previousPageRef.current = currentPage
      return () => clearTimeout(timer)
    }
  }, [currentPage])

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
    // Use key prop to ensure proper unmount/mount cycle when switching pages
    // This prevents Portal cleanup issues with Radix UI components
    switch (currentPage) {
      case "dashboard":
        return <DashboardPage key="dashboard" />
      case "devices":
        return <DevicesPage key="devices" />
      case "automation":
        return <AutomationPage key="automation" />
      case "settings":
        return <SettingsPage key="settings" />
      case "commands":
        return <CommandsPage key="commands" />
      case "decisions":
        return <DecisionsPage key="decisions" />
      case "plugins":
        return <PluginsPage key="plugins" />
      case "events":
        return <EventsPage key="events" />
      default:
        return <DashboardPage key="dashboard" />
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
    <div className="flex flex-col h-screen bg-background">
      <TopNav />
      <main className="flex-1 min-h-0 overflow-y-auto">
        {renderPage()}
      </main>
      <Toaster />
    </div>
  )
}

export default App

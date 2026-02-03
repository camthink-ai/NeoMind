import { useEffect, useState } from "react"
import { listen } from "@tauri-apps/api/event"
import { getVersion } from "@tauri-apps/api/app"
import { BrandLogoHorizontal } from "@/components/shared/BrandName"

interface BackendReadyEvent {
  status: "ready" | "timeout"
  port: number
}

/**
 * Startup loading screen for Tauri app.
 *
 * Shows a loading animation while waiting for the backend server to start.
 * The window is initially hidden in tauri.conf.json and shown by Rust
 * once the backend is ready.
 *
 * This component provides a smooth startup experience on all platforms,
 * especially Windows where backend startup can be slower.
 */
export function StartupLoading({ onReady }: { onReady: () => void }) {
  const [status, setStatus] = useState<"loading" | "ready" | "timeout">("loading")
  const [version, setVersion] = useState<string>("")

  useEffect(() => {
    let timeoutId: number | null = null

    const checkBackendStatus = async () => {
      try {
        // Get app version
        const ver = await getVersion()
        setVersion(ver)
      } catch {
        // Ignore version fetch errors
      }

      // The window is shown after Rust determines backend is ready,
      // but we should verify by checking the health endpoint directly.
      // This handles the race condition where the event might have been
      // emitted before the frontend was ready to listen.
      const checkHealth = async (): Promise<boolean> => {
        try {
          const response = await fetch('http://localhost:9375/api/health', {
            signal: AbortSignal.timeout(2000),
          })
          return response.ok
        } catch {
          return false
        }
      }

      // Listen for backend-ready event from Rust (for quick response)
      try {
        const unlisten = await listen<BackendReadyEvent>("backend-ready", (event) => {
          console.log("Backend ready event received:", event.payload)
          setStatus("ready")
          onReady()
        })

        // Also check health directly in case event was already sent
        let isReady = await checkHealth()
        if (isReady) {
          console.log("Backend health check passed immediately")
          setStatus("ready")
          onReady()
          unlisten()
          return
        }

        // Poll health endpoint for a few times in case event is missed
        for (let i = 0; i < 15; i++) {
          await new Promise(resolve => setTimeout(resolve, 200))
          isReady = await checkHealth()
          if (isReady) {
            console.log("Backend health check passed after polling")
            setStatus("ready")
            onReady()
            unlisten()
            return
          }
        }

        // If still not ready after polling, wait for event with timeout
        timeoutId = setTimeout(() => {
          console.log("Startup timeout - proceeding to app anyway")
          setStatus("timeout")
          onReady()
        }, 5000)
      } catch (error) {
        console.error("Error in startup check:", error)
        // Proceed anyway on error
        setStatus("timeout")
        onReady()
      }
    }

    checkBackendStatus()

    return () => {
      if (timeoutId) clearTimeout(timeoutId)
    }
  }, [onReady])

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-background overflow-hidden">
      {/* Animated background */}
      <div className="fixed inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
        <div className="absolute inset-0" style={{
          backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
          backgroundSize: '32px 32px'
        }} />
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
      </div>

      {/* Main content */}
      <div className="relative z-10 flex flex-col items-center gap-6">
        {/* Logo */}
        <BrandLogoHorizontal className="h-12" />

        {/* Loading animation */}
        <div className="flex items-center gap-3">
          <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '0ms' }} />
          <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '150ms' }} />
          <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '300ms' }} />
        </div>

        {/* Status text */}
        <p className="text-sm text-muted-foreground">
          {status === "loading" && "Starting backend service..."}
          {status === "ready" && "Service ready"}
          {status === "timeout" && "Connecting..."}
        </p>

        {/* Version info */}
        {version && (
          <p className="text-xs text-muted-foreground/50 mt-4">v{version}</p>
        )}
      </div>
    </div>
  )
}

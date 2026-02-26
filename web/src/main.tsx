import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import "./index.css"
import "./i18n/config"
import App from "./App"
import { ThemeProvider } from "@/components/ui/theme"
import { initVisualViewport } from "@/hooks/useVisualViewport"

// Initialize global VisualViewport tracking for mobile keyboard handling
initVisualViewport()

// Create a React Query client with optimized defaults
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000,  // 5 minutes - data is fresh for 5 min
      gcTime: 10 * 60 * 1000,    // 10 minutes - cache persists for 10 min
      retry: 2,                   // Retry failed requests twice
      refetchOnWindowFocus: false, // Don't refetch on window focus (reduces unnecessary requests)
      refetchOnReconnect: true,    // Refetch on reconnect (ensures data freshness)
    },
  },
})

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter future={{ v7_relativeSplatPath: true, v7_startTransition: true }}>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider>
          <App />
        </ThemeProvider>
      </QueryClientProvider>
    </BrowserRouter>
  </StrictMode>,
)

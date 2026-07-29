import { createContext, useContext, useEffect, useState } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { setTheme as setAppTheme } from "@tauri-apps/api/app"
import { isTauriEnv } from "@/lib/api"

type Theme = "dark" | "light" | "system"

interface ThemeContextType {
  theme: Theme
  setTheme: (theme: Theme) => void
  resolvedTheme: "dark" | "light"
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined)

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setTheme] = useState<Theme>("system")
  const [resolvedTheme, setResolvedTheme] = useState<"dark" | "light">(() => {
    // Detect system theme immediately to prevent flash
    if (typeof window !== "undefined") {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
    }
    return "light"
  })
  const [mounted, setMounted] = useState(false)

  // Get the actual theme (resolve "system" to dark or light)
  const getActualTheme = (preferredTheme: Theme): "dark" | "light" => {
    if (preferredTheme === "system") {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
    }
    return preferredTheme
  }

  useEffect(() => {
    setMounted(true)
    const stored = localStorage.getItem("theme") as Theme | null
    if (stored) {
      setTheme(stored)
    }

    // Apply theme immediately on mount to prevent flash
    const actualTheme = getActualTheme(stored || "system")
    const root = document.documentElement
    root.classList.remove("light", "dark")
    root.classList.add(actualTheme)
  }, [])

  // Update resolved theme when theme changes or system preference changes
  useEffect(() => {
    if (!mounted) return

    const updateResolvedTheme = () => {
      const actual = getActualTheme(theme)
      setResolvedTheme(actual)
    }

    updateResolvedTheme()

    // Listen for system theme changes when using "system" theme
    if (theme === "system") {
      const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)")
      const handler = () => updateResolvedTheme()
      mediaQuery.addEventListener("change", handler)
      return () => mediaQuery.removeEventListener("change", handler)
    }
  }, [theme, mounted])

  // Apply theme to document
  useEffect(() => {
    if (!mounted) return
    const root = document.documentElement
    root.classList.remove("light", "dark")
    root.classList.add(resolvedTheme)
    localStorage.setItem("theme", theme)
  }, [resolvedTheme, theme, mounted])

  // Sync the native window theme + background (Tauri title bar / launch flash)
  // to the resolved app theme so the OS chrome follows the app's light/dark
  // and there's no mismatched background flash. No-op in web mode.
  useEffect(() => {
    if (!isTauriEnv()) return
    const win = getCurrentWindow()
    // App-level theme sets NSApp appearance -> drives the macOS title bar.
    // (Window-level setTheme alone doesn't move the title bar on macOS.)
    setAppTheme(resolvedTheme).catch(() => {})
    win.setTheme(resolvedTheme).catch(() => {})
    // Match the app canvas (--background): oklch(0.975) light / oklch(0.135 0.01 270) dark.
    const bg: [number, number, number] =
      resolvedTheme === "dark" ? [20, 20, 27] : [245, 245, 245]
    win.setBackgroundColor(bg).catch(() => {})
  }, [resolvedTheme])

  // Don't block rendering - always show children with current theme
  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolvedTheme }}>
      {children}
    </ThemeContext.Provider>
  )
}

export function useTheme() {
  const context = useContext(ThemeContext)
  if (context === undefined) {
    throw new Error("useTheme must be used within ThemeProvider")
  }
  return context
}

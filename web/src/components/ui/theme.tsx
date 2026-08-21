import { createContext, useContext, useEffect, useState } from "react"
import { isTauriEnv } from "@/lib/api"

type Theme = "dark" | "light" | "system"

// Fresh-install default. Desktop defaults dark: the product is
// dark-first (dashboards/monitoring), and WKWebView's
// prefers-color-scheme is unreliable until the window appearance is
// explicitly set (see main.rs set_theme). Browsers detect correctly,
// so they keep following the system.
const defaultTheme: Theme = isTauriEnv() ? "dark" : "system"

interface ThemeContextType {
  theme: Theme
  setTheme: (theme: Theme) => void
  resolvedTheme: "dark" | "light"
}

const ThemeContext = createContext<ThemeContextType | undefined>(undefined)

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [theme, setTheme] = useState<Theme>(defaultTheme)
  const [resolvedTheme, setResolvedTheme] = useState<"dark" | "light">(() => {
    // Detect system theme immediately to prevent flash. Tauri default is
    // dark (see defaultTheme) so start there; browsers probe the real
    // system preference.
    if (isTauriEnv()) return "dark"
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
    const actualTheme = getActualTheme(stored || defaultTheme)
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

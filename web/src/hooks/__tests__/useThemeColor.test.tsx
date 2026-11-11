import { describe, it, expect, beforeEach, afterEach } from "vitest"
import { render, waitFor, cleanup } from "@testing-library/react"
import { ThemeProvider } from "@/components/ui/theme"
import { ThemeColorSync, useThemeColor } from "@/hooks/useThemeColor"

function OverlayThemeColor({ active }: { active: boolean }) {
  useThemeColor("popover", active)
  return null
}

describe("useThemeColor", () => {
  let lightMeta: HTMLMetaElement
  let darkMeta: HTMLMetaElement

  beforeEach(() => {
    localStorage.setItem("theme", "dark")
    document.documentElement.classList.remove("light", "dark")
    document.documentElement.classList.add("dark")
    document.documentElement.style.setProperty("--background", "oklch(0.135 0.01 270 / 97%)")
    document.documentElement.style.setProperty("--popover", "oklch(0.19 0.01 270)")

    lightMeta = document.createElement("meta")
    lightMeta.name = "theme-color"
    lightMeta.media = "(prefers-color-scheme: light)"
    lightMeta.content = "#ffffff"
    darkMeta = document.createElement("meta")
    darkMeta.name = "theme-color"
    darkMeta.media = "(prefers-color-scheme: dark)"
    darkMeta.content = "#000000"
    document.head.append(lightMeta, darkMeta)
  })

  afterEach(() => {
    cleanup()
    lightMeta.remove()
    darkMeta.remove()
    localStorage.removeItem("theme")
    document.documentElement.style.removeProperty("--background")
    document.documentElement.style.removeProperty("--popover")
  })

  it("updates every theme-color meta from --background", async () => {
    render(
      <ThemeProvider>
        <ThemeColorSync />
      </ThemeProvider>,
    )

    await waitFor(() => {
      expect(lightMeta.getAttribute("content")).toBe("#07080c")
      expect(darkMeta.getAttribute("content")).toBe("#07080c")
    })
  })

  it("restores prior values when an overlay theme-color deactivates", async () => {
    const { rerender } = render(
      <ThemeProvider>
        <ThemeColorSync />
        <OverlayThemeColor active />
      </ThemeProvider>,
    )

    await waitFor(() => {
      expect(darkMeta.getAttribute("content")).toBe("#121418")
    })

    rerender(
      <ThemeProvider>
        <ThemeColorSync />
        <OverlayThemeColor active={false} />
      </ThemeProvider>,
    )

    await waitFor(() => {
      // Overlay restores the values ThemeColorSync had written.
      expect(lightMeta.getAttribute("content")).toBe("#07080c")
      expect(darkMeta.getAttribute("content")).toBe("#07080c")
    })
  })
})

import { useEffect } from "react"
import { useTheme } from "@/components/ui/theme"

/**
 * useThemeColor — sync the PWA `theme-color` meta with a full-screen overlay's
 * effective background so the status-bar / safe-area strip matches the overlay
 * body on iOS/Android. Without it, the notch area keeps the base theme-color
 * (light/dark media fallbacks in index.html; runtime sync uses --background)
 * and reads as a colored band above dialogs whose surface differs
 * (bg-popover = white, etc.).
 *
 * Accepts a CSS var name WITHOUT the `--` prefix (e.g. "popover", "bg-90") or a
 * literal color. Resolves the var to its current value, converts oklch/rgb to
 * hex, and — for semi-transparent tokens — composites over `--background` so the
 * emitted color is opaque (theme-color with alpha is unreliable on iOS).
 * Updates every `meta[name=theme-color]` (light + dark media variants).
 * On unmount / inactive restores the previous theme-color values.
 */

function oklchToHex(L: number, C: number, H: number, alpha: number): { rgb: [number, number, number]; a: number } {
  // oklch → oklab
  const h = (H * Math.PI) / 180
  const a = C * Math.cos(h)
  const b = C * Math.sin(h)
  // oklab → LMS → linear sRGB
  const l_ = L + 0.3963377774 * a + 0.2158037573 * b
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b
  const s_ = L - 0.0894841775 * a - 1.291485548 * b
  const l = l_ ** 3
  const m = m_ ** 3
  const s = s_ ** 3
  let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s
  let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s
  let bb = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s
  const toSrgb = (c: number) => {
    c = Math.min(1, Math.max(0, c))
    return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055
  }
  r = toSrgb(r)
  g = toSrgb(g)
  bb = toSrgb(bb)
  return { rgb: [r * 255, g * 255, bb * 255], a: alpha }
}

function parseColor(raw: string): { rgb: [number, number, number]; a: number } | null {
  raw = raw.trim()
  // #rgb / #rrggbb / #rrggbbaa
  let m = raw.match(/^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/)
  if (m) {
    let hex = m[1]
    if (hex.length === 3) hex = hex.split("").map((c) => c + c).join("")
    const r = parseInt(hex.slice(0, 2), 16)
    const g = parseInt(hex.slice(2, 4), 16)
    const b = parseInt(hex.slice(4, 6), 16)
    const a = hex.length === 8 ? parseInt(hex.slice(6, 8), 16) / 255 : 1
    return { rgb: [r, g, b], a }
  }
  // rgb()/rgba()
  m = raw.match(/rgba?\(\s*([\d.]+)\s*[, ]\s*([\d.]+)\s*[, ]\s*([\d.]+)\s*(?:[,/]\s*([\d.]+%?))?\s*\)/)
  if (m) {
    const a = m[4] ? (m[4].endsWith("%") ? parseFloat(m[4]) / 100 : parseFloat(m[4])) : 1
    return { rgb: [parseFloat(m[1]), parseFloat(m[2]), parseFloat(m[3])], a }
  }
  // oklch(L C H / alpha) or oklch(L C H)
  m = raw.match(/oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*(?:\/\s*([\d.]+%?))?\s*\)/)
  if (m) {
    let alpha = m[4] ? parseFloat(m[4]) : 1
    if (m[4]?.endsWith("%")) alpha = alpha / 100
    return oklchToHex(parseFloat(m[1]), parseFloat(m[2]), parseFloat(m[3]), alpha)
  }
  return null
}

function toHex({ rgb, a: _a }: { rgb: [number, number, number]; a: number }): string {
  return (
    "#" +
    rgb.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0")).join("")
  )
}

export function useThemeColor(
  tokenOrHex: string,
  active: boolean = true,
): void {
  const { resolvedTheme } = useTheme()

  useEffect(() => {
    if (!active) return

    // index.html ships light+dark media variants. querySelector alone would
    // only ever update the first (light) tag, leaving the dark-mode status
    // bar / safe-area strip stuck on the static fallback (#000).
    const metas = Array.from(
      document.querySelectorAll<HTMLMetaElement>('meta[name="theme-color"]'),
    )
    if (metas.length === 0) return
    const prev = metas.map((meta) => meta.getAttribute("content"))

    // Resolve token → raw CSS var value (e.g. "popover" → "oklch(1 0 0)").
    let raw = tokenOrHex
    if (!tokenOrHex.startsWith("#") && !tokenOrHex.startsWith("oklch") && !tokenOrHex.startsWith("rgb")) {
      raw = getComputedStyle(document.documentElement).getPropertyValue(`--${tokenOrHex}`).trim() || tokenOrHex
    }
    const color = parseColor(raw)
    let hex = color ? toHex(color) : raw

    // Composite semi-transparent token over the app background so theme-color
    // stays opaque and matches what the frosted overlay visually reads as.
    // When the token IS --background (already translucent), keep the opaque
    // oklch rgb from toHex above — blending it over itself is a no-op, but
    // blending over a mismatched canvas would tint the status bar wrong.
    if (color && color.a < 1 && tokenOrHex !== "background") {
      // Underlay uses the opaque channel of --background (dark canvas is
      // translucent too; its rgb is the charcoal we want beneath overlays).
      const bg = parseColor(getComputedStyle(document.documentElement).getPropertyValue("--background").trim()) ?? { rgb: [247, 247, 247], a: 1 }
      const blended: [number, number, number] = [
        color.rgb[0] * color.a + bg.rgb[0] * (1 - color.a),
        color.rgb[1] * color.a + bg.rgb[1] * (1 - color.a),
        color.rgb[2] * color.a + bg.rgb[2] * (1 - color.a),
      ]
      hex = toHex({ rgb: blended, a: 1 })
    }

    for (const meta of metas) {
      meta.setAttribute("content", hex)
    }
    return () => {
      metas.forEach((meta, i) => {
        const previous = prev[i]
        if (previous === null) {
          meta.removeAttribute("content")
        } else {
          meta.setAttribute("content", previous)
        }
      })
    }
  }, [tokenOrHex, active, resolvedTheme])
}

/** Keep PWA/OS theme-color aligned with --background for the active app theme. */
export function ThemeColorSync() {
  useThemeColor("background", true)
  return null
}

import { getServerOrigin } from '@/lib/api'

// Check if a string is an image (base64, data URL, or /api/images/ URL)
export function isBase64Image(value: unknown): boolean {
  if (typeof value !== "string") return false
  const str = value.trim()

  // API image URL (file-backed storage)
  if (str.startsWith("/api/images/")) return true
  // HTTP(S) image URL
  if (str.startsWith("http")) return true
  // data URL prefix
  if (str.startsWith("data:image/")) return true

  // Check for raw base64 with known image signatures
  if (str.length < 100) return false

  const imageSignatures = [
    "iVBORw0KGgo",  // PNG
    "/9j/",          // JPEG
    "R0lGODlh",      // GIF
    "UklGR",         // WebP
    "Qk",            // BMP
  ]
  return imageSignatures.some(sig => str.startsWith(sig))
}

// Strip whitespace/newlines from base64 data
function sanitizeBase64(data: string): string {
  return data.replace(/[\s\r\n]+/g, '')
}

// Get the data URL for a base64 image value.
// Handles: data:image/ URLs (with double-prefix unwrap), raw base64.
export function getImageDataUrl(value: unknown): string | null {
  if (typeof value !== "string") return null
  const str = value.trim()
  if (!str) return null

  // API image URL — prepend server origin
  if (str.startsWith('/api/images/')) {
    return getServerOrigin() + str
  }

  // Already a data:image URL
  if (str.startsWith("data:image/")) {
    const commaIdx = str.indexOf(',')
    if (commaIdx === -1) return str
    const b64 = sanitizeBase64(str.slice(commaIdx + 1))

    // Unwrap double-prefixed data URLs
    if (b64.startsWith('data:image/') || b64.startsWith('data:')) {
      return getImageDataUrl(b64)
    }

    return str.slice(0, commaIdx + 1) + b64
  }

  // Any other data: prefix (malformed) — strip and retry
  if (str.startsWith('data:')) {
    const afterComma = str.slice(str.indexOf(',') + 1)
    if (afterComma && afterComma !== str) return getImageDataUrl(afterComma)
    return null
  }

  // Raw base64 — detect format from signature
  const clean = sanitizeBase64(str)
  if (clean.startsWith("iVBORw0KGgo")) return `data:image/png;base64,${clean}`
  if (clean.startsWith("/9j/")) return `data:image/jpeg;base64,${clean}`
  if (clean.startsWith("R0lGODlh")) return `data:image/gif;base64,${clean}`
  if (clean.startsWith("UklGR")) return `data:image/webp;base64,${clean}`
  if (clean.startsWith("Qk")) return `data:image/bmp;base64,${clean}`

  // Fallback: long enough and looks like base64 → try as PNG
  if (clean.length >= 100 && /^[A-Za-z0-9+/=_-]+$/.test(clean)) {
    return `data:image/png;base64,${clean}`
  }

  return null
}


// Check if a string is a base64-encoded image
export function isBase64Image(value: unknown): boolean {
  if (typeof value !== "string") return false
  const str = value.trim()

  // Check for data URL prefix
  if (str.startsWith("data:image/")) return true

  // Check for base64 pattern that looks like an image
  const imageSignatures = [
    "iVBORw0KGgo",  // PNG
    "/9j/",          // JPEG
    "R0lGODlh",      // GIF
    "UklGR",         // WebP
    "Qk",            // BMP
  ]

  // Base64 string should be reasonably long and have valid padding
  if (str.length < 100) return false
  if (!/^[A-Za-z0-9+/=]+$/.test(str)) return false

  // Check for known image signatures
  return imageSignatures.some(sig => str.startsWith(sig))
}

// Get the data URL for a base64 image value
export function getImageDataUrl(value: unknown): string | null {
  if (typeof value !== "string") return null
  const str = value.trim()

  // Already has data URL prefix
  if (str.startsWith("data:image/")) return str

  // Detect image type from signature and add data URL prefix
  if (str.startsWith("iVBORw0KGgo")) return `data:image/png;base64,${str}`
  if (str.startsWith("/9j/")) return `data:image/jpeg;base64,${str}`
  if (str.startsWith("R0lGODlh")) return `data:image/gif;base64,${str}`
  if (str.startsWith("UklGR")) return `data:image/webp;base64,${str}`
  if (str.startsWith("Qk")) return `data:image/bmp;base64,${str}`

  // Fallback - try as PNG
  return `data:image/png;base64,${str}`
}

// Format metric value for display
export function formatMetricValue(value: unknown, dataType?: string, t?: (key: string) => string): string {
  if (value === null || value === undefined) return "-"
  if (typeof value === "boolean") return value ? (t?.('devices.yes') || "是") : (t?.('devices.no') || "否")
  if (typeof value === "number") {
    if (dataType === "integer") {
      return value.toLocaleString("zh-CN")
    }
    return parseFloat(value.toFixed(2)).toString()
  }
  // For base64 images, show a placeholder text
  if (isBase64Image(value)) return t?.('devices.image') || "[图片]"
  return String(value)
}

// Check if value is a base64 image for rendering
export function isImageValue(value: unknown): boolean {
  return typeof value === "string" && isBase64Image(value)
}

// Generate device type ID from name
export function generateDeviceTypeId(name: string): string {
  return name.toLowerCase()
    .replace(/\s+/g, "_")
    .replace(/[^a-z0-9_]/g, "")
    .replace(/_+/g, "_")
    .replace(/^_|_$/g, "")
}

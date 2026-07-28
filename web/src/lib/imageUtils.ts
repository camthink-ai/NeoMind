/**
 * Shared image utility functions.
 *
 * Extracted from ImageDisplay.tsx and ImageHistory.tsx to eliminate duplication.
 * Contains base64 detection, image format identification, and URL normalization.
 */

import { getServerOrigin } from '@/lib/api'

// ============================================================================
// Types
// ============================================================================

export type ImageFormatType = 'png' | 'jpeg' | 'jpg' | 'gif' | 'webp' | 'bmp' | 'svg' | 'tiff' | 'ico' | 'unknown'

export interface NormalizedImage {
  src: string
  format: ImageFormatType
  isBase64: boolean
  isDataUrl: boolean
  originalValue: string
}

// ============================================================================
// Constants
// ============================================================================

/** Magic bytes for image type detection */
export const IMAGE_MAGIC_BYTES: Record<string, { magic: number[]; type: ImageFormatType; mime: string }> = {
  png: { magic: [0x89, 0x50, 0x4E, 0x47], type: 'png', mime: 'image/png' },
  jpeg: { magic: [0xFF, 0xD8, 0xFF], type: 'jpeg', mime: 'image/jpeg' },
  gif: { magic: [0x47, 0x49, 0x46], type: 'gif', mime: 'image/gif' },
  webp: { magic: [0x52, 0x49, 0x46, 0x46], type: 'webp', mime: 'image/webp' },
  bmp: { magic: [0x42, 0x4D], type: 'bmp', mime: 'image/bmp' },
  tiff: { magic: [0x49, 0x49, 0x2A, 0x00], type: 'tiff', mime: 'image/tiff' },
  ico: { magic: [0x00, 0x00, 0x01, 0x00], type: 'ico', mime: 'image/x-icon' },
}

// ============================================================================
// Format detection
// ============================================================================

/** Detect image format from the first few bytes of decoded base64 data. */
export function detectImageFormatFromMagicBytes(base64Data: string): { type: ImageFormatType; mime: string } | null {
  try {
    const pureBase64 = base64Data.replace(/^data:image\/[^;]+;base64,/, '').replace(/^data:,/, '')
    const binaryString = atob(pureBase64.slice(0, 32))
    for (const info of Object.values(IMAGE_MAGIC_BYTES)) {
      if (info.magic.every((byte, i) => binaryString.charCodeAt(i) === byte)) {
        return { type: info.type, mime: info.mime }
      }
    }
  } catch { /* invalid base64 */ }
  return null
}

/** Check if a string looks like pure base64 (no data: prefix, no URL). */
export function isPureBase64(str: string): boolean {
  if (!str || str.length < 100) return false
  const cleaned = str.trim().replace(/[\s\r\n]+/g, '')
  if (cleaned.startsWith('http://') || cleaned.startsWith('https://') || cleaned.startsWith('data:')) return false
  if (!/^[A-Za-z0-9+/=_-]+$/.test(cleaned)) return false
  try { atob(cleaned.slice(0, 100)); return true } catch { return false }
}

// ============================================================================
// URL normalization (memoized via module-level cache)
// ============================================================================

const DEFAULT_FORMAT = { type: 'png' as ImageFormatType, mime: 'image/png' }

/** Cache for normalizeImageUrl to avoid re-decoding base64 on every render. */
const normalizeCache = new Map<string, NormalizedImage | null>()
const CACHE_MAX_SIZE = 32
// Skip caching for inputs larger than 10KB (likely base64 camera frames)
// to avoid retaining multi-MB strings as Map keys.
const CACHE_MAX_INPUT_LENGTH = 10_000

/**
 * Normalize various image formats to a standard data URL.
 * Results are cached by originalValue string for O(1) repeat lookups.
 * Large inputs (likely base64) are computed but NOT cached to avoid memory pressure.
 */
export function normalizeImageUrl(value: string | number | undefined | null): NormalizedImage | null {
  if (value === null || value === undefined) return null
  const valueStr = String(value)
  const trimmed = valueStr.trim()

  if (!trimmed || trimmed === '-' || trimmed === 'undefined' || trimmed === 'null') return null
  if (trimmed.includes('via.placeholder.com') || trimmed.includes('placehold.co')) return null

  // Skip cache for large inputs (base64 images) to avoid memory bloat
  const shouldCache = trimmed.length <= CACHE_MAX_INPUT_LENGTH

  // Check cache first (before expensive atob)
  if (shouldCache) {
    const cached = normalizeCache.get(trimmed)
    if (cached !== undefined) return cached
  }

  const result = computeNormalizedImage(trimmed, valueStr)

  if (shouldCache) {
    // Evict oldest entries when cache is full
    if (normalizeCache.size >= CACHE_MAX_SIZE) {
      const firstKey = normalizeCache.keys().next().value
      if (firstKey !== undefined) normalizeCache.delete(firstKey)
    }
    normalizeCache.set(trimmed, result)
  }
  return result
}

/** MIME string → ImageFormatType lookup (avoids scanning IMAGE_MAGIC_BYTES). */
const MIME_TO_FORMAT: Record<string, ImageFormatType> = {
  'png': 'png', 'jpeg': 'jpeg', 'jpg': 'jpg', 'gif': 'gif',
  'webp': 'webp', 'bmp': 'bmp', 'svg': 'svg', 'tiff': 'tiff', 'ico': 'ico',
}

/** Inner computation — separated for caching. */
function computeNormalizedImage(trimmed: string, valueStr: string): NormalizedImage | null {
  // 0. API image URL (/api/images/...) — stored as file, serve via authenticated route.
  //    Must prepend server origin (frontend ≠ API origin in Tauri/Vite dev).
  if (trimmed.startsWith('/api/images/')) {
    return {
      src: getServerOrigin() + trimmed,
      format: 'unknown',
      isBase64: false,
      isDataUrl: false,
      originalValue: valueStr,
    }
  }

  // 1. Proper data:image/ URL
  if (trimmed.startsWith('data:image/')) {
    // Fast path: already a well-formed base64 data URL — skip expensive
    // slice/regex/concat on potentially multi-MB strings.
    if (trimmed.includes(';base64,')) {
      const mimeMatch = trimmed.match(/^data:image\/([^;,]+)/i)
      const mimeStr = mimeMatch?.[1]?.toLowerCase()
      return {
        src: trimmed,
        format: (mimeStr ? MIME_TO_FORMAT[mimeStr] : undefined) ?? 'unknown',
        isBase64: true,
        isDataUrl: true,
        originalValue: valueStr,
      }
    }
    const commaIdx = trimmed.indexOf(',')
    const b64 = commaIdx !== -1 ? trimmed.slice(commaIdx + 1).replace(/[\s\r\n]+/g, '') : ''
    if (b64.startsWith('data:image/') || b64.startsWith('data:')) {
      const unwrapped = normalizeImageUrl(b64)
      return unwrapped
    }
    const detectedFormat = detectImageFormatFromMagicBytes(b64) || DEFAULT_FORMAT
    const mimeMatch = trimmed.match(/data:image\/([^;,]+)/i)
    const declaredMime = mimeMatch ? `image/${mimeMatch[1].toLowerCase()}` : 'image/png'
    const finalMime = detectedFormat.mime || declaredMime
    return { src: `data:${finalMime};base64,${b64}`, format: detectedFormat.type, isBase64: true, isDataUrl: true, originalValue: valueStr }
  }

  // 2. data:base64, prefix
  if (trimmed.startsWith('data:base64,')) {
    const b64 = trimmed.slice(12).replace(/[\s\r\n]+/g, '')
    const fmt = detectImageFormatFromMagicBytes(b64) || DEFAULT_FORMAT
    return { src: `data:${fmt.mime};base64,${b64}`, format: fmt.type, isBase64: true, isDataUrl: true, originalValue: valueStr }
  }

  // 3. Non-standard data: prefix
  if (trimmed.startsWith('data:')) {
    const commaIdx = trimmed.indexOf(',')
    const b64 = commaIdx !== -1 ? trimmed.slice(commaIdx + 1).replace(/[\s\r\n]+/g, '') : ''
    const fmt = detectImageFormatFromMagicBytes(b64) || DEFAULT_FORMAT
    return { src: `data:${fmt.mime};base64,${b64}`, format: fmt.type, isBase64: true, isDataUrl: true, originalValue: valueStr }
  }

  // 4. Pure base64
  if (isPureBase64(trimmed)) {
    const clean = trimmed.replace(/[\s\r\n]+/g, '')
    const fmt = detectImageFormatFromMagicBytes(clean) || DEFAULT_FORMAT
    return { src: `data:${fmt.mime};base64,${clean}`, format: fmt.type, isBase64: true, isDataUrl: true, originalValue: valueStr }
  }

  // 5. HTTP(S) URL
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    return { src: trimmed, format: 'unknown', isBase64: false, isDataUrl: false, originalValue: valueStr }
  }

  // 6. Relative URL
  if (trimmed.startsWith('/')) {
    return { src: trimmed, format: 'unknown', isBase64: false, isDataUrl: false, originalValue: valueStr }
  }

  // 7. URL without protocol
  if (trimmed.includes('.') && !trimmed.includes(' ')) {
    return { src: trimmed.startsWith('//') ? `https:${trimmed}` : `https://${trimmed}`, format: 'unknown', isBase64: false, isDataUrl: false, originalValue: valueStr }
  }

  // 8. Last resort: raw base64
  const sanitized = trimmed.replace(/[\s\r\n]+/g, '')
  if (sanitized.length >= 100 && /^[A-Za-z0-9+/=_-]+$/.test(sanitized)) {
    const fmt = detectImageFormatFromMagicBytes(sanitized) || DEFAULT_FORMAT
    return { src: `data:${fmt.mime};base64,${sanitized}`, format: fmt.type, isBase64: true, isDataUrl: true, originalValue: valueStr }
  }

  return null
}

/** Extract the src string from a normalized image result (null-safe). */
export function toNormalizedSrc(value: string | number | undefined | null): string | null {
  return normalizeImageUrl(value)?.src ?? null
}

/**
 * Resolve a value to a previewable `<img src>`, or `null` if it isn't an image.
 *
 * Unlike `normalizeImageUrl` (which normalizes ANY url-like string and returns
 * non-null for arbitrary URLs), this is a strict image check used to decide
 * whether to render an image preview vs. treat the value as text. Only these
 * qualify:
 *   - base64 / `data:` URLs
 *   - backend image-store URLs (`/api/images/...`, relative or absolute)
 *   - HTTP(S) URLs ending in a common image extension
 *
 * Post image-url-migration, device image metrics arrive as `/api/images/` URLs
 * instead of inline base64, so preview paths must recognise them.
 */
export function resolveImageSrc(value: string | number | undefined | null): string | null {
  if (value === null || value === undefined) return null
  const v = String(value).trim()
  if (!v) return null
  const n = normalizeImageUrl(v)
  if (!n) return null
  // base64 / data URL — always an image
  if (n.isBase64 || n.isDataUrl) return n.src
  // backend image store (normalizeImageUrl already prepended the server origin
  // for relative /api/images/ paths; absolute http://host/api/images/ passes through)
  if (v.includes('/api/images/')) return n.src
  // HTTP(S) image URL by extension
  if (/^https?:\/\/[^\s]+\.(png|jpe?g|gif|webp|bmp|svg)([?#]|$)/i.test(v)) return n.src
  return null
}

// ============================================================================
// Value extraction
// ============================================================================

/**
 * Extract the most likely image value from various data shapes.
 * Handles arrays (picks latest by timestamp), objects (checks common field names),
 * and primitive values.
 */
export function extractImageValue(value: unknown): string {
  if (value === null || value === undefined) return ''
  if (typeof value === 'string') return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return String(value)

  if (Array.isArray(value)) {
    if (value.length === 0) return ''
    let latestItem = value[0]
    let latestTime = -1
    for (const item of value) {
      if (typeof item === 'object' && item !== null) {
        const obj = item as Record<string, unknown>
        const time = (obj.time ?? obj.timestamp ?? obj.t ?? 0) as number
        if (time > latestTime) { latestTime = time; latestItem = item }
      }
    }
    return extractImageValue(latestItem)
  }

  if (typeof value === 'object') {
    const obj = value as Record<string, unknown>
    for (const key of ['src', 'url', 'image', 'imageUrl', 'image_url', 'data', 'value', 'content']) {
      if (obj[key] !== null && obj[key] !== undefined) {
        const extracted = extractImageValue(obj[key])
        if (extracted && extracted !== '-') return extracted
      }
    }
  }

  return ''
}

/** Extract timestamp (in ms) from a telemetry point object. */
export function extractTimestamp(obj: Record<string, unknown>): number | undefined {
  const ts = obj.timestamp ?? obj.time ?? obj.t
  if (ts === undefined || ts === null) return undefined
  if (typeof ts === 'number') return ts > 10000000000 ? ts : ts * 1000
  if (typeof ts === 'string') { const parsed = Date.parse(ts); if (!isNaN(parsed)) return parsed }
  return undefined
}

/** Get file extension from format type. */
export function getFileExtension(format: ImageFormatType): string {
  const ext: Record<ImageFormatType, string> = {
    png: 'png', jpeg: 'jpg', jpg: 'jpg', gif: 'gif', webp: 'webp',
    bmp: 'bmp', svg: 'svg', tiff: 'tiff', ico: 'ico', unknown: 'png',
  }
  return ext[format] || 'png'
}

// ============================================================================
// Image compression for uploads
// ============================================================================

const COMPRESS_MAX_DIMENSION = 1200
const COMPRESS_TARGET_BYTES = 150 * 1024 // 150KB target

/**
 * Compress an image file to a compact data URL suitable for storing in
 * dashboard config. Resizes to fit within 1200px and reduces JPEG quality
 * to target ~150KB output. Small images are returned as-is.
 */
export function compressImageFile(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    if (!file.type.startsWith('image/')) {
      reject(new Error('not_an_image'))
      return
    }

    const url = URL.createObjectURL(file)
    const img = new Image()
    img.onload = () => {
      URL.revokeObjectURL(url)

      // Skip compression for tiny images
      if (img.width <= COMPRESS_MAX_DIMENSION && img.height <= COMPRESS_MAX_DIMENSION && file.size <= COMPRESS_TARGET_BYTES) {
        const reader = new FileReader()
        reader.onload = (e) => resolve(e.target?.result as string)
        reader.onerror = () => reject(new Error('read_failed'))
        reader.readAsDataURL(file)
        return
      }

      let { width, height } = img
      if (width > COMPRESS_MAX_DIMENSION || height > COMPRESS_MAX_DIMENSION) {
        const ratio = Math.min(COMPRESS_MAX_DIMENSION / width, COMPRESS_MAX_DIMENSION / height)
        width = Math.round(width * ratio)
        height = Math.round(height * ratio)
      }

      const canvas = document.createElement('canvas')
      canvas.width = width
      canvas.height = height
      const ctx = canvas.getContext('2d')
      if (!ctx) { reject(new Error('canvas_failed')); return }
      ctx.drawImage(img, 0, 0, width, height)

      // Try quality levels until under target
      let quality = 0.8
      let dataUrl = canvas.toDataURL('image/jpeg', quality)
      while (dataUrl.length > COMPRESS_TARGET_BYTES * 1.37 && quality > 0.2) {
        quality -= 0.15
        dataUrl = canvas.toDataURL('image/jpeg', quality)
      }

      resolve(dataUrl)
    }
    img.onerror = () => {
      URL.revokeObjectURL(url)
      reject(new Error('load_failed'))
    }
    img.src = url
  })
}

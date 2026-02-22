/**
 * Generate a unique identifier.
 *
 * Uses crypto.randomUUID() if available (modern browsers, secure context),
 * otherwise falls back to a Math.random() based implementation.
 *
 * This ensures compatibility across all environments including:
 * - Non-HTTPS contexts (HTTP, localhost)
 * - Older browsers
 * - Node.js environments without crypto support
 */
export function generateId(): string {
  // Try crypto.randomUUID() first (most reliable)
  if (typeof crypto !== 'undefined' && crypto.randomUUID && typeof crypto.randomUUID === 'function') {
    try {
      return crypto.randomUUID()
    } catch {
      // Fall through to fallback
    }
  }

  // Fallback: generate a random string using Math.random()
  // Format: id_<timestamp>_<random_string>
  const timestamp = Date.now().toString(36)
  const random = Math.random().toString(36).substring(2, 15)
  return `id_${timestamp}_${random}`
}

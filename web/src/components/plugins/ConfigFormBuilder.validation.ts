// MQTT topic validators — split from ConfigFormBuilder.tsx (component
// files may only export components for react-refresh).

/**
 * Validate MQTT topic pattern
 * Rules:
 * - # (multi-level wildcard) must be at the end only
 * - + (single-level wildcard) can replace a single level
 * - Topic segments cannot be empty (except after #)
 * - No spaces allowed
 */
export function validateMqttTopic(topic: string): { valid: boolean; error?: string } {
  const trimmed = topic.trim()

  if (!trimmed) {
    return { valid: false, error: 'Topic cannot be empty' }
  }

  // Check for spaces
  if (trimmed.includes(' ')) {
    return { valid: false, error: 'Topic cannot contain spaces' }
  }

  const segments = trimmed.split('/')

  // Check each segment
  for (let i = 0; i < segments.length; i++) {
    const segment = segments[i]

    // Empty segment (consecutive slashes)
    if (!segment) {
      return { valid: false, error: `Empty segment at position ${i + 1} (consecutive slashes)` }
    }

    // Multi-level wildcard (#) - must be last and only segment
    if (segment === '#') {
      if (i !== segments.length - 1) {
        return { valid: false, error: '# wildcard must be the last segment' }
      }
      // Valid - can exit early since # is last
      return { valid: true }
    }

    // Single-level wildcard (+) - valid
    if (segment === '+') {
      continue
    }

    // Check for invalid characters (only allow alphanumeric, -, _, ., :)
    // MQTT spec allows most printable characters except # + and control chars
    if (segment.includes('#') || segment.includes('+')) {
      return { valid: false, error: `Invalid use of wildcards in segment "${segment}"` }
    }
  }

  return { valid: true }
}

/**
 * Validate multiple MQTT topics (one per line)
 */
export function validateMqttTopics(topics: string[]): { valid: boolean; errors: string[] } {
  const errors: string[] = []

  for (let i = 0; i < topics.length; i++) {
    const topic = topics[i]
    if (!topic.trim()) continue // Skip empty lines

    const result = validateMqttTopic(topic)
    if (!result.valid) {
      errors.push(`Line ${i + 1}: ${result.error}`)
    }
  }

  return { valid: errors.length === 0, errors }
}

/**
 * Helpers for command-parameter option lists and data_type normalization.
 *
 * Backend MetricDataType::Enum serializes as `{ enum: { options: string[] } }`.
 * The UI historically expected `{ enum: string[] }` (and plain `allowed_values`
 * string arrays). Treating the object form as an array caused
 * `o.map is not a function` when opening a command dialog.
 *
 * Device-type JSON (marketplace / imports) often uses TitleCase `"Integer"`;
 * runtime `/current` after serde is lowercase. `normalizeDataType` collapses both.
 */

import type { ParameterDataType, ParameterDefinition } from '@/types/device'

const TAGGED_METRIC_KEY =
  /^(String|string|Integer|integer|Float|float|Boolean|boolean|Null|null|Array|array|Binary|binary)$/

/**
 * Unwrap tagged MetricValue objects like `{ String: "white" }` /
 * `{ Array: [{ Integer: 1 }] }`. Array/Binary unwrap recursively so seeded
 * defaults become plain JS arrays / base64 strings for the form.
 */
export function unwrapMetricValue(v: unknown): unknown {
  if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
    const obj = v as Record<string, unknown>
    const keys = Object.keys(obj)
    if (keys.length === 1) {
      const k = keys[0]
      if (TAGGED_METRIC_KEY.test(k)) {
        const inner = obj[k]
        if (/^(Array|array)$/.test(k) && Array.isArray(inner)) {
          return inner.map(unwrapMetricValue)
        }
        return inner
      }
    }
  }
  if (Array.isArray(v)) {
    return v.map(unwrapMetricValue)
  }
  return v
}

/** Unicode scalar count — matches Rust `chars().count()` for Length rules. */
export function unicodeScalarLength(s: string): number {
  return [...s].length
}

/**
 * Truncate to at most `max` Unicode scalars (not UTF-16 units).
 * Use this instead of native `maxLength`, which counts UTF-16 and blocks
 * valid supplementary-plane input (emoji) before validation runs.
 */
export function truncateToUnicodeScalars(s: string, max: number): string {
  if (max < 0) return s
  const chars = [...s]
  if (chars.length <= max) return s
  return chars.slice(0, max).join('')
}

/**
 * Resolve enum option strings from either frontend shape `{ enum: string[] }`
 * or backend shape `{ enum: { options: string[] } }`.
 */
export function getEnumOptions(
  dataType: ParameterDataType | undefined | null,
): string[] | null {
  if (!dataType || typeof dataType !== 'object' || !('enum' in dataType)) {
    return null
  }
  const e = (dataType as { enum: unknown }).enum
  if (Array.isArray(e) && e.every((x) => typeof x === 'string')) {
    return e as string[]
  }
  if (
    e !== null &&
    typeof e === 'object' &&
    Array.isArray((e as { options?: unknown }).options)
  ) {
    const opts = (e as { options: unknown[] }).options
    if (opts.every((x) => typeof x === 'string')) {
      return opts as string[]
    }
  }
  return null
}

/** Flatten allowed_values to plain JS values for Select / defaults. */
export function getAllowedValues(
  param: Pick<ParameterDefinition, 'allowed_values'>,
): unknown[] | null {
  if (!param.allowed_values || param.allowed_values.length === 0) {
    return null
  }
  return param.allowed_values.map(unwrapMetricValue)
}

/**
 * True for plain `"array"` or backend object form
 * `{ array: { element_type?: ... } }` / `{ Array: ... }`.
 */
export function isArrayDataType(
  dataType: ParameterDataType | undefined | null,
): boolean {
  const n = normalizeDataType(dataType)
  if (n === 'array') return true
  if (!n || typeof n !== 'object') return false
  return 'array' in n
}

/** Scalar kind after normalization (enum/array objects → null). */
export function scalarDataType(
  dataType: ParameterDataType | undefined | null,
): 'integer' | 'float' | 'string' | 'boolean' | 'binary' | 'array' | null {
  const n = normalizeDataType(dataType)
  if (typeof n === 'string') return n
  if (n && typeof n === 'object' && 'array' in n) return 'array'
  return null
}

/**
 * Normalize marketplace / import / API `data_type` into the UI ParameterDataType.
 * - TitleCase strings → lowercase scalars
 * - JSON Schema `"number"` → `"float"`
 * - `{ Enum: { options } }` / `{ enum: [...] }` → `{ enum: { options } }` or `{ enum: string[] }`
 * - `{ Array: { element_type } }` → `{ array: { element_type } }` (lowercase key)
 */
export function normalizeDataType(raw: unknown): ParameterDataType {
  if (raw == null) return 'string'

  if (typeof raw === 'string') {
    const s = raw.trim().toLowerCase()
    switch (s) {
      case 'integer':
      case 'int':
        return 'integer'
      case 'float':
      case 'number':
      case 'double':
        return 'float'
      case 'boolean':
      case 'bool':
        return 'boolean'
      case 'binary':
        return 'binary'
      case 'array':
        return 'array'
      case 'string':
      default:
        return 'string'
    }
  }

  if (typeof raw === 'object' && !Array.isArray(raw)) {
    const obj = raw as Record<string, unknown>
    const entries = Object.entries(obj)
    if (entries.length === 0) return 'string'

    const enumEntry = entries.find(([k]) => k.toLowerCase() === 'enum')
    if (enumEntry) {
      const e = enumEntry[1]
      if (Array.isArray(e) && e.every((x) => typeof x === 'string')) {
        return { enum: e as string[] }
      }
      if (e !== null && typeof e === 'object') {
        const opts = (e as { options?: unknown }).options
        if (Array.isArray(opts) && opts.every((x) => typeof x === 'string')) {
          return { enum: { options: opts as string[] } }
        }
      }
      return 'string'
    }

    const arrayEntry = entries.find(([k]) => k.toLowerCase() === 'array')
    if (arrayEntry) {
      const body = arrayEntry[1]
      if (body !== null && typeof body === 'object' && !Array.isArray(body)) {
        const et = (body as { element_type?: unknown }).element_type
        const element_type =
          typeof et === 'string' ? et.toLowerCase() : et == null ? null : String(et)
        return { array: { element_type } }
      }
      return 'array'
    }
  }

  return 'string'
}

/**
 * Map JSON Schema `type` (extension input_schema) onto NeoMind ParameterDataType.
 * `number` → `float`; otherwise same rules as normalizeDataType.
 */
export function mapJsonSchemaType(schemaType: unknown): ParameterDataType {
  if (typeof schemaType === 'string' && schemaType.toLowerCase() === 'number') {
    return 'float'
  }
  return normalizeDataType(schemaType)
}

/**
 * Seed initial values for a command's parameters.
 *
 * Consolidates the parameter-default initialization logic that previously
 * lived inline in both `CommandButton.tsx` and `DeviceDetail.tsx`. Beyond
 * deduplication, this module also recognizes a small set of "ID-shaped"
 * parameter names (`request_id`, `trace_id`, …) and pre-fills them with a
 * generated UUID so the user doesn't have to hand-craft a tracking ID
 * every time they invoke a command like NE301's `capture`.
 *
 * The detection is intentionally conservative — name-based only, opt-out
 * by renaming the parameter. Template authors who want explicit control
 * can still set `default_value` on the parameter, which always wins.
 */

import { generateId } from '@/lib/id'
import type { ParameterDefinition } from '@/types/device'
import {
  getAllowedValues,
  getEnumOptions,
  isArrayDataType,
  normalizeDataType,
  scalarDataType,
  unwrapMetricValue,
} from './parameterOptions'

/**
 * Parameter names that should be auto-populated with a generated id.
 * Matched case-insensitively against the bare parameter name, with or
 * without underscores. Covers the common conventions:
 *   request_id, requestId, trace_id, traceId, correlation_id, msg_id, message_id
 */
const AUTO_ID_PATTERNS = [
  /^request_?id$/i,
  /^trace_?id$/i,
  /^correlation_?id$/i,
  /^msg_?id$/i,
  /^message_?id$/i,
]

/** Returns true if this parameter should be auto-filled with a generated id. */
export function isAutoIdParam(param: ParameterDefinition): boolean {
  // Explicit defaults win — never override a template-declared default.
  if (param.default_value !== undefined) return false
  // Only string-typed parameters are candidates. Numbers/booleans named
  // "request_id" would be nonsensical and we shouldn't second-guess that.
  if (scalarDataType(normalizeDataType(param.data_type)) !== 'string') return false
  return AUTO_ID_PATTERNS.some((re) => re.test(param.name))
}

/**
 * Build the initial `values` map for a command's parameters.
 *
 * Resolution order for each parameter:
 *   1. `default_value` from schema (always wins)
 *   2. Auto-generated UUID for recognized ID parameters
 *   3. Type-appropriate empty value (0 / false / [] / first enum item / '')
 */
export function seedCommandDefaults(
  parameters: ParameterDefinition[] | undefined | null,
): Record<string, unknown> {
  if (!parameters || parameters.length === 0) return {}

  const values: Record<string, unknown> = {}

  for (const param of parameters) {
    // 1. Explicit default (unwrap tagged MetricValue from API)
    if (param.default_value !== undefined && param.default_value !== null) {
      values[param.name] = unwrapMetricValue(param.default_value)
      continue
    }

    // 2. Auto-id (only for string params matching known patterns)
    if (isAutoIdParam(param)) {
      values[param.name] = generateId()
      continue
    }

    // 3. Type-appropriate empty value. `undefined` means leave the key
    // absent so optional/required fields are not fake-filled with 0.
    const empty = emptyValueFor(param)
    if (empty !== undefined) {
      values[param.name] = empty
    }
  }

  return values
}

/**
 * Return a sensible initial value, or `undefined` to leave the key unset.
 * Numerics prefer `min` over `0` so NE301-style `min: 1` dialogs are valid
 * on first open.
 */
function emptyValueFor(param: ParameterDefinition): unknown {
  const normalized = normalizeDataType(param.data_type)
  const enumOpts = getEnumOptions(normalized)
  if (enumOpts && enumOpts.length > 0) {
    return enumOpts[0]
  }

  const allowed = getAllowedValues(param)
  if (allowed && allowed.length > 0) {
    return allowed[0]
  }

  if (isArrayDataType(normalized)) return []
  if (normalized && typeof normalized === 'object') {
    // Unrecognized object form (e.g. empty enum) — leave unset.
    return undefined
  }

  switch (scalarDataType(normalized)) {
    case 'integer':
    case 'float':
      if (typeof param.min === 'number') return param.min
      // Leave unset — required validation prompts the user; optional omits.
      return undefined
    case 'boolean':
      return false
    case 'array':
      return []
    case 'string':
    case 'binary':
    case null:
    default:
      return ''
  }
}

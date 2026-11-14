/**
 * Shared validate + prepare for device-command dynamic forms.
 *
 * UnifiedFormDialog submits via button onClick (not <form>), so HTML5
 * attributes never run. Callers must validate here before sendCommand.
 */

import type { ParameterDefinition, ValidationRule } from '@/types/device'
import { evalVisibleWhen } from '@/lib/parameterExpr'
import {
  getAllowedValues,
  getEnumOptions,
  isArrayDataType,
  normalizeDataType,
  scalarDataType,
  unicodeScalarLength,
} from './parameterOptions'

export interface CommandParamError {
  name: string
  message: string
}

export type ValidateCommandParamsResult =
  | { ok: true }
  | { ok: false; errors: CommandParamError[] }

function labelOf(param: ParameterDefinition): string {
  return param.display_name || param.name
}

function isEmptyValue(value: unknown): boolean {
  if (value === undefined || value === null) return true
  if (typeof value === 'string' && value.trim() === '') return true
  return false
}

function findRule(
  validations: ValidationRule[] | undefined,
  type: ValidationRule['type'],
): ValidationRule | undefined {
  return validations?.find((v) => v.type === type)
}

/**
 * Validate visible parameters before send.
 * Hidden (`visible_when`) fields are not required-checked but still type-checked
 * when a value is present (they are still submitted).
 */
export function validateCommandParams(
  parameters: ParameterDefinition[] | undefined | null,
  values: Record<string, unknown>,
): ValidateCommandParamsResult {
  if (!parameters || parameters.length === 0) return { ok: true }

  const errors: CommandParamError[] = []

  for (const param of parameters) {
    const visible = evalVisibleWhen(param.visible_when, values)
    const raw = values[param.name]
    const empty = isEmptyValue(raw)

    if (!visible) {
      // Hidden: skip required; still reject obviously corrupt typed values.
      if (!empty && isArrayDataType(param.data_type) && typeof raw === 'string') {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: invalid JSON array`,
        })
      }
      continue
    }

    if (param.required && empty) {
      errors.push({
        name: param.name,
        message: `${labelOf(param)} is required`,
      })
      continue
    }

    if (empty) continue

    // Array must be parsed JSON, not leftover textarea string.
    const kind = scalarDataType(normalizeDataType(param.data_type))

    if (kind === 'array' || isArrayDataType(param.data_type)) {
      if (!Array.isArray(raw)) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: expected a JSON array`,
        })
        continue
      }
    }

    if (kind === 'integer' || kind === 'float') {
      if (typeof raw !== 'number' || Number.isNaN(raw)) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: expected a number`,
        })
        continue
      }
      if (param.min !== undefined && param.min !== null && raw < param.min) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: min ${param.min}`,
        })
      }
      if (param.max !== undefined && param.max !== null && raw > param.max) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: max ${param.max}`,
        })
      }
      const range = findRule(param.validation, 'range')
      if (range && range.type === 'range') {
        if (raw < range.min || raw > range.max) {
          errors.push({
            name: param.name,
            message: range.error_message || `${labelOf(param)}: out of range`,
          })
        }
      }
    }

    if (typeof raw === 'string') {
      const lengthRule = findRule(param.validation, 'length')
      if (lengthRule && lengthRule.type === 'length') {
        // Unicode scalars — same contract as Rust ValidationRule::Length.
        const len = unicodeScalarLength(raw)
        if (len < lengthRule.min || len > lengthRule.max) {
          errors.push({
            name: param.name,
            message: lengthRule.error_message || `${labelOf(param)}: invalid length`,
          })
        }
      }
      const pattern = findRule(param.validation, 'pattern')
      if (pattern && pattern.type === 'pattern') {
        try {
          const re = new RegExp(pattern.regex)
          if (!re.test(raw)) {
            errors.push({
              name: param.name,
              message: pattern.error_message || `${labelOf(param)}: invalid format`,
            })
          }
        } catch {
          // Bad regex in schema — failure-open (same spirit as visible_when).
        }
      }
    }

    const enumOpts = getEnumOptions(normalizeDataType(param.data_type))
    const allowed = getAllowedValues(param)
    if (allowed && allowed.length > 0) {
      if (!allowed.some((v) => v === raw || String(v) === String(raw))) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: value not in allowed list`,
        })
      }
    } else if (enumOpts && enumOpts.length > 0) {
      if (!enumOpts.includes(String(raw))) {
        errors.push({
          name: param.name,
          message: `${labelOf(param)}: value not in enum options`,
        })
      }
    }
  }

  return errors.length > 0 ? { ok: false, errors } : { ok: true }
}

/**
 * Build the outbound params map:
 * - drop undefined / null
 * - drop empty strings for optional params (omit vs send "")
 * - keep 0 / false / non-empty arrays
 * - keep empty string for required (validation should have blocked)
 */
export function prepareCommandParams(
  parameters: ParameterDefinition[] | undefined | null,
  values: Record<string, unknown>,
): Record<string, unknown> {
  const defs = parameters ?? []
  const byName = new Map(defs.map((p) => [p.name, p]))
  const out: Record<string, unknown> = {}

  for (const [key, value] of Object.entries(values)) {
    if (value === undefined || value === null) continue
    const param = byName.get(key)
    if (
      param &&
      !param.required &&
      typeof value === 'string' &&
      value.trim() === ''
    ) {
      continue
    }
    out[key] = value
  }

  return out
}

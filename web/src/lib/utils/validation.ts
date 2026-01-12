/**
 * Validation utilities for form inputs
 */

export const validators = {
  /**
   * Required field validator
   */
  required: (value: any, message?: string) => {
    if (value === null || value === undefined) {
      return message || '此字段为必填项'
    }
    if (typeof value === 'string' && !value.trim()) {
      return message || '此字段为必填项'
    }
    if (Array.isArray(value) && value.length === 0) {
      return message || '此字段为必填项'
    }
    return null
  },

  /**
   * Device ID validator
   */
  deviceId: (value: string) => {
    if (!value) return null
    return /^[a-zA-Z0-9_-]+$/.test(value)
      ? null
      : '设备ID只能包含字母、数字、下划线和连字符'
  },

  /**
   * Port number validator
   */
  port: (value: number) => {
    if (!value) return null
    if (value < 1 || value > 65535) {
      return '端口号必须在 1-65535 范围内'
    }
    return null
  },

  /**
   * Email validator
   */
  email: (value: string) => {
    if (!value) return null
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
    return emailRegex.test(value) ? null : '请输入有效的邮箱地址'
  },

  /**
   * URL validator
   */
  url: (value: string) => {
    if (!value) return null
    try {
      new URL(value)
      return null
    } catch {
      return '请输入有效的 URL'
    }
  },

  /**
   * Minimum length validator
   */
  minLength: (value: string, min: number, name?: string) => {
    if (!value) return null
    return value.length >= min
      ? null
      : `${name || '此字段'}至少需要 ${min} 个字符`
  },

  /**
   * Maximum length validator
   */
  maxLength: (value: string, max: number, name?: string) => {
    if (!value) return null
    return value.length <= max
      ? null
      : `${name || '此字段'}不能超过 ${max} 个字符`
  },

  /**
   * Pattern validator
   */
  pattern: (value: string, regex: RegExp, message: string) => {
    if (!value) return null
    return regex.test(value) ? null : message
  },
}

/**
 * Validate multiple fields
 *
 * @example
 * const result = validate(
 *   { email: 'test@test.com', deviceId: 'sensor-1' },
 *   {
 *     email: (v) => validators.email(v),
 *     deviceId: (v) => validators.deviceId(v),
 *   }
 * )
 */
export function validate(
  values: Record<string, any>,
  rules: Record<string, (value: any) => string | null>
) {
  const errors: Record<string, string> = {}

  for (const [field, validator] of Object.entries(rules)) {
    const error = validator(values[field])
    if (error) {
      errors[field] = error
    }
  }

  return {
    isValid: Object.keys(errors).length === 0,
    errors,
  }
}

/**
 * Async validation
 */
export async function validateAsync(
  values: Record<string, any>,
  rules: Record<string, (value: any) => Promise<string | null>>
) {
  const errors: Record<string, string> = {}

  for (const [field, validator] of Object.entries(rules)) {
    const error = await validator(values[field])
    if (error) {
      errors[field] = error
    }
  }

  return {
    isValid: Object.keys(errors).length === 0,
    errors,
  }
}

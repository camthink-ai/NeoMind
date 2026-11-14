import { describe, expect, it } from 'vitest'
import type { ParameterDefinition } from '@/types/device'
import {
  prepareCommandParams,
  validateCommandParams,
} from '../commandParams'
import { seedCommandDefaults } from '../seedCommandDefaults'
import {
  getEnumOptions,
  isArrayDataType,
  mapJsonSchemaType,
  normalizeDataType,
  truncateToUnicodeScalars,
  unicodeScalarLength,
  unwrapMetricValue,
} from '../parameterOptions'

describe('parameterOptions', () => {
  it('unwraps tagged MetricValue objects', () => {
    expect(unwrapMetricValue({ String: 'white' })).toBe('white')
    expect(unwrapMetricValue({ string: 'white' })).toBe('white')
    expect(unwrapMetricValue(42)).toBe(42)
    expect(
      unwrapMetricValue({ Array: [{ Integer: 1 }, { Integer: 2 }] }),
    ).toEqual([1, 2])
    expect(unwrapMetricValue({ Binary: 'YQ==' })).toBe('YQ==')
  })

  it('counts Unicode scalars for length alignment', () => {
    expect(unicodeScalarLength('你好世界啊')).toBe(5)
    expect(unicodeScalarLength('😀😀😀😀😀')).toBe(5)
    // Five emoji = 5 scalars / 10 UTF-16 units — truncate by scalars.
    expect(truncateToUnicodeScalars('😀😀😀😀😀😀', 5)).toBe('😀😀😀😀😀')
    expect(truncateToUnicodeScalars('😀😀😀😀😀', 5)).toBe('😀😀😀😀😀')
  })

  it('reads both enum wire shapes', () => {
    expect(getEnumOptions({ enum: ['a', 'b'] })).toEqual(['a', 'b'])
    expect(getEnumOptions({ enum: { options: ['a', 'b'] } })).toEqual(['a', 'b'])
  })

  it('detects array object form', () => {
    expect(isArrayDataType('array')).toBe(true)
    expect(isArrayDataType({ array: { element_type: 'string' } } as never)).toBe(
      true,
    )
    expect(isArrayDataType('string')).toBe(false)
  })

  it('normalizes TitleCase and JSON Schema number', () => {
    expect(normalizeDataType('Integer')).toBe('integer')
    expect(normalizeDataType('Float')).toBe('float')
    expect(mapJsonSchemaType('number')).toBe('float')
    expect(normalizeDataType({ Enum: { options: ['a', 'b'] } })).toEqual({
      enum: { options: ['a', 'b'] },
    })
    expect(normalizeDataType({ Array: { element_type: 'String' } })).toEqual({
      array: { element_type: 'string' },
    })
  })
})

describe('seedCommandDefaults', () => {
  it('seeds numeric min instead of 0', () => {
    const params: ParameterDefinition[] = [
      {
        name: 'duration_sec',
        data_type: 'integer',
        min: 1,
        max: 3600,
        required: true,
      },
    ]
    expect(seedCommandDefaults(params)).toEqual({ duration_sec: 1 })
  })

  it('unwraps tagged defaults and leaves unset numerics without min', () => {
    const params: ParameterDefinition[] = [
      {
        name: 'color',
        data_type: { enum: { options: ['white', 'black'] } },
        default_value: { String: 'white' } as never,
      },
      { name: 'count', data_type: 'integer', required: true },
    ]
    expect(seedCommandDefaults(params)).toEqual({ color: 'white' })
  })

  it('unwraps tagged Array defaults into plain arrays', () => {
    const params: ParameterDefinition[] = [
      {
        name: 'ids',
        data_type: 'array',
        default_value: { Array: [{ Integer: 1 }, { Integer: 2 }] } as never,
      },
    ]
    expect(seedCommandDefaults(params)).toEqual({ ids: [1, 2] })
    expect(
      validateCommandParams(params, seedCommandDefaults(params)).ok,
    ).toBe(true)
  })
})

describe('validateCommandParams / prepareCommandParams', () => {
  const sleepParams: ParameterDefinition[] = [
    {
      name: 'duration_sec',
      display_name: 'Sleep Duration',
      data_type: 'integer',
      min: 1,
      max: 3600,
      required: true,
    },
  ]

  it('rejects missing required', () => {
    const r = validateCommandParams(sleepParams, {})
    expect(r.ok).toBe(false)
  })

  it('rejects below min', () => {
    const r = validateCommandParams(sleepParams, { duration_sec: 0 })
    expect(r.ok).toBe(false)
  })

  it('accepts valid number', () => {
    expect(validateCommandParams(sleepParams, { duration_sec: 60 }).ok).toBe(
      true,
    )
  })

  it('omits empty optional strings on prepare', () => {
    const params: ParameterDefinition[] = [
      { name: 'note', data_type: 'string', required: false },
      { name: 'mode', data_type: 'string', required: true },
    ]
    expect(
      prepareCommandParams(params, { note: '', mode: 'always', extra: null }),
    ).toEqual({ mode: 'always' })
  })

  it('rejects half-parsed array strings', () => {
    const params: ParameterDefinition[] = [
      { name: 'items', data_type: 'array', required: true },
    ]
    const r = validateCommandParams(params, { items: '[1, 2' })
    expect(r.ok).toBe(false)
  })

  it('accepts CJK within Length max by Unicode scalars', () => {
    const params: ParameterDefinition[] = [
      {
        name: 'text',
        data_type: 'string',
        required: true,
        validation: [
          { type: 'length', min: 1, max: 5, error_message: 'max 5' },
        ],
      },
    ]
    expect(validateCommandParams(params, { text: '你好世界啊' }).ok).toBe(true)
    expect(validateCommandParams(params, { text: '你好世界啊六' }).ok).toBe(
      false,
    )
  })
})

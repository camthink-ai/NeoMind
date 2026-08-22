import { describe, it, expect } from 'vitest'
import { parseExprRefs, evalExpression, evalExpressionSeries } from './expressionSource'

describe('parseExprRefs', () => {
  it('extracts canonical refs', () => {
    expect(parseExprRefs('avg(device:s1:values.battery, device:s2:values.battery)')).toEqual([
      'device:s1:values.battery',
      'device:s2:values.battery',
    ])
  })
  it('extracts sugar refs and dedupes against canonical', () => {
    expect(parseExprRefs('s1.temperature - s2.temperature')).toEqual([
      'device:s1:temperature',
      'device:s2:temperature',
    ])
    expect(parseExprRefs('device:s1:temperature + s1.temperature')).toEqual([
      'device:s1:temperature',
    ])
  })
})

describe('evalExpression', () => {
  const v = {
    'device:s1:temperature': 20,
    'device:s2:temperature': 30,
  }
  it('arithmetic with sugar refs', () => {
    expect(evalExpression('s2.temperature - s1.temperature', v).value).toBe(10)
    expect(evalExpression('(s1.temperature + s2.temperature) / 2', v).value).toBe(25)
  })
  it('functions with canonical refs', () => {
    expect(evalExpression('avg(device:s1:temperature, device:s2:temperature)', v).value).toBe(25)
    expect(evalExpression('max(s1.temperature, s2.temperature)', v).value).toBe(30)
    expect(evalExpression('round(sqrt(pow(s1.temperature, 2)))', v).value).toBe(20)
  })
  it('mixed spellings of the same ref', () => {
    expect(evalExpression('device:s1:temperature + s1.temperature', v).value).toBe(40)
  })
  it('missing refs → error', () => {
    const r = evalExpression('s1.temperature + s9.temperature', v)
    expect(r.value).toBeNull()
    expect(r.error).toContain('device:s9:temperature')
  })
  it('rejects identifier/JS injection after substitution', () => {
    // `alert(1)` — alert is not a ref and not a whitelisted fn → rejected
    const r = evalExpression('alert(1)', v)
    expect(r.value).toBeNull()
    expect(r.error).toContain('disallowed')
    // Member access via a ref-looking sugar on window
    const r2 = evalExpression('s1.constructor', v)
    expect(r2.value).toBeNull()
  })
  it('div-by-zero → not finite → error', () => {
    expect(evalExpression('s1.temperature / 0', v).value).toBeNull()
  })
})

describe('evalExpressionSeries', () => {
  const series = {
    'device:s1:t': [
      { t: 0, v: 1 },
      { t: 10, v: 2 },
      { t: 20, v: 3 },
    ],
    'device:s2:t': [
      { t: 5, v: 10 },
      { t: 15, v: 20 },
    ],
  }
  it('forward-fill aligns and evaluates per skeleton timestamp', () => {
    const { series: out } = evalExpressionSeries('s1.t + s2.t', series)
    // t=0: s2 not started → skipped. t=10: s2=10 → 12. t=20: s2=20 → 23.
    expect(out).toEqual([
      { t: 10, v: 12 },
      { t: 20, v: 23 },
    ])
  })
  it('no history at all → empty + error', () => {
    const r = evalExpressionSeries('s1.t + s2.t', {})
    expect(r.series).toEqual([])
    expect(r.error).toBeTruthy()
  })
})

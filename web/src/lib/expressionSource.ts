/**
 * Expression data source — inline computed values WITHOUT pre-creating a
 * transform. A component's data_source can be:
 *
 *   {"type":"telemetry", "source":"expression", "mode":"latest",
 *    "expr":"avg(device:sensor-001:values.battery, device:sensor-002:values.battery)"}
 *
 * The expression references device metrics (`device:<id>:<field>` canonical,
 * or bare `<id>.<field>` sugar) and combines them with arithmetic and a
 * small function set (avg/sum/min/max/abs/round/floor/ceil/sqrt/pow).
 * Evaluation happens client-side: refs resolve to live store telemetry
 * (latest) or fetched history aligned forward-fill (timeseries).
 *
 * Security: refs are substituted with numeric literals first; the remainder
 * must then consist ONLY of numbers, operators, and whitelisted function
 * names — anything else (identifiers, strings, member access) is rejected
 * before `new Function` sees it.
 */

export interface ExprPoint {
  t: number
  v: number
}

export interface ExprResult {
  value: number | null
  error?: string
}

const REF_CANONICAL = /device:([A-Za-z0-9_-]+):([A-Za-z0-9_.-]+)/g
// Sugar `<id>.<field>` — must NOT match the tail of a canonical
// `device:<id>:<field>` (its `field` contains dots after a colon).
const REF_SUGAR = /(^|[^:A-Za-z0-9_])([A-Za-z][A-Za-z0-9_-]*)\.([A-Za-z][A-Za-z0-9_.-]*)/g
const FN_NAMES = ['avg', 'sum', 'min', 'max', 'abs', 'round', 'floor', 'ceil', 'sqrt', 'pow']

/** All refs in canonical form `device:<id>:<field>` (deduped, order of first use). */
export function parseExprRefs(expr: string): string[] {
  const seen: string[] = []
  const push = (id: string, field: string) => {
    const ref = `device:${id}:${field}`
    if (!seen.includes(ref)) seen.push(ref)
  }
  for (const m of expr.matchAll(REF_CANONICAL)) push(m[1], m[2])
  for (const m of expr.matchAll(REF_SUGAR)) push(m[2], m[3])
  return seen
}

/** id/field pair for a canonical ref. */
export function refParts(ref: string): { id: string; field: string } | null {
  const m = ref.match(/^device:([A-Za-z0-9_-]+):([A-Za-z0-9_.-]+)$/)
  return m ? { id: m[1], field: m[2] } : null
}

/**
 * Evaluate with resolved numeric ref values. Substitution is longest-ref-first
 * so `sensor-001.temperature` can't be clobbered by a `sensor-001`-prefixed
 * shorter ref.
 */
export function evalExpression(expr: string, values: Record<string, number>): ExprResult {
  const refs = parseExprRefs(expr)
  let js = expr
  const sorted = [...refs].sort((a, b) => b.length - a.length)
  const missing: string[] = []
  for (const ref of sorted) {
    const sugar = ref.replace(/^device:([A-Za-z0-9_-]+):([A-Za-z0-9_.-]+)$/, '$1.$2')
    const v = values[ref]
    if (v === undefined || v === null || Number.isNaN(v)) {
      missing.push(ref)
      continue
    }
    // Replace BOTH spellings of this ref with the numeric literal.
    js = js.split(ref).join(`(${v})`).split(sugar).join(`(${v})`)
  }
  if (missing.length > 0) {
    return { value: null, error: `unresolved refs: ${missing.join(', ')}` }
  }

  // Safety gate: strip whitelisted fn names + digits, then only operators may
  // remain. Identifiers (incl. member access like `window.x`), strings,
  // backticks, `=>`, `[` — all rejected here.
  let residue = js
  for (const fn of FN_NAMES) residue = residue.split(fn).join('')
  residue = residue.replace(/[0-9.]/g, '')
  if (!/^[+\-*/(),%\s]*$/.test(residue)) {
    return { value: null, error: `expression contains disallowed tokens: "${residue.trim().slice(0, 40)}"` }
  }

  try {
    const fns: Record<string, (...args: number[]) => number> = {
      avg: (...a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : 0),
      sum: (...a) => a.reduce((x, y) => x + y, 0),
      min: (...a) => Math.min(...a),
      max: (...a) => Math.max(...a),
      abs: Math.abs,
      round: Math.round,
      floor: Math.floor,
      ceil: Math.ceil,
      sqrt: Math.sqrt,
      pow: Math.pow,
    }
     
    const fn = new Function(...FN_NAMES, `"use strict"; return (${js});`)
    const out = fn(...FN_NAMES.map((n) => fns[n]))
    if (typeof out !== 'number' || !Number.isFinite(out)) {
      return { value: null, error: 'expression did not evaluate to a finite number' }
    }
    return { value: out }
  } catch (e) {
    return { value: null, error: e instanceof Error ? e.message : String(e) }
  }
}

/**
 * Evaluate per-timestamp over aligned history. The first ref's timestamps
 * form the skeleton; every other ref forward-fills its latest value at or
 * before each skeleton timestamp. Series input: ascending by t.
 */
export function evalExpressionSeries(
  expr: string,
  seriesByRef: Record<string, ExprPoint[]>
): { series: ExprPoint[]; error?: string } {
  const refs = parseExprRefs(expr)
  const skeletonRef = refs.find((r) => (seriesByRef[r] ?? []).length > 0)
  if (!skeletonRef) return { series: [], error: 'no history for any referenced metric' }
  const skeleton = seriesByRef[skeletonRef]

  // Pointers for forward-fill per other ref.
  const idx: Record<string, number> = {}
  const out: ExprPoint[] = []
  for (const p of skeleton) {
    const values: Record<string, number> = {}
    for (const ref of refs) {
      const s = seriesByRef[ref] ?? []
      while (idx[ref] !== undefined && idx[ref] + 1 < s.length && s[idx[ref] + 1].t <= p.t) {
        idx[ref]++
      }
      if (s.length === 0) continue
      if (idx[ref] === undefined) {
        // Before the other series starts: no value yet — skip the point.
        if (s[0].t > p.t) continue
        idx[ref] = 0
      }
      values[ref] = s[idx[ref]].v
    }
    if (Object.keys(values).length !== refs.length) continue
    const r = evalExpression(expr, values)
    if (r.value !== null) out.push({ t: p.t, v: r.value })
  }
  return { series: out }
}

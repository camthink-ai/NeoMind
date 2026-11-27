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
 * Security: refs are substituted with numeric literals first, and the
 * resulting expression is parsed and evaluated by a self-contained
 * recursive-descent parser (no `eval` / `new Function`). Only numbers,
 * arithmetic/comparison/boolean operators, and whitelisted function names
 * are accepted — anything else (arbitrary identifiers, strings, member
 * access) is rejected.
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

const WHITELISTED_FNS: Record<string, (...args: number[]) => number> = {
  avg: (...a) => (a.length ? a.reduce((x, y) => x + y, 0) / a.length : 0),
  sum: (...a) => a.reduce((x, y) => x + y, 0),
  min: (...a) => (a.length ? Math.min(...a) : NaN),
  max: (...a) => (a.length ? Math.max(...a) : NaN),
  abs: Math.abs,
  round: Math.round,
  floor: Math.floor,
  ceil: Math.ceil,
  sqrt: Math.sqrt,
  pow: Math.pow,
}

type Token =
  | { type: 'number'; value: number }
  | { type: 'ident'; value: string }
  | { type: 'op'; value: string }
  | { type: 'lparen' }
  | { type: 'rparen' }
  | { type: 'comma' }
  | { type: 'question' }
  | { type: 'colon' }
  | { type: 'eof' }

const TWO_CHAR_OPS = new Set(['<=', '>=', '==', '!=', '&&', '||'])
const SINGLE_CHAR_OPS = new Set(['+', '-', '*', '/', '%', '<', '>', '!'])
const MAX_PARSE_DEPTH = 64

function tokenize(input: string): Token[] {
  const tokens: Token[] = []
  let i = 0
  const n = input.length

  while (i < n) {
    const ch = input[i]
    if (/\s/.test(ch)) {
      i++
      continue
    }

    // Number literal (integer, decimal, optional scientific notation)
    if (/[0-9]/.test(ch) || (ch === '.' && i + 1 < n && /[0-9]/.test(input[i + 1]))) {
      const start = i
      while (i < n && /[0-9]/.test(input[i])) i++
      if (i < n && input[i] === '.') {
        i++
        while (i < n && /[0-9]/.test(input[i])) i++
      }
      if (i < n && (input[i] === 'e' || input[i] === 'E')) {
        const expStart = i
        i++
        if (i < n && (input[i] === '+' || input[i] === '-')) i++
        const digitStart = i
        while (i < n && /[0-9]/.test(input[i])) i++
        if (i === digitStart) {
          throw new Error(`expression contains disallowed token: "${input.slice(start, expStart + 1)}"`)
        }
      }
      const raw = input.slice(start, i)
      const num = Number(raw)
      if (!Number.isFinite(num)) {
        throw new Error(`invalid number literal: "${raw}"`)
      }
      tokens.push({ type: 'number', value: num })
      continue
    }

    // Identifier (only whitelisted function names or boolean literals)
    if (/[A-Za-z_]/.test(ch)) {
      const start = i
      while (i < n && /[A-Za-z0-9_]/.test(input[i])) i++
      const ident = input.slice(start, i)
      if (
        ident !== 'true' &&
        ident !== 'false' &&
        !Object.prototype.hasOwnProperty.call(WHITELISTED_FNS, ident)
      ) {
        throw new Error(`expression contains disallowed identifier: "${ident.slice(0, 40)}"`)
      }
      tokens.push({ type: 'ident', value: ident })
      continue
    }

    // Two-character operators
    if (i + 1 < n) {
      const pair = input.slice(i, i + 2)
      if (TWO_CHAR_OPS.has(pair)) {
        tokens.push({ type: 'op', value: pair })
        i += 2
        continue
      }
    }

    // Single-character operators and punctuation
    if (SINGLE_CHAR_OPS.has(ch)) {
      tokens.push({ type: 'op', value: ch })
      i++
      continue
    }
    if (ch === '(') {
      tokens.push({ type: 'lparen' })
      i++
      continue
    }
    if (ch === ')') {
      tokens.push({ type: 'rparen' })
      i++
      continue
    }
    if (ch === ',') {
      tokens.push({ type: 'comma' })
      i++
      continue
    }
    if (ch === '?') {
      tokens.push({ type: 'question' })
      i++
      continue
    }
    if (ch === ':') {
      tokens.push({ type: 'colon' })
      i++
      continue
    }

    throw new Error(`expression contains disallowed tokens: "${input.slice(i).trim().slice(0, 40)}"`)
  }

  tokens.push({ type: 'eof' })
  return tokens
}

class ExprParser {
  private pos = 0
  private depth = 0

  constructor(private readonly tokens: Token[]) {}

  private peek(): Token {
    return this.tokens[this.pos] ?? { type: 'eof' }
  }

  private consume(): Token {
    const tok = this.peek()
    if (tok.type !== 'eof') this.pos++
    return tok
  }

  private matchOp(...ops: string[]): string | null {
    const tok = this.peek()
    if (tok.type === 'op' && ops.includes(tok.value)) {
      this.pos++
      return tok.value
    }
    return null
  }

  private enter(): void {
    this.depth++
    if (this.depth > MAX_PARSE_DEPTH) {
      throw new Error('expression exceeds maximum nesting depth')
    }
  }

  private leave(): void {
    this.depth--
  }

  parse(): number {
    const result = this.parseTernary()
    if (this.peek().type !== 'eof') {
      throw new Error('unexpected trailing tokens in expression')
    }
    return result
  }

  private parseTernary(): number {
    this.enter()
    try {
      const cond = this.parseLogicalOr()
      if (this.peek().type === 'question') {
        this.consume()
        const whenTrue = this.parseTernary()
        if (this.peek().type !== 'colon') {
          throw new Error('expected ":" in conditional expression')
        }
        this.consume()
        const whenFalse = this.parseTernary()
        return cond !== 0 ? whenTrue : whenFalse
      }
      return cond
    } finally {
      this.leave()
    }
  }

  private parseLogicalOr(): number {
    let left = this.parseLogicalAnd()
    while (this.matchOp('||')) {
      const right = this.parseLogicalAnd()
      left = left !== 0 || right !== 0 ? 1 : 0
    }
    return left
  }

  private parseLogicalAnd(): number {
    let left = this.parseEquality()
    while (this.matchOp('&&')) {
      const right = this.parseEquality()
      left = left !== 0 && right !== 0 ? 1 : 0
    }
    return left
  }

  private parseEquality(): number {
    let left = this.parseComparison()
    let op: string | null
    while ((op = this.matchOp('==', '!='))) {
      const right = this.parseComparison()
      left = op === '==' ? (left === right ? 1 : 0) : (left !== right ? 1 : 0)
    }
    return left
  }

  private parseComparison(): number {
    let left = this.parseAdditive()
    let op: string | null
    while ((op = this.matchOp('<', '<=', '>', '>='))) {
      const right = this.parseAdditive()
      switch (op) {
        case '<':
          left = left < right ? 1 : 0
          break
        case '<=':
          left = left <= right ? 1 : 0
          break
        case '>':
          left = left > right ? 1 : 0
          break
        case '>=':
          left = left >= right ? 1 : 0
          break
      }
    }
    return left
  }

  private parseAdditive(): number {
    let left = this.parseMultiplicative()
    let op: string | null
    while ((op = this.matchOp('+', '-'))) {
      const right = this.parseMultiplicative()
      left = op === '+' ? left + right : left - right
    }
    return left
  }

  private parseMultiplicative(): number {
    let left = this.parseUnary()
    let op: string | null
    while ((op = this.matchOp('*', '/', '%'))) {
      const right = this.parseUnary()
      if (op === '*') left = left * right
      else if (op === '/') left = left / right
      else left = left % right
    }
    return left
  }

  private parseUnary(): number {
    const op = this.matchOp('+', '-', '!')
    if (op) {
      this.enter()
      try {
        const operand = this.parseUnary()
        if (op === '+') return +operand
        if (op === '-') return -operand
        return operand === 0 ? 1 : 0
      } finally {
        this.leave()
      }
    }
    return this.parsePrimary()
  }

  private parsePrimary(): number {
    const tok = this.peek()
    if (tok.type === 'number') {
      this.consume()
      return tok.value
    }

    if (tok.type === 'lparen') {
      this.consume()
      const val = this.parseTernary()
      if (this.peek().type !== 'rparen') {
        throw new Error('expected closing ")"')
      }
      this.consume()
      return val
    }

    if (tok.type === 'ident') {
      this.consume()
      if (tok.value === 'true') return 1
      if (tok.value === 'false') return 0
      const fn = WHITELISTED_FNS[tok.value]
      if (!fn) {
        throw new Error(`expression contains disallowed identifier: "${tok.value}"`)
      }
      if (this.peek().type !== 'lparen') {
        throw new Error(`expected "(" after function "${tok.value}"`)
      }
      this.consume()
      const args: number[] = []
      if (this.peek().type !== 'rparen') {
        args.push(this.parseTernary())
        while (this.peek().type === 'comma') {
          this.consume()
          args.push(this.parseTernary())
        }
      }
      if (this.peek().type !== 'rparen') {
        throw new Error(`expected closing ")" for function "${tok.value}"`)
      }
      this.consume()
      return fn(...args)
    }

    throw new Error('unexpected token in expression')
  }
}

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
  let substituted = expr
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
    substituted = substituted.split(ref).join(`(${v})`).split(sugar).join(`(${v})`)
  }
  if (missing.length > 0) {
    return { value: null, error: `unresolved refs: ${missing.join(', ')}` }
  }

  try {
    const tokens = tokenize(substituted)
    const out = new ExprParser(tokens).parse()
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

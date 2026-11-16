/**
 * JSON envelope layer (§6): request / response / event encode+decode.
 * Rejects UTF-8 BOM, non-object tops, float integers, trailing garbage.
 */

import { provErrorCodeFromWire, provOpFromWire, type ProvErrorCode, type ProvOp } from './ops'

export type JsonObject = Record<string, unknown>

export interface ProvError {
  codeName: string
  message: string
  code: ProvErrorCode | null
}

export interface ProvRequest {
  kind: 'request'
  id: number
  opName: string
  data: JsonObject
  op: ProvOp | null
}

export interface ProvResponse {
  kind: 'response'
  id: number
  opName: string
  ok: boolean
  data?: JsonObject
  error?: ProvError
}

export interface ProvEvent {
  kind: 'event'
  seq: number
  event: string
  data: JsonObject
}

export type ProvEnvelope = ProvRequest | ProvResponse | ProvEvent

export class ProvEnvelopeError extends Error {
  readonly reason: string

  constructor(reason: string) {
    super(`ProvEnvelopeError: ${reason}`)
    this.name = 'ProvEnvelopeError'
    this.reason = reason
  }
}

const utf8Decoder = new TextDecoder('utf-8', { fatal: true })
const utf8Encoder = new TextEncoder()

function isPlainObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function firstNonWhitespace(s: string): number {
  for (let i = 0; i < s.length; i++) {
    if (s[i]!.trim() !== '') return i
  }
  return -1
}

/** Scan object end index (exclusive); -1 if mismatched / unclosed. */
function scanObjectEnd(s: string, start: number): number {
  let depth = 0
  let inString = false
  let escaped = false
  for (let i = start; i < s.length; i++) {
    const c = s[i]!
    if (inString) {
      if (escaped) {
        escaped = false
      } else if (c === '\\') {
        escaped = true
      } else if (c === '"') {
        inString = false
      }
      continue
    }
    if (c === '"') {
      inString = true
    } else if (c === '{' || c === '[') {
      depth++
    } else if (c === '}' || c === ']') {
      depth--
      if (depth === 0) return i + 1
      if (depth < 0) return -1
    }
  }
  return -1
}

function decodeMap(map: JsonObject): ProvEnvelope {
  // Integers must not be floats (§6) — check before shape.
  for (const key of ['id', 'seq'] as const) {
    const v = map[key]
    if (typeof v === 'number' && !Number.isInteger(v)) {
      throw new ProvEnvelopeError('float_int')
    }
  }

  if ('event' in map) return decodeEvent(map)
  return decodeRequestOrResponse(map)
}

function decodeEvent(map: JsonObject): ProvEvent {
  const seq = map.seq
  const event = map.event
  const data = map.data
  if (
    typeof seq !== 'number' ||
    !Number.isInteger(seq) ||
    seq < 0 ||
    typeof event !== 'string' ||
    !isPlainObject(data)
  ) {
    throw new ProvEnvelopeError('invalid_shape')
  }
  return { kind: 'event', seq, event, data }
}

function decodeRequestOrResponse(map: JsonObject): ProvRequest | ProvResponse {
  const id = map.id
  const op = map.op
  if (
    typeof id !== 'number' ||
    !Number.isInteger(id) ||
    id < 0 ||
    typeof op !== 'string' ||
    op.length === 0
  ) {
    throw new ProvEnvelopeError('invalid_shape')
  }
  if ('ok' in map) return decodeResponse(map, id, op)
  return decodeRequest(map, id, op)
}

function decodeRequest(map: JsonObject, id: number, op: string): ProvRequest {
  const data = map.data
  if (!isPlainObject(data)) throw new ProvEnvelopeError('invalid_shape')
  return {
    kind: 'request',
    id,
    opName: op,
    data,
    op: provOpFromWire(op),
  }
}

function decodeResponse(map: JsonObject, id: number, op: string): ProvResponse {
  const ok = map.ok
  if (typeof ok !== 'boolean' || !op.endsWith('.result')) {
    throw new ProvEnvelopeError('invalid_shape')
  }
  if (ok) {
    const data = map.data
    if (data !== undefined && data !== null && !isPlainObject(data)) {
      throw new ProvEnvelopeError('invalid_shape')
    }
    return {
      kind: 'response',
      id,
      opName: op,
      ok: true,
      data: data === undefined || data === null ? undefined : data,
    }
  }
  const error = map.error
  if (!isPlainObject(error)) throw new ProvEnvelopeError('invalid_shape')
  const code = error.code
  const message = error.message
  if (typeof code !== 'string' || typeof message !== 'string') {
    throw new ProvEnvelopeError('invalid_shape')
  }
  return {
    kind: 'response',
    id,
    opName: op,
    ok: false,
    error: {
      codeName: code,
      message,
      code: provErrorCodeFromWire(code),
    },
  }
}

/** Decode a complete logical-packet payload (UTF-8 JSON envelope). */
export function decodeEnvelope(wireBytes: Uint8Array | number[]): ProvEnvelope {
  const bytes = wireBytes instanceof Uint8Array ? wireBytes : new Uint8Array(wireBytes)

  // BOM must be rejected at the byte layer — TextDecoder may strip it.
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    throw new ProvEnvelopeError('bom')
  }

  let text: string
  try {
    text = utf8Decoder.decode(bytes)
  } catch {
    throw new ProvEnvelopeError('invalid_utf8')
  }

  const start = firstNonWhitespace(text)
  if (start < 0 || text[start] !== '{') {
    throw new ProvEnvelopeError('not_object')
  }
  const end = scanObjectEnd(text, start)
  if (end < 0) throw new ProvEnvelopeError('malformed')
  if (text.slice(end).trim() !== '') throw new ProvEnvelopeError('trailing_garbage')

  let value: unknown
  try {
    value = JSON.parse(text.slice(start, end))
  } catch {
    throw new ProvEnvelopeError('malformed')
  }
  if (!isPlainObject(value)) throw new ProvEnvelopeError('not_object')

  return decodeMap(value)
}

export function encodeRequest(opts: {
  id: number
  op: string
  data: JsonObject
}): string {
  return JSON.stringify({ id: opts.id, op: opts.op, data: opts.data })
}

export function encodeResponse(opts: {
  id: number
  op: string
  ok: boolean
  data?: JsonObject
  error?: { codeName: string; message: string }
}): string {
  if (opts.ok) {
    if (opts.error) throw new Error('error must be null when ok is true')
    return JSON.stringify({
      id: opts.id,
      op: opts.op,
      ok: true,
      data: opts.data ?? {},
    })
  }
  if (!opts.error) throw new Error('error is required when ok is false')
  return JSON.stringify({
    id: opts.id,
    op: opts.op,
    ok: false,
    error: { code: opts.error.codeName, message: opts.error.message },
  })
}

export function encodeEvent(opts: {
  seq: number
  event: string
  data: JsonObject
}): string {
  return JSON.stringify({ seq: opts.seq, event: opts.event, data: opts.data })
}

export function encodeEnvelopeBytes(jsonText: string): Uint8Array {
  return utf8Encoder.encode(jsonText)
}

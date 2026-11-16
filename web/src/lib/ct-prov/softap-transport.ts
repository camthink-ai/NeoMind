/**
 * SoftAP HTTP CT-Prov transport: whole envelope POST /api/v1/ctprov (no fragmentation).
 *
 * NeoMind never programmatically joins Wi‑Fi on macOS / Tauri / Web — the
 * operator must hand-join the device SoftAP in system settings first.
 */

import { DEFAULT_REQUEST_TIMEOUT_MS } from './constants'
import {
  decodeEnvelope,
  encodeRequest,
  type ProvRequest,
  type ProvResponse,
} from './envelope'
import { softapEndpointForHost, softapHostForModel } from './models'
import type { ProvTransport } from './transport'

export class SoftapTransportError extends Error {
  readonly code: string

  constructor(code: string, detail: string) {
    super(`SoftapTransportError(${code}): ${detail}`)
    this.name = 'SoftapTransportError'
    this.code = code
  }
}

export type SoftapProbeResult =
  | { ok: true; status: number }
  | { ok: false; reason: 'unreachable' | 'timeout'; detail: string }

/**
 * Lightweight reachability probe before opening a CT-Prov session.
 * Any HTTP response (including 404/405) means the SoftAP gateway is reachable;
 * AbortError / network failure → computer is almost certainly not on the SoftAP.
 */
export async function probeSoftapHost(
  host: string,
  opts?: { timeoutMs?: number; fetchImpl?: typeof fetch },
): Promise<SoftapProbeResult> {
  const timeoutMs = opts?.timeoutMs ?? 4_000
  const fetchImpl = opts?.fetchImpl ?? fetch.bind(globalThis)
  const endpoint = softapEndpointForHost(host.trim())
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), timeoutMs)
  try {
    // GET is enough: SoftAP may answer 404/405; that still proves L3/L4 reachability.
    const response = await fetchImpl(endpoint, {
      method: 'GET',
      signal: controller.signal,
    })
    return { ok: true, status: response.status }
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    const name = (err as { name?: string })?.name
    if (name === 'AbortError' || msg.toLowerCase().includes('abort')) {
      return { ok: false, reason: 'timeout', detail: `probe timed out after ${timeoutMs}ms` }
    }
    return { ok: false, reason: 'unreachable', detail: msg }
  } finally {
    clearTimeout(timer)
  }
}

/** Map SoftAP transport/probe failures to stable UI reason codes. */
export function softapFailureKind(
  err: unknown,
): 'not_on_softap' | 'http' | 'protocol' | 'other' {
  if (err instanceof SoftapTransportError) {
    if (err.code === 'ap_unreachable') return 'not_on_softap'
    if (err.code === 'http_status') return 'http'
    if (err.code === 'bad_envelope') return 'protocol'
    return 'other'
  }
  const msg = err instanceof Error ? err.message : String(err)
  if (msg.includes('ap_unreachable') || msg.toLowerCase().includes('timed out')) {
    return 'not_on_softap'
  }
  if (msg.includes('http_status') || /HTTP \d{3}/.test(msg)) return 'http'
  if (msg.includes('bad_envelope')) return 'protocol'
  return 'other'
}

export class SoftapHttpCtTransport implements ProvTransport {
  private endpoint: string
  private closed = false
  private fetchImpl: typeof fetch

  constructor(opts?: { host?: string; model?: string; endpoint?: string; fetchImpl?: typeof fetch }) {
    if (opts?.endpoint) {
      this.endpoint = opts.endpoint
    } else if (opts?.host) {
      this.endpoint = softapEndpointForHost(opts.host)
    } else if (opts?.model) {
      this.endpoint = softapEndpointForHost(softapHostForModel(opts.model))
    } else {
      this.endpoint = softapEndpointForHost(softapHostForModel('NE101'))
    }
    this.fetchImpl = opts?.fetchImpl ?? fetch.bind(globalThis)
  }

  get endpointUrl(): string {
    return this.endpoint
  }

  async sendRequest(
    request: ProvRequest,
    opts?: { timeoutMs?: number },
  ): Promise<ProvResponse> {
    if (this.closed) {
      throw new SoftapTransportError('transport_closed', 'transport closed before request sent')
    }
    const timeoutMs = opts?.timeoutMs ?? DEFAULT_REQUEST_TIMEOUT_MS
    const body = encodeRequest({
      id: request.id,
      op: request.opName,
      data: request.data,
    })

    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), timeoutMs)
    let response: Response
    try {
      response = await this.fetchImpl(this.endpoint, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json; charset=utf-8' },
        body,
        signal: controller.signal,
      })
    } catch (err) {
      clearTimeout(timer)
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('abort') || (err as { name?: string })?.name === 'AbortError') {
        throw new SoftapTransportError('ap_unreachable', `request timed out after ${timeoutMs}ms`)
      }
      throw new SoftapTransportError('ap_unreachable', msg)
    } finally {
      clearTimeout(timer)
    }

    if (!response.ok) {
      throw new SoftapTransportError(
        'http_status',
        `device returned HTTP ${response.status}`,
      )
    }

    const text = await response.text()
    let env
    try {
      env = decodeEnvelope(new TextEncoder().encode(text))
    } catch (err) {
      throw new SoftapTransportError(
        'bad_envelope',
        err instanceof Error ? err.message : String(err),
      )
    }
    if (env.kind !== 'response') {
      throw new SoftapTransportError('bad_envelope', `expected response, got ${env.kind}`)
    }
    if (env.id !== request.id) {
      throw new SoftapTransportError(
        'bad_envelope',
        `response id ${env.id} does not match request ${request.id}`,
      )
    }
    return env
  }

  async close(): Promise<void> {
    this.closed = true
  }
}

/**
 * SoftAP HTTP CT-Prov transport: whole envelope POST /api/v1/ctprov (no fragmentation).
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

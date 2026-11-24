/// fetchAPI retries gateway errors for reads only. A write that comes back
/// 502/504 is sent exactly once and reported as "outcome unknown": the server
/// may already have run it, and a second request would run it again —
/// a duplicate object, or a device command dispatched twice.
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest'
import i18n from '@/i18n/config'
import { fetchAPI } from '../client'

const { notifyFromError, notifySuccess } = vi.hoisted(() => ({ notifyFromError: vi.fn(), notifySuccess: vi.fn() }))
vi.mock('@/lib/notify', () => ({ notifyFromError, notifySuccess }))

const fetchMock = vi.fn<typeof fetch>()
const COMMAND_URL = '/devices/dev-1/command/setValue'

/** What a reverse proxy hands back on an upstream timeout: an HTML page, no JSON. */
function gatewayTimeout() {
  return new Response('<html><body><center>504 Gateway Time-out</center></body></html>', { status: 504 })
}

function json(status: number, body: unknown = { message: `gateway answered ${status}` }) {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

/**
 * Run `call` under fake timers. The rejection is asserted up front so it is
 * never unhandled while the retry backoff is being fast-forwarded.
 */
async function settle(call: Promise<unknown>) {
  const rejected = expect(call).rejects.toThrow()
  await vi.runAllTimersAsync()
  await rejected
}

describe('fetchAPI — write requests are never auto-retried', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    fetchMock.mockReset()
    notifyFromError.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it.each([504, 503, 429])('sends a POST exactly once when the gateway answers %s', async (status) => {
    fetchMock.mockResolvedValue(json(status))

    await settle(fetchAPI(COMMAND_URL, { method: 'POST', body: JSON.stringify({ value: 1 }) }))

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it.each(['PUT', 'PATCH', 'DELETE'])('sends a %s exactly once on a 504', async (method) => {
    fetchMock.mockResolvedValue(gatewayTimeout())

    await settle(fetchAPI('/devices/dev-1', { method }))

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('retries a GET on 503: the first attempt plus three retries', async () => {
    fetchMock.mockResolvedValue(json(503))

    await settle(fetchAPI('/devices'))

    expect(fetchMock).toHaveBeenCalledTimes(4)
  })

  it('retries a write only when the endpoint opts in', async () => {
    fetchMock.mockResolvedValue(json(503))

    await settle(fetchAPI('/devices', { method: 'POST', retryWrite: true }))

    expect(fetchMock).toHaveBeenCalledTimes(4)
  })

  it('still honours skipRetry for reads', async () => {
    fetchMock.mockResolvedValue(json(503))

    await settle(fetchAPI('/devices', { skipRetry: true }))

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reports a 504 on a write as "outcome unknown" rather than a plain failure', async () => {
    fetchMock.mockResolvedValue(gatewayTimeout())
    const expected = i18n.t('common:outcomeUnknown')

    const call = fetchAPI(COMMAND_URL, { method: 'POST' })
    const rejected = expect(call).rejects.toMatchObject({ status: 504, message: expected })
    await vi.runAllTimersAsync()
    await rejected

    expect(notifyFromError).toHaveBeenCalledWith(expected)
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reports a 502 on a write the same way', async () => {
    fetchMock.mockResolvedValue(json(502, { message: 'Bad Gateway' }))

    const call = fetchAPI(COMMAND_URL, { method: 'POST' })
    const rejected = expect(call).rejects.toMatchObject({ status: 502, message: i18n.t('common:outcomeUnknown') })
    await vi.runAllTimersAsync()
    await rejected

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reports a 503 on a write as outcome unknown too', async () => {
    fetchMock.mockResolvedValue(json(503, { message: 'Service Unavailable' }))

    const call = fetchAPI(COMMAND_URL, { method: 'POST' })
    const rejected = expect(call).rejects.toMatchObject({
      status: 503,
      message: i18n.t('common:outcomeUnknown'),
    })
    await vi.runAllTimersAsync()
    await rejected

    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('keeps the server message for a write the backend rejected outright', async () => {
    fetchMock.mockResolvedValue(json(400, { message: 'Device is offline' }))

    await expect(fetchAPI(COMMAND_URL, { method: 'POST' })).rejects.toMatchObject({ status: 400, message: 'Device is offline' })
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})

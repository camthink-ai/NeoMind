/// One user action = one write. Pressing a dashboard command button and
/// confirming sends the device command once. When the gateway then times out —
/// the server may already have dispatched it — the button must not send a
/// second one, and must tell the user the outcome is unknown instead of
/// claiming the command simply failed.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import i18n from '@/i18n/config'
import { CommandButton } from '../CommandButton'
import type { DataSource } from '@/types/dashboard'

const toast = vi.hoisted(() => vi.fn())
// Both the button and fetchAPI's own error toast go through this module.
vi.mock('@/hooks/use-toast', () => ({ useToast: () => ({ toast }), toast }))

const fetchMock = vi.fn<typeof fetch>()
const DEVICE_ID = 'dev-1'
const COMMAND_PATH = `/devices/${DEVICE_ID}/command/setValue`

const dataSource: DataSource = { type: 'command', sourceId: DEVICE_ID, command: 'setValue' }

/** What a reverse proxy hands back on an upstream timeout: an HTML page, no JSON. */
function gatewayTimeout() {
  return new Response('<html><body><center>504 Gateway Time-out</center></body></html>', { status: 504 })
}

function commandPosts(): unknown[][] {
  return fetchMock.mock.calls.filter(([url, init]) => (
    String(url).endsWith(COMMAND_PATH) && (init?.method ?? 'GET').toUpperCase() === 'POST'
  ))
}

// jsdom has no ResizeObserver; Radix primitives measure themselves on mount.
beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})

describe('CommandButton — a device command is sent once per confirmation', () => {
  beforeEach(() => {
    localStorage.clear()
    sessionStorage.clear()
    fetchMock.mockReset()
    toast.mockReset()
    vi.useFakeTimers({ shouldAdvanceTime: true })
    vi.stubGlobal('fetch', fetchMock)
    // Command definitions for the dialog; anything else is an empty object.
    fetchMock.mockImplementation(async (url) => {
      if (String(url).endsWith(`/devices/${DEVICE_ID}/current`)) {
        return new Response(JSON.stringify({
          metrics: {},
          commands: [{ name: 'setValue', display_name: 'Set value', parameters: [] }],
        }), { status: 200, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } })
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })

  it('sends the command exactly once when the gateway times out, and reports the outcome as unknown', async () => {
    render(<CommandButton dataSource={dataSource} title="Set value" />)

    // One user action: press the button, confirm in the dialog.
    fireEvent.click(screen.getByRole('button', { name: /Set value/ }))
    const confirm = await screen.findByRole('button', { name: 'commandButton.send' })
    await waitFor(() => expect(confirm).toBeEnabled())

    // "The server processed it, the client got a 504."
    fetchMock.mockImplementation(async (url) => (String(url).endsWith(COMMAND_PATH) ? gatewayTimeout() : new Response('{}')))
    fireEvent.click(confirm)

    const outcomeUnknown = i18n.t('common:outcomeUnknown')
    await waitFor(() => expect(toast).toHaveBeenCalledWith(expect.objectContaining({ description: outcomeUnknown })))

    // Let any would-be retry (500 ms backoff) have its chance before counting.
    await vi.advanceTimersByTimeAsync(10_000)
    expect(commandPosts()).toHaveLength(1)
    expect(toast).toHaveBeenCalledWith(expect.objectContaining({
      title: 'commandButton.commandFailed',
      description: outcomeUnknown,
    }))
  })
})

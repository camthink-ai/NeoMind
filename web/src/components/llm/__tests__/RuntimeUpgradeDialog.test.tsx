/// Rendering tests for the runtime upgrade dialog (badge/button states per
/// channel snapshot; API layer mocked — no server).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { RuntimeUpgradeDialog } from '../RuntimeUpgradeDialog'
import type { BuiltinRuntimeInfo } from '@/types/llm-backend'

vi.mock('@/lib/api', () => ({
  api: {
    getBuiltinRuntime: vi.fn(),
    checkBuiltinRuntimeUpdate: vi.fn(),
    upgradeBuiltinRuntime: vi.fn(),
    revertBuiltinRuntime: vi.fn(),
    setBuiltinRuntimeVariant: vi.fn(),
  },
}))
vi.mock('@/hooks/useEvents', () => ({ useEvents: vi.fn() }))
vi.mock('@/lib/notify', () => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }))

import { api } from '@/lib/api'

function snapshot(overrides: Partial<BuiltinRuntimeInfo> = {}): BuiltinRuntimeInfo {
  return {
    active: { variant: 'cpu', tag: 'b11609', source: 'bundled', path: '/bin/x' },
    bundled: { present: true, tag: 'b11609', usable: true, path: '/bin/x' },
    installed: [
      { variant: 'cpu', tag: 'b11700', source: 'cache', path: '/data/llama-server/b11700' },
    ],
    detected_variant: 'cpu',
    override_variant: null,
    pin: 'b11609',
    recommended: { tag: 'b11700', notes_zh: '修复', notes_en: 'fixes' },
    update_available: true,
    channel: 'ok',
    channel_error: null,
    upgrading: false,
    ...overrides,
  }
}

beforeEach(() => {
  vi.mocked(api.checkBuiltinRuntimeUpdate).mockReset()
  vi.mocked(api.getBuiltinRuntime).mockReset()
  vi.mocked(api.upgradeBuiltinRuntime).mockReset()
})

describe('RuntimeUpgradeDialog', () => {
  it('enables upgrade + revert when a newer runtime is recommended', async () => {
    vi.mocked(api.checkBuiltinRuntimeUpdate).mockResolvedValue(snapshot())
    render(
      <RuntimeUpgradeDialog open onClose={() => {}} runtime={null} onChanged={() => {}} />,
    )
    await waitFor(() => {
      expect(screen.getByText(/runtimeUpgradeTo/)).toBeTruthy()
    })
    // The recommended tag shows in the version line.
    expect(screen.getByText('b11700')).toBeTruthy()
    // Revert is enabled — one cache entry exists.
    const revert = screen.getByText(/runtimeRevert/).closest('button')
    expect(revert).not.toBeNull()
    expect(revert?.hasAttribute('disabled')).toBe(false)
  })

  it('disables upgrade when already up to date', async () => {
    vi.mocked(api.checkBuiltinRuntimeUpdate).mockResolvedValue(
      snapshot({ update_available: false, recommended: null, installed: [] }),
    )
    render(
      <RuntimeUpgradeDialog open onClose={() => {}} runtime={null} onChanged={() => {}} />,
    )
    await waitFor(() => {
      expect(screen.getByText(/runtimeUpToDate/)).toBeTruthy()
    })
    // No update → no upgrade CTA in the footer (a Close takes its place);
    // Revert stays reachable but disabled without cache entries.
    expect(screen.queryByText(/runtimeUpgradeTo/)).toBeNull()
    // No cache entry → revert disabled.
    const revert = screen.getByText(/runtimeRevert/).closest('button')
    expect(revert?.hasAttribute('disabled')).toBe(true)
  })

  it('surfaces an unreachable channel without failing the dialog', async () => {
    vi.mocked(api.checkBuiltinRuntimeUpdate).mockResolvedValue(
      snapshot({ channel: 'unreachable', recommended: null, update_available: false }),
    )
    render(
      <RuntimeUpgradeDialog open onClose={() => { }} runtime={null} onChanged={() => { }} />,
    )
    await waitFor(() => {
      expect(screen.getByText(/runtimeChannelUnreachable(?!Short)/)).toBeTruthy()
    })
  })
})

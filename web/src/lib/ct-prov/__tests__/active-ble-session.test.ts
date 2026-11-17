import { describe, expect, it, vi } from 'vitest'

import {
  hasActiveBleSession,
  releaseActiveBleSession,
  retainActiveBleSession,
} from '../active-ble-session'

describe('active-ble-session registry', () => {
  it('releases the retained session and clears the registry', () => {
    const release = vi.fn()
    retainActiveBleSession(release)
    expect(hasActiveBleSession()).toBe(true)
    releaseActiveBleSession()
    expect(release).toHaveBeenCalledTimes(1)
    expect(hasActiveBleSession()).toBe(false)
    releaseActiveBleSession()
    expect(release).toHaveBeenCalledTimes(1)
  })

  it('replacing a session releases the previous one', () => {
    const first = vi.fn()
    const second = vi.fn()
    retainActiveBleSession(first)
    retainActiveBleSession(second)
    expect(first).toHaveBeenCalledTimes(1)
    expect(hasActiveBleSession()).toBe(true)
    releaseActiveBleSession()
    expect(second).toHaveBeenCalledTimes(1)
    expect(hasActiveBleSession()).toBe(false)
  })

  it('unregister does not release', () => {
    const release = vi.fn()
    const unregister = retainActiveBleSession(release)
    unregister()
    expect(release).not.toHaveBeenCalled()
    expect(hasActiveBleSession()).toBe(false)
  })
})

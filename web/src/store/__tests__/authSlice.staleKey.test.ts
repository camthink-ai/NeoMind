/// A session that rests on an API key must be dropped only when the backend
/// says the key itself is invalid. `/auth/me` answers 401 for an unknown or
/// deactivated key and has no 403 branch, so a 403, a 5xx or a network failure
/// all keep the session. A 401 that arrives after the user switched keys
/// belongs to the old key and must not clear the new one.
/// When the key checks out, the service account the backend returns is
/// published as the signed-in user, and a late answer for an old key must not
/// overwrite the current one.
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest'
import { create } from 'zustand'
import { createAuthSlice, type AuthSlice } from '../slices/authSlice'
import { api } from '@/lib/api'
import { tokenManager } from '@/lib/auth'
import { clearApiKey, getApiKey, setApiKey } from '@/lib/urls'
import type { UserInfo } from '@/types'

const useAuth = create<AuthSlice>()(createAuthSlice)

/** An error as fetchAPI throws it: a message, plus a `status` when the server answered. */
function failure(status?: number) {
  return Object.assign(new Error(status ? `server answered ${status}` : 'Failed to fetch'), { status })
}

function pendingVerification() {
  let resolve!: (user: UserInfo) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<UserInfo>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

/** The virtual service account `/auth/me` returns for an API key. */
const keyAUser: UserInfo = { id: 'apikey:a', username: 'key-a', role: 'admin', created_at: 0 }
const keyBUser: UserInfo = { id: 'apikey:b', username: 'key-b', role: 'user', created_at: 0 }

describe('checkAuthStatus with an API key session', () => {
  let verification: ReturnType<typeof pendingVerification>
  let getCurrentUser: ReturnType<typeof vi.spyOn>
  let warn: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    localStorage.clear()
    sessionStorage.clear()
    tokenManager.clearAll()
    clearApiKey()
    warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    // Signed in on key A; its check against the backend is still in flight.
    setApiKey('key-A')
    verification = pendingVerification()
    getCurrentUser = vi.spyOn(api, 'getCurrentUser').mockReturnValue(verification.promise)
    useAuth.getState().checkAuthStatus()
    expect(useAuth.getState().isAuthenticated).toBe(true)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    clearApiKey()
    tokenManager.clearAll()
  })

  it('publishes the service account when the key checks out', async () => {
    verification.resolve(keyAUser)
    await flush()

    expect(useAuth.getState().user).toEqual(keyAUser)
    expect(tokenManager.getUser()).toEqual(keyAUser)
    expect(getApiKey()).toBe('key-A')
    expect(useAuth.getState().isAuthenticated).toBe(true)
  })

  it('signs out when the backend answers 401', async () => {
    tokenManager.setUser(keyAUser, false)
    verification.reject(failure(401))
    await flush()

    expect(getApiKey()).toBe('')
    expect(useAuth.getState().isAuthenticated).toBe(false)
    expect(useAuth.getState().user).toBeNull()
    expect(tokenManager.getUser()).toBeNull()
    expect(warn).not.toHaveBeenCalled()
  })

  it.each([403, 500])('keeps the session when the backend answers %s', async (status) => {
    verification.reject(failure(status))
    await flush()

    expect(getApiKey()).toBe('key-A')
    expect(useAuth.getState().isAuthenticated).toBe(true)
    expect(warn).toHaveBeenCalledTimes(1)
  })

  it('keeps the session when the check fails on the network', async () => {
    verification.reject(failure())
    await flush()

    expect(getApiKey()).toBe('key-A')
    expect(useAuth.getState().isAuthenticated).toBe(true)
    expect(warn).toHaveBeenCalledTimes(1)
  })

  it('does not let a late 401 for the old key clear the key the user switched to', async () => {
    setApiKey('key-B')
    verification.reject(failure(401))
    await flush()

    expect(getApiKey()).toBe('key-B')
    expect(useAuth.getState().isAuthenticated).toBe(true)
  })

  it('does not let a late result for the old key overwrite the new key’s user', async () => {
    // The user switches to key B, and B's own check finishes first …
    const keyBCheck = pendingVerification()
    getCurrentUser.mockReturnValueOnce(keyBCheck.promise)
    setApiKey('key-B')
    useAuth.getState().checkAuthStatus()
    keyBCheck.resolve(keyBUser)
    await flush()
    expect(useAuth.getState().user).toEqual(keyBUser)

    // … only then does key A's answer arrive.
    verification.resolve(keyAUser)
    await flush()

    expect(useAuth.getState().user).toEqual(keyBUser)
    expect(tokenManager.getUser()).toEqual(keyBUser)
  })
})

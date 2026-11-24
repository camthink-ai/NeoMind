/// The legacy "remember me" entry stored the login password in plaintext.
/// Startup purges it: only that one key goes, the call is safe to repeat, and
/// a storage that refuses to answer must not break app boot.
import { describe, it, expect, beforeEach } from 'vitest'
import { LEGACY_CREDENTIALS_KEY, purgeLegacyCredentials } from '../legacyCredentials'

describe('purgeLegacyCredentials', () => {
  beforeEach(() => {
    localStorage.clear()
    sessionStorage.clear()
  })

  it('removes the legacy plaintext credentials and nothing else', () => {
    localStorage.setItem(LEGACY_CREDENTIALS_KEY, JSON.stringify({ username: 'ming', password: 'hunter2', rememberMe: true }))
    localStorage.setItem('neomind_token', 'jwt-token')
    localStorage.setItem('neomind_remembered_username', 'ming')
    localStorage.setItem('neomind-store', '{"state":{"sessionId":"s1"},"version":3}')

    expect(purgeLegacyCredentials()).toBe(true)

    expect(localStorage.getItem(LEGACY_CREDENTIALS_KEY)).toBeNull()
    expect(localStorage.getItem('neomind_token')).toBe('jwt-token')
    expect(localStorage.getItem('neomind_remembered_username')).toBe('ming')
    expect(localStorage.getItem('neomind-store')).toBe('{"state":{"sessionId":"s1"},"version":3}')
  })

  it('leaves the stored session alone, so an upgraded user stays signed in', () => {
    sessionStorage.setItem('neomind_token_session', 'session-jwt')
    sessionStorage.setItem('neomind_user_session', '{"username":"ming"}')
    localStorage.setItem(LEGACY_CREDENTIALS_KEY, JSON.stringify({ username: 'ming', password: 'hunter2' }))

    purgeLegacyCredentials()

    expect(sessionStorage.getItem('neomind_token_session')).toBe('session-jwt')
    expect(sessionStorage.getItem('neomind_user_session')).toBe('{"username":"ming"}')
  })

  it('is a no-op when there is nothing to purge, and safe to repeat', () => {
    expect(purgeLegacyCredentials()).toBe(false)
    expect(purgeLegacyCredentials()).toBe(false)
  })

  it('does not throw when storage is unavailable', () => {
    const refused = () => { throw new DOMException('Access is denied for this document.', 'SecurityError') }

    expect(() => purgeLegacyCredentials({ getItem: refused, removeItem: refused })).not.toThrow()
    expect(purgeLegacyCredentials({ getItem: refused, removeItem: refused })).toBe(false)
  })
})

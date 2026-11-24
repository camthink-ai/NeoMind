/**
 * Legacy "remember me" credential cleanup.
 *
 * Older login pages persisted `{ username, password }` in plaintext under this
 * key whenever "remember me" was ticked. The JWT already keeps the session
 * signed in, so the password copy was never needed — and every same-origin
 * script (community widget and extension bundles included) could read it.
 *
 * Runs at app startup rather than on the login page: a user who is still
 * signed in after an upgrade never visits the login page, which is exactly
 * the long-term "remember me" user this has to reach.
 */

export const LEGACY_CREDENTIALS_KEY = 'neomind_remembered_credentials'

/**
 * Remove the legacy plaintext credentials. Idempotent and touches nothing else.
 * @returns true if a legacy entry was found and removed.
 */
export function purgeLegacyCredentials(
  storage?: Pick<Storage, 'getItem' | 'removeItem'>,
): boolean {
  try {
    const store = storage ?? globalThis.localStorage
    if (!store.getItem(LEGACY_CREDENTIALS_KEY)) return false
    store.removeItem(LEGACY_CREDENTIALS_KEY)
    return true
  } catch {
    // Storage unavailable (blocked, disabled) — nothing can be purged
    return false
  }
}

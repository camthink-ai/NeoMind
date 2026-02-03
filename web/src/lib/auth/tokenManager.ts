/**
 * Unified Token Manager
 *
 * Centralized JWT token management for authentication.
 * Handles localStorage/sessionStorage storage with migration support.
 */

import type { UserInfo } from '@/types'

// ============================================================================
// Constants
// ============================================================================

const TOKEN_KEY = 'neomind_token'
const TOKEN_KEY_SESSION = 'neomind_token_session'
const USER_KEY = 'neomind_user'
const USER_KEY_SESSION = 'neomind_user_session'

// Legacy keys for migration
const OLD_TOKEN_KEY = 'neotalk_token'
const OLD_TOKEN_KEY_SESSION = 'neotalk_token_session'
const OLD_USER_KEY = 'neotalk_user'
const OLD_USER_KEY_SESSION = 'neotalk_user_session'

// ============================================================================
// Token Manager
// ============================================================================

class TokenManagerClass {
  private migrated = false

  /**
   * Get the current authentication token from storage.
   * Tries new keys first, falls back to legacy keys with automatic migration.
   */
  getToken(): string | null {
    // Try new keys first
    let token = localStorage.getItem(TOKEN_KEY) || sessionStorage.getItem(TOKEN_KEY_SESSION)

    // Perform one-time migration if token not found
    if (!token && !this.migrated) {
      token = this.migrateToken()
    }

    return token
  }

  /**
   * Store the authentication token.
   * @param token - The JWT token to store
   * @param remember - If true, uses localStorage (persists across sessions).
   *                   If false, uses sessionStorage (cleared on browser close).
   */
  setToken(token: string, remember: boolean = false): void {
    if (remember) {
      localStorage.setItem(TOKEN_KEY, token)
      sessionStorage.removeItem(TOKEN_KEY_SESSION)
    } else {
      sessionStorage.setItem(TOKEN_KEY_SESSION, token)
      localStorage.removeItem(TOKEN_KEY)
    }
  }

  /**
   * Clear the authentication token from all storage locations.
   */
  clearToken(): void {
    localStorage.removeItem(TOKEN_KEY)
    sessionStorage.removeItem(TOKEN_KEY_SESSION)
    // Also clear legacy keys
    localStorage.removeItem(OLD_TOKEN_KEY)
    sessionStorage.removeItem(OLD_TOKEN_KEY_SESSION)
  }

  /**
   * Check if a token exists in any storage location.
   */
  hasToken(): boolean {
    return !!(
      localStorage.getItem(TOKEN_KEY) ||
      sessionStorage.getItem(TOKEN_KEY_SESSION)
    )
  }

  // ========================================================================
  // User Info Management
  // ========================================================================

  /**
   * Get the current user information from storage.
   */
  getUser(): UserInfo | null {
    let userStr = localStorage.getItem(USER_KEY) || sessionStorage.getItem(USER_KEY_SESSION)

    // Try migration if user not found
    if (!userStr && !this.migrated) {
      userStr = this.migrateUser()
    }

    if (userStr) {
      try {
        return JSON.parse(userStr)
      } catch {
        return null
      }
    }
    return null
  }

  /**
   * Store user information.
   * @param user - The user info to store
   * @param remember - If true, uses localStorage; otherwise sessionStorage
   */
  setUser(user: UserInfo, remember: boolean = false): void {
    const userStr = JSON.stringify(user)
    if (remember) {
      localStorage.setItem(USER_KEY, userStr)
      sessionStorage.removeItem(USER_KEY_SESSION)
    } else {
      sessionStorage.setItem(USER_KEY_SESSION, userStr)
      localStorage.removeItem(USER_KEY)
    }
  }

  /**
   * Clear user information from all storage locations.
   */
  clearUser(): void {
    localStorage.removeItem(USER_KEY)
    sessionStorage.removeItem(USER_KEY_SESSION)
    // Also clear legacy keys
    localStorage.removeItem(OLD_USER_KEY)
    sessionStorage.removeItem(OLD_USER_KEY_SESSION)
  }

  /**
   * Clear both token and user information.
   * Convenience method for logout.
   */
  clearAll(): void {
    this.clearToken()
    this.clearUser()
  }

  // ========================================================================
  // Migration (Private)
  // ========================================================================

  /**
   * Migrate legacy token keys to new keys.
   * Only runs once per session.
   */
  private migrateToken(): string | null {
    this.migrated = true

    const oldLocalToken = localStorage.getItem(OLD_TOKEN_KEY)
    const oldSessionToken = sessionStorage.getItem(OLD_TOKEN_KEY_SESSION)

    if (oldLocalToken) {
      // Migrate from localStorage
      localStorage.setItem(TOKEN_KEY, oldLocalToken)
      localStorage.removeItem(OLD_TOKEN_KEY)
      return oldLocalToken
    }

    if (oldSessionToken) {
      // Migrate from sessionStorage
      sessionStorage.setItem(TOKEN_KEY_SESSION, oldSessionToken)
      sessionStorage.removeItem(OLD_TOKEN_KEY_SESSION)
      return oldSessionToken
    }

    return null
  }

  /**
   * Migrate legacy user keys to new keys.
   */
  private migrateUser(): string | null {
    this.migrated = true

    const oldLocalUser = localStorage.getItem(OLD_USER_KEY)
    const oldSessionUser = sessionStorage.getItem(OLD_USER_KEY_SESSION)

    if (oldLocalUser) {
      localStorage.setItem(USER_KEY, oldLocalUser)
      localStorage.removeItem(OLD_USER_KEY)
      return oldLocalUser
    }

    if (oldSessionUser) {
      sessionStorage.setItem(USER_KEY_SESSION, oldSessionUser)
      sessionStorage.removeItem(OLD_USER_KEY_SESSION)
      return oldSessionUser
    }

    return null
  }

  /**
   * Force migration to run again (for testing).
   */
  resetMigrationFlag(): void {
    this.migrated = false
  }
}

// ============================================================================
// Singleton Instance
// ============================================================================

export const tokenManager = new TokenManagerClass()

// ============================================================================
// Convenience Exports
// ============================================================================

export const {
  getToken,
  setToken,
  clearToken,
  hasToken,
  getUser,
  setUser,
  clearUser,
  clearAll,
} = tokenManager

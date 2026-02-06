/**
 * Unified Token Manager
 *
 * Centralized JWT token management for authentication.
 */

import type { UserInfo } from '@/types'

// ============================================================================
// Constants
// ============================================================================

const TOKEN_KEY = 'neomind_token'
const TOKEN_KEY_SESSION = 'neomind_token_session'
const USER_KEY = 'neomind_user'
const USER_KEY_SESSION = 'neomind_user_session'

// ============================================================================
// Token Manager
// ============================================================================

class TokenManagerClass {
  /**
   * Get the current authentication token from storage.
   */
  getToken(): string | null {
    return localStorage.getItem(TOKEN_KEY) || sessionStorage.getItem(TOKEN_KEY_SESSION)
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
    const userStr = localStorage.getItem(USER_KEY) || sessionStorage.getItem(USER_KEY_SESSION)

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
  }

  /**
   * Clear both token and user information.
   * Convenience method for logout.
   */
  clearAll(): void {
    this.clearToken()
    this.clearUser()
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

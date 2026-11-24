/**
 * Authentication Slice
 *
 * Handles user authentication with JWT tokens.
 */

import type { StateCreator } from 'zustand'
import type { AuthState } from '../types'
import type { UserInfo } from '@/types'
import { tokenManager, api, getApiKey, clearApiKey } from '@/lib/api'
import { logWarning } from '@/lib/errors'

export interface AuthSlice extends AuthState {
  // UI state
  loginError: string | null
  // Actions
  checkAuthStatus: () => void
  setLoginError: (error: string | null) => void
  // User authentication actions
  login: (username: string, password: string, rememberMe?: boolean) => Promise<void>
  register: (username: string, password: string) => Promise<void>
  logout: () => Promise<void>
  getCurrentUser: () => Promise<UserInfo | null>
}

export const createAuthSlice: StateCreator<
  AuthSlice,
  [],
  [],
  AuthSlice
> = (set, get) => ({
  // Initial state
  apiKey: null,
  isAuthenticated: false,
  user: tokenManager.getUser(),  // Restore from cache
  token: tokenManager.getToken(),
  loginError: null,

  // Actions
  checkAuthStatus: () => {
    // Check for JWT token or API key
    const token = tokenManager.getToken()
    const apiKey = getApiKey()
    const cachedUser = tokenManager.getUser()
    if (token) {
      set({ token, isAuthenticated: true, user: cachedUser })
      // Try to fetch current user info
      get().getCurrentUser().catch((error) => {
        // Only clear token and user on 401/403 (auth errors)
        if (error?.status === 401 || error?.status === 403) {
          tokenManager.clearToken()
          tokenManager.clearUser()
          set({ token: null, user: null, isAuthenticated: false })
        }
        // On 500 or other errors, keep cached user info
      })
    } else if (apiKey) {
      // API key auth — no JWT token, but authenticated via service account
      set({ token: null, isAuthenticated: true, user: null })
      // Check the key against the backend: a stale one (e.g. the backend data
      // was wiped) would otherwise leave isAuthenticated true and the app
      // looping between rendering protected pages and bouncing off 401s.
      // Calls the API directly — getCurrentUser() swallows every error, so it
      // does the success bookkeeping inline instead.
      // `/auth/me` is guarded by hybrid_auth_middleware, which answers 401 for
      // an unknown or deactivated key and has no 403 branch. So only a 401
      // means the key itself is invalid; on 403, a 5xx or a network failure
      // the key may still be good and the session is kept.
      const usedKey = apiKey
      api.getCurrentUser()
        .then((user) => {
          // The service account is the signed-in user (sidebar, mobile nav and
          // chat read `user`). Session-only cache: an API-key sign-in is never
          // "remembered". A result arriving after the key was replaced belongs
          // to the old key.
          if (getApiKey() !== usedKey) return
          tokenManager.setUser(user, false)
          set({ user })
        })
        .catch((error) => {
          // A 401 arriving after the key was replaced belongs to the old one.
          if (getApiKey() !== usedKey) return
          if (error?.status === 401) {
            clearApiKey()
            tokenManager.clearUser()
            set({ isAuthenticated: false, user: null })
            return
          }
          logWarning('API key check failed without a 401; keeping the session', { status: error?.status ?? null })
        })
    } else {
      set({ isAuthenticated: false, user: null })
    }
  },

  saveApiKey: () => {
    // No-op - API key authentication removed
  },

  clearApiKey: () => {
    // No-op - API key authentication removed
  },

  // User authentication actions
  login: async (username: string, password: string, rememberMe = false) => {
    const response = await api.login(username, password, rememberMe)
    // Cache user info
    tokenManager.setUser(response.user, rememberMe)
    set({
      user: response.user,
      token: response.token,
      isAuthenticated: true,
    })
  },

  register: async (username: string, password: string) => {
    const response = await api.register(username, password)
    // Cache user info (session only for register)
    tokenManager.setUser(response.user, false)
    set({
      user: response.user,
      token: response.token,
      isAuthenticated: true,
    })
  },

  logout: async () => {
    try {
      await api.logout()
    } catch {
      // Ignore errors during logout
    } finally {
      tokenManager.clearToken()
      tokenManager.clearUser()
      // Drop the previous user's dashboard cache, current-dashboard pointer,
      // and local→server ID mapping so none of it leaks into the next
      // account's session (dynamic import breaks the slice ↔ store cycle).
      try {
        const { useStore } = await import('@/store')
        useStore.getState().clearDashboards()
      } catch {
        // Store not mounted yet — nothing cached to clear
      }
      set({
        user: null,
        token: null,
        isAuthenticated: false,
      })
    }
  },

  getCurrentUser: async () => {
    try {
      const user = await api.getCurrentUser()
      // Update cache with latest user info
      const remember = !!localStorage.getItem('neomind_token')
      tokenManager.setUser(user, remember)
      set({ user })
      return user
    } catch {
      return null
    }
  },

  setLoginError: (error: string | null) => {
    set({ loginError: error })
  },
})

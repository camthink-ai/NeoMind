/**
 * Authentication Slice
 *
 * Handles user authentication with JWT tokens.
 */

import type { StateCreator } from 'zustand'
import type { AuthState } from '../types'
import type { UserInfo } from '@/types'
import { tokenManager, api, getApiKey, clearApiKey } from '@/lib/api'

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
      // Fetch service account info (backend returns virtual user for API key).
      // A 401/403 means the key is stale (e.g. the backend data was wiped) —
      // clear it, otherwise isAuthenticated stays true and the app loops
      // between rendering protected pages and bouncing off 401s.
      get().getCurrentUser().catch((error) => {
        if (error?.status === 401 || error?.status === 403) {
          clearApiKey()
          set({ isAuthenticated: false, user: null })
        }
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

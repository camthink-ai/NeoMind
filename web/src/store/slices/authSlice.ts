/**
 * Authentication Slice
 *
 * Handles user authentication with JWT tokens.
 */

import type { StateCreator } from 'zustand'
import type { AuthState } from '../types'
import type { UserInfo } from '@/types'
import { tokenManager, api } from '@/lib/api'

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
    // Check for JWT token
    const token = tokenManager.getToken()
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
      const remember = !!localStorage.getItem('neotalk_token')
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

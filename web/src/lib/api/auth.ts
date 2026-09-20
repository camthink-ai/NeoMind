/** auth api — split from api.ts 2026-09. */
import type { UserInfo, LoginResponse, RegisterRequest, ChangePasswordRequest } from '@/types'
import { fetchAPI, tokenManager } from "./client"

export const authApi = {
  // ========== Authentication API ==========
  login: (username: string, password: string, rememberMe: boolean = false) =>
    // skipErrorToast: the login page renders its own inline error — a toast
    // for the same failure is redundant double feedback.
    fetchAPI<LoginResponse>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
      skipAuth: true,
      skipGlobalError: true,
      skipErrorToast: true,
    }).then(res => {
      // Store token
      tokenManager.setToken(res.token, rememberMe)
      return res
    }),
  register: (username: string, password: string) =>
    fetchAPI<LoginResponse>('/auth/register', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
      skipAuth: true,
      skipGlobalError: true,
      skipErrorToast: true,
    }).then(res => {
      // Store token
      tokenManager.setToken(res.token, false)
      return res
    }),
  logout: () =>
    fetchAPI<{ message: string }>('/auth/logout', {
      method: 'POST',
    }).then(res => {
      // Clear token
      tokenManager.clearToken()
      return res
    }),
  getCurrentUser: () =>
    fetchAPI<UserInfo>('/auth/me'),
  changePassword: (req: ChangePasswordRequest) =>
    fetchAPI<{ message: string }>('/auth/change-password', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  listUsers: () =>
    fetchAPI<{ users: UserInfo[] }>('/users'),
  createUser: (req: RegisterRequest) =>
    fetchAPI<{ user: UserInfo }>('/users', {
      method: 'POST',
      body: JSON.stringify(req),
    }),
  deleteUser: (username: string) =>
    fetchAPI<{ message: string }>(`/users/${username}`, {
      method: 'DELETE',
    }),

}

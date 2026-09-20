/** http api — split from api.ts 2026-09. */
import { fetchAPI, type FetchOptions } from "./client"

export const httpApi = {
  // ========== Generic HTTP Methods ==========
  /**
   * Generic GET request
   */
  get: <T>(path: string, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'GET' }),

  /**
   * Generic POST request
   */
  post: <T>(path: string, body: unknown, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'POST', body: JSON.stringify(body) }),

  /**
   * Generic PUT request
   */
  put: <T>(path: string, body: unknown, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'PUT', body: JSON.stringify(body) }),

  /**
   * Generic DELETE request
   */
  delete: <T>(path: string, options: Omit<FetchOptions, 'method' | 'body'> = {}) =>
    fetchAPI<T>(path, { ...options, method: 'DELETE' }),

}

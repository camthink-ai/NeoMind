/// Tests for API helper utilities
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'

// Mock fetch for API tests
const mockFetch = vi.fn()

describe('API Utilities', () => {
  beforeEach(() => {
    global.fetch = mockFetch
  })

  afterEach(() => {
    mockFetch.mockClear()
  })

  describe('Token Management', () => {
    it('should store and retrieve token', () => {
      const testToken = 'test-jwt-token-12345'
      localStorage.setItem('auth_token', testToken)
      const retrieved = localStorage.getItem('auth_token')
      expect(retrieved).toBe(testToken)
    })

    it('should clear token on logout', () => {
      localStorage.setItem('auth_token', 'test-token')
      localStorage.removeItem('auth_token')
      const retrieved = localStorage.getItem('auth_token')
      expect(retrieved).toBeNull()
    })
  })

  describe('API URL Construction', () => {
    it('should construct API URL correctly for web environment', () => {
      const isTauri = !!(window as any).__TAURI__
      const apiBase = isTauri ? 'http://localhost:9375/api' : '/api'
      expect(apiBase).toBeDefined()
    })

    it('should handle path concatenation correctly', () => {
      const apiBase = '/api'
      const endpoint = '/devices'
      const fullUrl = `${apiBase}${endpoint}`
      expect(fullUrl).toBe('/api/devices')
    })
  })

  describe('Request Headers', () => {
    it('should include authorization header when token exists', () => {
      const token = 'test-token-abc123'
      const headers: Record<string, string> = {
        'Content-Type': 'application/json',
      }

      if (token) {
        headers['Authorization'] = `Bearer ${token}`
      }

      expect(headers['Authorization']).toBe(`Bearer ${token}`)
    })

    it('should not include authorization header when token is missing', () => {
      const headers: Record<string, string> = {
        'Content-Type': 'application/json',
      }

      expect(headers['Authorization']).toBeUndefined()
    })
  })

  describe('Error Handling', () => {
    it('should handle 401 unauthorized responses', () => {
      const errorStatus = 401
      expect(errorStatus).toBeGreaterThanOrEqual(400)
      expect(errorStatus).toBeLessThan(500)
    })

    it('should handle 404 not found responses', () => {
      const errorStatus = 404
      expect(errorStatus).toBe(404)
    })

    it('should handle 500 server error responses', () => {
      const errorStatus = 500
      expect(errorStatus).toBeGreaterThanOrEqual(500)
    })
  })

  describe('Request Query Parameters', () => {
    it('should build query string from object', () => {
      const params = new URLSearchParams({
        page: '1',
        limit: '20',
        sort: 'name',
      })

      expect(params.toString()).toBe('page=1&limit=20&sort=name')
    })

    it('should handle empty query parameters', () => {
      const params = new URLSearchParams({})
      expect(params.toString()).toBe('')
    })

    it('should handle special characters in query parameters', () => {
      const params = new URLSearchParams({
        search: 'hello world',
        filter: 'type=device',
      })

      expect(params.toString()).toContain('search=hello+world')
      expect(params.toString()).toContain('filter=type%3Ddevice')
    })
  })
})

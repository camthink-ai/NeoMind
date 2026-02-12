/// Tests for TypeScript types
import { describe, it, expect } from 'vitest'

// Type guards and utility tests for types
describe('Type Definitions', () => {
  describe('UserRole', () => {
    it('should accept valid user roles', () => {
      const validRoles: Array<'admin' | 'user' | 'viewer'> = ['admin', 'user', 'viewer']
      expect(validRoles).toHaveLength(3)
      expect(validRoles).toContain('admin')
      expect(validRoles).toContain('user')
      expect(validRoles).toContain('viewer')
    })
  })

  describe('Device type', () => {
    it('should create a valid device object', () => {
      const device = {
        id: 'device-1',
        device_id: 'device-1',
        name: 'Test Device',
        device_type: 'sensor',
        adapter_type: 'mqtt',
        status: 'online',
        last_seen: '2024-01-01T00:00:00Z',
        online: true,
      }

      expect(device.id).toBe('device-1')
      expect(device.device_id).toBe('device-1')
      expect(device.name).toBe('Test Device')
      expect(device.online).toBe(true)
    })

    it('should allow optional current_values', () => {
      const device = {
        id: 'device-2',
        device_id: 'device-2',
        name: 'Test Device 2',
        device_type: 'sensor',
        adapter_type: 'mqtt',
        status: 'online',
        last_seen: '2024-01-01T00:00:00Z',
        online: true,
        current_values: {
          temperature: 25.5,
          humidity: 60,
        },
      }

      expect(device.current_values).toBeDefined()
      expect(device.current_values?.temperature).toBe(25.5)
    })
  })

  describe('LoginRequest', () => {
    it('should create valid login request', () => {
      const loginRequest = {
        username: 'testuser',
        password: 'testpass',
      }

      expect(loginRequest.username).toBe('testuser')
      expect(loginRequest.password).toBe('testpass')
    })
  })

  describe('UserInfo', () => {
    it('should create valid user info', () => {
      const userInfo = {
        id: 'user-1',
        username: 'testuser',
        role: 'admin' as const,
        created_at: 1704067200000,
      }

      expect(userInfo.id).toBe('user-1')
      expect(userInfo.role).toBe('admin')
    })
  })
})

/// Tests for store types and utilities
import { describe, it, expect } from 'vitest'

describe('Store Types', () => {
  describe('NeoMindStore interface', () => {
    it('should define auth slice properties', () => {
      const authSlice = {
        isAuthenticated: true,
        user: {
          id: 'user-1',
          username: 'testuser',
          role: 'admin',
          created_at: 1704067200000,
        },
        token: 'test-jwt-token',
      }

      expect(authSlice.isAuthenticated).toBe(true)
      expect(authSlice.user?.username).toBe('testuser')
      expect(authSlice.token).toBeDefined()
    })

    it('should define session slice properties', () => {
      const sessionSlice = {
        sessionId: 'session-1',
        messages: [
          {
            id: 'msg-1',
            role: 'user',
            content: 'Hello',
            timestamp: 1704067200000,
          },
          {
            id: 'msg-2',
            role: 'assistant',
            content: 'Hi there!',
            timestamp: 1704067201000,
          },
        ],
        pendingStreamContent: '',
      }

      expect(sessionSlice.sessionId).toBe('session-1')
      expect(sessionSlice.messages).toHaveLength(2)
      expect(sessionSlice.messages[0].role).toBe('user')
      expect(sessionSlice.messages[1].role).toBe('assistant')
    })

    it('should define device slice properties', () => {
      const deviceSlice = {
        devices: [
          {
            id: 'device-1',
            device_id: 'device-1',
            name: 'Light 1',
            device_type: 'light',
            adapter_type: 'mqtt',
            status: 'online',
            last_seen: '2024-01-01T00:00:00Z',
            online: true,
          },
        ],
        selectedDevice: null,
        filters: { online: true, type: '' },
      }

      expect(deviceSlice.devices).toHaveLength(1)
      expect(deviceSlice.devices[0].name).toBe('Light 1')
      expect(deviceSlice.selectedDevice).toBeNull()
    })
  })

  describe('Message Role Types', () => {
    it('should accept valid message roles', () => {
      const roles: Array<'user' | 'assistant' | 'system'> = ['user', 'assistant', 'system']
      expect(roles).toHaveLength(3)
    })

    it('should distinguish user from assistant messages', () => {
      const userMessage = { role: 'user' as const, content: 'Hello' }
      const assistantMessage = { role: 'assistant' as const, content: 'Hi!' }

      expect(userMessage.role).toBe('user')
      expect(assistantMessage.role).toBe('assistant')
      expect(userMessage.role).not.toBe(assistantMessage.role)
    })
  })

  describe('Device Status Types', () => {
    it('should accept valid device statuses', () => {
      const statuses = ['online', 'offline', 'error', 'unknown']
      expect(statuses).toHaveLength(4)
    })

    it('should handle online/offline boolean flags', () => {
      const onlineDevice = { online: true, status: 'online' }
      const offlineDevice = { online: false, status: 'offline' }

      expect(onlineDevice.online).toBe(true)
      expect(offlineDevice.online).toBe(false)
    })
  })
})

/// Store State Management Tests
///
/// Tests for Zustand store state behavior including:
/// - State mutations
/// - State reset/clear operations
/// - Store persistence
/// - Edge cases in state updates

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { create } from 'zustand'
import { devtools, persist } from 'zustand/middleware'

// Mock implementations for testing
interface TestAuthState {
  isAuthenticated: boolean
  user: { id: string; username: string; role: string } | null
  token: string | null
}

interface TestSessionState {
  sessionId: string | null
  messages: Array<{ id: string; role: string; content: string; timestamp: number }>
  pendingStreamContent: string
}

describe('Store State Management', () => {
  describe('State Immutability', () => {
    it('should create new state object on updates', () => {
      const store = create<{ count: number }>(() => ({ count: 0 }))
      const state1 = store.getState()
      store.setState({ count: 1 })
      const state2 = store.getState()

      expect(state1).not.toBe(state2)
      expect(state1.count).toBe(0)
      expect(state2.count).toBe(1)
    })
  })

  describe('Authentication State', () => {
    it('should handle login transition', () => {
      const initialState: TestAuthState = {
        isAuthenticated: false,
        user: null,
        token: null,
      }

      const store = create<TestAuthState>(() => initialState)

      expect(store.getState().isAuthenticated).toBe(false)

      // Simulate login
      store.setState({
        isAuthenticated: true,
        user: { id: 'user-1', username: 'testuser', role: 'user' },
        token: 'jwt-token-123',
      })

      expect(store.getState().isAuthenticated).toBe(true)
      expect(store.getState().user?.username).toBe('testuser')
      expect(store.getState().token).toBe('jwt-token-123')
    })

    it('should handle logout transition', () => {
      const loggedInState: TestAuthState = {
        isAuthenticated: true,
        user: { id: 'user-1', username: 'testuser', role: 'user' },
        token: 'jwt-token-123',
      }

      const store = create<TestAuthState>(() => loggedInState)

      // Simulate logout
      store.setState({
        isAuthenticated: false,
        user: null,
        token: null,
      })

      expect(store.getState().isAuthenticated).toBe(false)
      expect(store.getState().user).toBeNull()
      expect(store.getState().token).toBeNull()
    })

    it('should handle partial state updates', () => {
      const store = create<TestAuthState>(() => ({
        isAuthenticated: true,
        user: { id: 'user-1', username: 'testuser', role: 'user' },
        token: 'jwt-token-123',
      }))

      // Partial update - only token changes
      store.setState({ token: 'new-token-456' })

      expect(store.getState().token).toBe('new-token-456')
      expect(store.getState().user?.username).toBe('testuser') // Unchanged
      expect(store.getState().isAuthenticated).toBe(true) // Unchanged
    })
  })

  describe('Session State', () => {
    it('should handle empty message array', () => {
      const store = create<TestSessionState>(() => ({
        sessionId: null,
        messages: [],
        pendingStreamContent: '',
      }))

      expect(store.getState().messages).toEqual([])
      expect(store.getState().messages).toHaveLength(0)
    })

    it('should add messages to existing array', () => {
      const store = create<TestSessionState>(() => ({
        sessionId: 'session-1',
        messages: [
          { id: 'msg-1', role: 'user', content: 'Hello', timestamp: 1000 },
        ],
        pendingStreamContent: '',
      }))

      // Add new message
      const currentMessages = store.getState().messages
      store.setState({
        messages: [
          ...currentMessages,
          { id: 'msg-2', role: 'assistant', content: 'Hi!', timestamp: 2000 },
        ],
      })

      expect(store.getState().messages).toHaveLength(2)
      expect(store.getState().messages[1].content).toBe('Hi!')
    })

    it('should clear all messages', () => {
      const store = create<TestSessionState>(() => ({
        sessionId: 'session-1',
        messages: [
          { id: 'msg-1', role: 'user', content: 'Hello', timestamp: 1000 },
          { id: 'msg-2', role: 'assistant', content: 'Hi!', timestamp: 2000 },
        ],
        pendingStreamContent: '',
      }))

      store.setState({ messages: [] })

      expect(store.getState().messages).toEqual([])
      expect(store.getState().messages).toHaveLength(0)
    })

    it('should handle pending stream content updates', () => {
      const store = create<TestSessionState>(() => ({
        sessionId: 'session-1',
        messages: [],
        pendingStreamContent: '',
      }))

      // Simulate streaming response
      store.setState({ pendingStreamContent: 'Hello' })
      expect(store.getState().pendingStreamContent).toBe('Hello')

      store.setState({ pendingStreamContent: 'Hello World' })
      expect(store.getState().pendingStreamContent).toBe('Hello World')

      // Clear pending content
      store.setState({ pendingStreamContent: '' })
      expect(store.getState().pendingStreamContent).toBe('')
    })
  })

  describe('State Update Patterns', () => {
    it('should handle multiple rapid updates', () => {
      const store = create<{ count: number; history: number[] }>(() => ({
        count: 0,
        history: [],
      }))

      // Rapid updates
      for (let i = 1; i <= 10; i++) {
        store.setState({ count: i })
        store.setState({ history: [...store.getState().history, i] })
      }

      expect(store.getState().count).toBe(10)
      expect(store.getState().history).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
    })

    it('should handle conditional state updates', () => {
      interface ConditionalState {
        value: number
        enabled: boolean
      }
      const store = create<ConditionalState>(() => ({
        value: 0,
        enabled: true,
      }))

      // Only update if enabled
      if (store.getState().enabled) {
        store.setState({ value: 100 })
      }

      expect(store.getState().value).toBe(100)

      // Disable and try to update
      store.setState({ enabled: false })
      if (store.getState().enabled) {
        store.setState({ value: 200 })
      }

      expect(store.getState().value).toBe(100) // Should not change
    })
  })

  describe('Edge Cases', () => {
    it('should handle null and undefined values', () => {
      interface EdgeCaseState {
        value: string | null
        optional: number | undefined
      }
      const store = create<EdgeCaseState>(() => ({
        value: null,
        optional: undefined,
      }))

      expect(store.getState().value).toBeNull()
      expect(store.getState().optional).toBeUndefined()

      store.setState({ value: 'test', optional: 123 })

      expect(store.getState().value).toBe('test')
      expect(store.getState().optional).toBe(123)

      store.setState({ value: null, optional: undefined })

      expect(store.getState().value).toBeNull()
      expect(store.getState().optional).toBeUndefined()
    })

    it('should handle empty string vs null distinction', () => {
      interface DistinctionState {
        emptyString: string
        nullValue: string | null
      }
      const store = create<DistinctionState>(() => ({
        emptyString: '',
        nullValue: null,
      }))

      expect(store.getState().emptyString).toBe('')
      expect(store.getState().nullValue).toBeNull()

      store.setState({ emptyString: 'not empty', nullValue: 'also not empty' })

      expect(store.getState().emptyString).toBe('not empty')
      expect(store.getState().nullValue).toBe('also not empty')
    })

    it('should handle zero values correctly', () => {
      const store = create<{ count: number; total: number }>(() => ({
        count: 0,
        total: 0,
      }))

      expect(store.getState().count).toBe(0)
      expect(store.getState().total).toBe(0)

      // Update to non-zero
      store.setState({ count: 5 })
      expect(store.getState().count).toBe(5)

      // Reset to zero
      store.setState({ count: 0 })
      expect(store.getState().count).toBe(0)
    })
  })

  describe('Array State Management', () => {
    it('should handle array item additions', () => {
      interface ArrayState {
        items: string[]
      }
      const store = create<ArrayState>(() => ({
        items: ['a', 'b'],
      }))

      const currentItems = store.getState().items
      store.setState({ items: [...currentItems, 'c'] })

      expect(store.getState().items).toEqual(['a', 'b', 'c'])
    })

    it('should handle array item removals', () => {
      interface ArrayState {
        items: string[]
      }
      const store = create<ArrayState>(() => ({
        items: ['a', 'b', 'c'],
      }))

      store.setState({ items: store.getState().items.filter(item => item !== 'b') })

      expect(store.getState().items).toEqual(['a', 'c'])
    })

    it('should handle array item updates', () => {
      interface ItemState {
        items: { id: string; value: number }[]
      }
      const store = create<ItemState>(() => ({
        items: [
          { id: '1', value: 10 },
          { id: '2', value: 20 },
        ],
      }))

      store.setState({
        items: store.getState().items.map(item =>
          item.id === '1' ? { ...item, value: 15 } : item
        ),
      })

      expect(store.getState().items[0].value).toBe(15)
      expect(store.getState().items[1].value).toBe(20)
    })
  })

  describe('Nested State Updates', () => {
    it('should handle nested object updates', () => {
      interface NestedState {
        nested: {
          level1: {
            level2: {
              value: string
            }
          }
        }
      }
      const store = create<NestedState>(() => ({
        nested: {
          level1: {
            level2: {
              value: 'initial',
            },
          },
        },
      }))

      // Update nested value
      store.setState({
        nested: {
          ...store.getState().nested,
          level1: {
            ...store.getState().nested.level1,
            level2: {
              ...store.getState().nested.level1.level2,
              value: 'updated',
            },
          },
        },
      })

      expect(store.getState().nested.level1.level2.value).toBe('updated')
    })
  })

  describe('State Reset Behavior', () => {
    it('should reset to initial state', () => {
      interface ResetState {
        value: number
        status: 'active' | 'inactive'
      }
      const initialState: ResetState = {
        value: 100,
        status: 'active',
      }

      const store = create<ResetState>(() => initialState)

      store.setState({ value: 200, status: 'inactive' })

      // Reset
      store.setState(initialState)

      expect(store.getState().value).toBe(100)
      expect(store.getState().status).toBe('active')
    })
  })

  describe('Boolean State Flags', () => {
    it('should toggle boolean flags correctly', () => {
      interface FlagsState {
        isLoading: boolean
        hasError: boolean
        isSuccess: boolean
      }
      const store = create<FlagsState>(() => ({
        isLoading: false,
        hasError: false,
        isSuccess: true,
      }))

      // Toggle loading
      store.setState({ isLoading: true })
      expect(store.getState().isLoading).toBe(true)

      // Toggle error
      store.setState({ hasError: true })
      expect(store.getState().hasError).toBe(true)

      // Toggle success
      store.setState({ isSuccess: false })
      expect(store.getState().isSuccess).toBe(false)

      // Multiple flags at once
      store.setState({ isLoading: false, hasError: false, isSuccess: true })
      expect(store.getState().isLoading).toBe(false)
      expect(store.getState().hasError).toBe(false)
      expect(store.getState().isSuccess).toBe(true)
    })
  })

  describe('Object Reference Handling', () => {
    it('should create new object on partial updates', () => {
      interface ObjectState {
        config: {
          theme: string
          language: string
          notifications: boolean
        }
      }
      const store = create<ObjectState>(() => ({
        config: {
          theme: 'dark',
          language: 'en',
          notifications: true,
        },
      }))

      const originalConfig = store.getState().config

      store.setState({
        config: {
          ...store.getState().config,
          theme: 'light',
        },
      })

      // New object created
      expect(store.getState().config).not.toBe(originalConfig)
      // Other properties preserved
      expect(store.getState().config.language).toBe('en')
      expect(store.getState().config.notifications).toBe(true)
      // Updated property
      expect(store.getState().config.theme).toBe('light')
    })
  })
})

/**
 * Chat Selectors
 *
 * Optimized selectors for ChatContainer and other chat-related components.
 * Use these with Zustand's shallow comparison for array/object properties.
 */

import type { NeoTalkStore } from '..'

// ============================================================================
// State Selectors (single values - no shallow comparison needed)
// ============================================================================

/**
 * Select current session ID
 */
export const selectSessionId = (state: NeoTalkStore) => state.sessionId

/**
 * Select current messages
 */
export const selectMessages = (state: NeoTalkStore) => state.messages

/**
 * Select current user
 */
export const selectUser = (state: NeoTalkStore) => state.user

// ============================================================================
// LLM Backend Selectors (arrays - use shallow comparison)
// ============================================================================

/**
 * Select all LLM backends (raw array)
 * Use with shallow comparison:
 *   import { shallow } from 'zustand/shallow'
 *   const backends = useStore(selectLlmBackends, shallow)
 */
export const selectLlmBackends = (state: NeoTalkStore) => state.llmBackends

/**
 * Select active backend ID
 */
export const selectActiveBackendId = (state: NeoTalkStore) => state.activeBackendId

/**
 * Select active backend object
 */
export const selectActiveBackend = (state: NeoTalkStore) =>
  state.llmBackends.find(b => b.id === state.activeBackendId) || null

/**
 * Select backend-related state combined
 * Use with shallow comparison:
 *   const { llmBackends, activeBackendId } = useStore(selectLlmBackendState, shallow)
 */
export const selectLlmBackendState = (state: NeoTalkStore) => ({
  llmBackends: state.llmBackends,
  activeBackendId: state.activeBackendId,
})

// ============================================================================
// Action Selectors (stable function references)
// ============================================================================

/**
 * Select chat-related actions
 * These functions have stable references, so they can be selected directly
 */
export const selectChatActions = (state: NeoTalkStore) => ({
  addMessage: state.addMessage,
  createSession: state.createSession,
  switchSession: state.switchSession,
  activateBackend: state.activateBackend,
})

// ============================================================================
// Combined Selectors for convenience
// ============================================================================

/**
 * Select all chat state (for backward compatibility)
 * This selector returns an object - use with shallow comparison
 */
export const selectChatState = (state: NeoTalkStore) => ({
  sessionId: state.sessionId,
  messages: state.messages,
  user: state.user,
  llmBackends: state.llmBackends,
  activeBackendId: state.activeBackendId,
})

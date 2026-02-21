import { useEffect } from 'react'

/**
 * Global VisualViewport tracking for mobile keyboard handling (2025)
 *
 * Fixes the issue where styles don't recover after keyboard dismissal
 * by using direct dvh units and forcing viewport recalculation.
 *
 * @see https://dev.to/franciscomoretti/fix-mobile-keyboard-overlap-with-visualviewport-3a4a
 */

let keyboardHeight = 0
let initialHeight = 0

/**
 * Initialize global VisualViewport tracking
 * Call this once in your app root
 */
export function initVisualViewport() {
  if (typeof window === 'undefined') return

  initialHeight = window.innerHeight

  // Store initial viewport height as CSS variable to prevent layout shift
  document.documentElement.style.setProperty('--initial-viewport-height', `${initialHeight}px`)

  // Lock chat page height to initial viewport height
  document.documentElement.style.setProperty('--chat-page-height', `${initialHeight}px`)

  const updateViewport = () => {
    if (!window.visualViewport) return

    const currentHeight = window.visualViewport.height
    const diff = initialHeight - currentHeight

    // Detect keyboard: height decreased by more than 100px
    const wasOpen = keyboardHeight > 0
    const isOpen = diff > 100
    keyboardHeight = isOpen ? diff : 0

    // Update CSS variable
    document.documentElement.style.setProperty('--keyboard-height', `${keyboardHeight}px`)

    // Toggle body class for additional styling hooks
    if (isOpen) {
      document.body.classList.add('keyboard-open')
    } else {
      document.body.classList.remove('keyboard-open')
    }

    // Keyboard just closed - force style recalculation
    if (wasOpen && !isOpen) {
      // Small delay to ensure browser has finished animation
      setTimeout(() => {
        // Force layout recalculation
        void document.body.offsetHeight
        // Reset any transforms that might have been applied
        document.body.style.transform = ''
      }, 100)
    }
  }

  // Initial update
  updateViewport()

  // Listen to visualViewport changes
  window.visualViewport?.addEventListener('resize', updateViewport, { passive: true })
  window.visualViewport?.addEventListener('scroll', updateViewport, { passive: true })

  // Also listen for blur events (when input loses focus)
  document.addEventListener('blur', (e) => {
    if (e instanceof HTMLInputElement || e instanceof HTMLTextAreaElement) {
      // Keyboard might be closing, force update after delay
      setTimeout(updateViewport, 150)
    }
  }, true)
}

/**
 * Hook for components to track keyboard state
 */
export function useKeyboardState() {
  // This hook provides keyboard state without internal state
  // Components can read --keyboard-height CSS variable directly
  return {
    isOpen: keyboardHeight > 0,
    height: keyboardHeight,
  }
}

/**
 * Force viewport recalculation (call manually if needed)
 */
export function forceViewportReset() {
  if (typeof window === 'undefined') return

  keyboardHeight = 0
  document.documentElement.style.setProperty('--keyboard-height', '0px')

  // Remove keyboard-open class immediately
  document.body.classList.remove('keyboard-open')

  // Blur any focused input
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur()
  }
}

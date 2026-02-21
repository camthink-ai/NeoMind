/**
 * Mobile Detection and Touch Utilities
 *
 * Provides utilities for detecting mobile devices and handling touch interactions.
 * Follows 2025 best practices for mobile web applications.
 */

import { useEffect, useState, useCallback, useRef } from 'react'

/**
 * Check if the current device supports touch
 */
export function isTouchDevice(): boolean {
  if (typeof window === 'undefined') return false
  return (
    'ontouchstart' in window ||
    navigator.maxTouchPoints > 0 ||
    // @ts-expect-error - msMaxTouchPoints is IE/Edge specific
    navigator.msMaxTouchPoints > 0
  )
}

/**
 * Check if the device is a mobile device (not tablet)
 * Based on screen size and touch capability
 */
export function isMobileDevice(): boolean {
  if (typeof window === 'undefined') return false
  return isTouchDevice() && window.innerWidth < 768
}

/**
 * Check if the device is a tablet
 */
export function isTabletDevice(): boolean {
  if (typeof window === 'undefined') return false
  return isTouchDevice() && window.innerWidth >= 768 && window.innerWidth < 1024
}

/**
 * Get the device type for responsive behavior
 */
export type DeviceType = 'mobile' | 'tablet' | 'desktop'

export function getDeviceType(): DeviceType {
  if (!isTouchDevice()) return 'desktop'
  if (window.innerWidth < 768) return 'mobile'
  return 'tablet'
}

/**
 * Hook to detect if the current device is touch-enabled
 * Updates on resize to handle device orientation changes
 */
export function useIsTouchDevice(): boolean {
  const [isTouch, setIsTouch] = useState(false)

  useEffect(() => {
    setIsTouch(isTouchDevice())

    const handleResize = () => {
      setIsTouch(isTouchDevice())
    }

    window.addEventListener('resize', handleResize)
    return () => window.removeEventListener('resize', handleResize)
  }, [])

  return isTouch
}

/**
 * Hook to detect the current device type
 */
export function useDeviceType(): DeviceType {
  const [deviceType, setDeviceType] = useState<DeviceType>('desktop')

  useEffect(() => {
    const updateDeviceType = () => {
      setDeviceType(getDeviceType())
    }

    updateDeviceType()
    window.addEventListener('resize', updateDeviceType)
    return () => window.removeEventListener('resize', updateDeviceType)
  }, [])

  return deviceType
}

/**
 * Hook to detect if we're in mobile view
 */
export function useIsMobile(): boolean {
  return useDeviceType() === 'mobile'
}

/**
 * Hook for touch-based hover state
 * Provides a touch-friendly alternative to mouse hover
 */
export interface UseTouchHoverOptions {
  /** Delay before showing hover state on touch (ms) */
  showDelay?: number
  /** Delay before hiding hover state (ms) */
  hideDelay?: number
  /** Whether to enable hover state */
  enabled?: boolean
}

export function useTouchHover(options: UseTouchHoverOptions = {}): {
  isHovered: boolean
  hoverProps: {
    onMouseEnter: () => void
    onMouseLeave: () => void
    onTouchStart: () => void
    onTouchEnd: () => void
  }
  forceShow: () => void
  forceHide: () => void
} {
  const { showDelay = 150, hideDelay = 300, enabled = true } = options

  const [isHovered, setIsHovered] = useState(false)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isTouchRef = useRef(false)

  const clearTimer = useCallback(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
  }, [])

  const show = useCallback(() => {
    if (!enabled) return
    clearTimer()
    timeoutRef.current = setTimeout(() => {
      setIsHovered(true)
    }, showDelay)
  }, [showDelay, enabled, clearTimer])

  const hide = useCallback(() => {
    if (!enabled) return
    clearTimer()
    timeoutRef.current = setTimeout(() => {
      setIsHovered(false)
    }, hideDelay)
  }, [hideDelay, enabled, clearTimer])

  const forceShow = useCallback(() => {
    clearTimer()
    setIsHovered(true)
  }, [clearTimer])

  const forceHide = useCallback(() => {
    clearTimer()
    setIsHovered(false)
  }, [clearTimer])

  // Mouse events
  const handleMouseEnter = useCallback(() => {
    if (!isTouchRef.current) {
      show()
    }
  }, [show])

  const handleMouseLeave = useCallback(() => {
    if (!isTouchRef.current) {
      hide()
    }
  }, [hide])

  // Touch events - toggle on tap
  const handleTouchStart = useCallback(() => {
    isTouchRef.current = true
    clearTimer()
  }, [clearTimer])

  const handleTouchEnd = useCallback(() => {
    clearTimer()
    // Toggle hover state on tap
    setIsHovered(prev => !prev)
  }, [clearTimer])

  // Cleanup on unmount
  useEffect(() => {
    return () => clearTimer()
  }, [clearTimer])

  return {
    isHovered,
    hoverProps: {
      onMouseEnter: handleMouseEnter,
      onMouseLeave: handleMouseLeave,
      onTouchStart: handleTouchStart,
      onTouchEnd: handleTouchEnd,
    },
    forceShow,
    forceHide,
  }
}

/**
 * Hook for long-press gesture detection
 * Useful for context menus on mobile
 */
export interface UseLongPressOptions {
  /** Delay before triggering long press (ms) */
  delay?: number
  /** Callback when long press is triggered */
  onLongPress: (event: React.TouchEvent | React.MouseEvent) => void
  /** Callback when touch starts */
  onTouchStart?: (event: React.TouchEvent) => void
  /** Callback when touch ends */
  onTouchEnd?: (event: React.TouchEvent) => void
  /** Callback when touch moves (cancels long press) */
  onTouchMove?: (event: React.TouchEvent) => void
}

export function useLongPress(options: UseLongPressOptions) {
  const { delay = 500, onLongPress, onTouchStart, onTouchEnd, onTouchMove } = options
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const startPosRef = useRef<{ x: number; y: number } | null>(null)

  const start = useCallback(
    (event: React.TouchEvent | React.MouseEvent) => {
      // Record start position for move detection
      if ('touches' in event && event.touches.length > 0) {
        startPosRef.current = {
          x: event.touches[0].clientX,
          y: event.touches[0].clientY,
        }
      }

      onTouchStart?.(event as React.TouchEvent)

      timeoutRef.current = setTimeout(() => {
        onLongPress(event)
      }, delay)
    },
    [delay, onLongPress, onTouchStart]
  )

  const clear = useCallback(
    (event: React.TouchEvent | React.MouseEvent) => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
      startPosRef.current = null
      onTouchEnd?.(event as React.TouchEvent)
    },
    [onTouchEnd]
  )

  const move = useCallback(
    (event: React.TouchEvent) => {
      // Cancel long press if moved too much
      if (startPosRef.current && event.touches.length > 0) {
        const moveX = Math.abs(event.touches[0].clientX - startPosRef.current.x)
        const moveY = Math.abs(event.touches[0].clientY - startPosRef.current.y)
        const threshold = 10 // 10px movement threshold

        if (moveX > threshold || moveY > threshold) {
          clear(event)
          return
        }
      }
      onTouchMove?.(event)
    },
    [onTouchMove, clear]
  )

  return {
    onMouseDown: start,
    onMouseUp: clear,
    onMouseLeave: clear,
    onTouchStart: start,
    onTouchEnd: clear,
    onTouchMove: move,
  }
}

/**
 * Safe area insets for notched devices
 */
export function useSafeAreaInsets() {
  const [insets, setInsets] = useState({
    top: 0,
    right: 0,
    bottom: 0,
    left: 0,
  })

  useEffect(() => {
    if (typeof window === 'undefined' || !CSS.supports('padding', 'env(safe-area-inset-top)')) {
      return
    }

    // Create temporary elements to measure safe areas
    const topDiv = document.createElement('div')
    const rightDiv = document.createElement('div')
    const bottomDiv = document.createElement('div')
    const leftDiv = document.createElement('div')

    topDiv.style.paddingTop = 'env(safe-area-inset-top)'
    rightDiv.style.paddingRight = 'env(safe-area-inset-right)'
    bottomDiv.style.paddingBottom = 'env(safe-area-inset-bottom)'
    leftDiv.style.paddingLeft = 'env(safe-area-inset-left)'

    document.body.appendChild(topDiv)
    document.body.appendChild(rightDiv)
    document.body.appendChild(bottomDiv)
    document.body.appendChild(leftDiv)

    const measureInsets = () => {
      setInsets({
        top: parseInt(getComputedStyle(topDiv).paddingTop) || 0,
        right: parseInt(getComputedStyle(rightDiv).paddingRight) || 0,
        bottom: parseInt(getComputedStyle(bottomDiv).paddingBottom) || 0,
        left: parseInt(getComputedStyle(leftDiv).paddingLeft) || 0,
      })
    }

    // Measure after a slight delay for layout
    const timeoutId = setTimeout(measureInsets, 100)

    // Listen for orientation changes
    window.addEventListener('resize', measureInsets)

    return () => {
      clearTimeout(timeoutId)
      window.removeEventListener('resize', measureInsets)
      topDiv.remove()
      rightDiv.remove()
      bottomDiv.remove()
      leftDiv.remove()
    }
  }, [])

  return insets
}

/**
 * useInterval - Safe interval hook with automatic cleanup
 *
 * Performance optimization: Prevents timer leaks by ensuring cleanup
 * on unmount. Provides better API than raw setInterval/clearInterval.
 *
 * @example
 * ```tsx
 * useInterval(() => {
 *   fetchData()
 * }, 5000) // Poll every 5 seconds
 * ```
 */

import { useEffect, useRef } from 'react'

interface UseIntervalOptions {
  /** Start immediately (default: true) */
  immediate?: boolean
  /** Run on mount before first interval (default: false) */
  runOnMount?: boolean
}

/**
 * Set up an interval that automatically cleans up on unmount.
 * This is safer than using setInterval directly as it prevents memory leaks.
 */
export function useInterval(
  callback: () => void,
  delay: number | null,
  options: UseIntervalOptions = {}
): void {
  const { immediate = true, runOnMount = false } = options
  const savedCallback = useRef(callback)
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  // Remember the latest callback
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  // Set up interval
  useEffect(() => {
    if (delay === null || !immediate) {
      return
    }

    // Run on mount if requested
    if (runOnMount) {
      savedCallback.current()
    }

    // Set up the interval
    intervalRef.current = setInterval(() => {
      savedCallback.current()
    }, delay)

    // Cleanup function - clear interval on unmount or delay change
    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
        intervalRef.current = null
      }
    }
  }, [delay, immediate, runOnMount])
}

/**
 * Set up a timeout that automatically cleans up on unmount.
 * Returns a function to manually clear the timeout if needed.
 */
export function useTimeout(
  callback: () => void,
  delay: number | null
): () => void {
  const savedCallback = useRef(callback)
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Remember the latest callback
  useEffect(() => {
    savedCallback.current = callback
  }, [callback])

  // Set up timeout
  useEffect(() => {
    if (delay === null) {
      return
    }

    timeoutRef.current = setTimeout(() => {
      savedCallback.current()
    }, delay)

    // Cleanup function
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
        timeoutRef.current = null
      }
    }
  }, [delay])

  // Return manual cleanup function
  return () => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
  }
}

/**
 * Track multiple timers and clean them all up at once.
 * Useful for components with multiple intervals/timeouts.
 *
 * @example
 * ```tsx
 * const timers = useTimers()
 *
 * useEffect(() => {
 *   timers.setInterval(() => fetchStatus(), 5000, 'status')
 *   timers.setTimeout(() => showToast(), 3000, 'toast')
 *   return () => timers.clearAll()
 * }, [])
 * ```
 */
export function useTimers() {
  const intervalsRef = useRef<Map<string, ReturnType<typeof setInterval>>>(new Map())
  const timeoutsRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map())

  const setIntervalById = (
    callback: () => void,
    delay: number,
    id: string
  ) => {
    // Clear existing interval with same ID
    if (intervalsRef.current.has(id)) {
      clearInterval(intervalsRef.current.get(id)!)
    }

    const intervalId = setInterval(callback, delay)
    intervalsRef.current.set(id, intervalId)
    return intervalId
  }

  const setTimeoutById = (
    callback: () => void,
    delay: number,
    id: string
  ) => {
    // Clear existing timeout with same ID
    if (timeoutsRef.current.has(id)) {
      clearTimeout(timeoutsRef.current.get(id)!)
    }

    const timeoutId = setTimeout(callback, delay)
    timeoutsRef.current.set(id, timeoutId)
    return timeoutId
  }

  const clearIntervalById = (id: string) => {
    const intervalId = intervalsRef.current.get(id)
    if (intervalId) {
      clearInterval(intervalId)
      intervalsRef.current.delete(id)
    }
  }

  const clearTimeoutById = (id: string) => {
    const timeoutId = timeoutsRef.current.get(id)
    if (timeoutId) {
      clearTimeout(timeoutId)
      timeoutsRef.current.delete(id)
    }
  }

  const clearAll = () => {
    // Clear all intervals
    intervalsRef.current.forEach((intervalId) => clearInterval(intervalId))
    intervalsRef.current.clear()

    // Clear all timeouts
    timeoutsRef.current.forEach((timeoutId) => clearTimeout(timeoutId))
    timeoutsRef.current.clear()
  }

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      clearAll()
    }
  }, [])

  return {
    setInterval: setIntervalById,
    setTimeout: setTimeoutById,
    clearInterval: clearIntervalById,
    clearTimeout: clearTimeoutById,
    clearAll,
  }
}

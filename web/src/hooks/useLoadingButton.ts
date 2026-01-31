/**
 * useLoadingButton - Hook to prevent double-clicks on buttons with async actions
 *
 * Automatically disables button during async operation and re-enables after completion.
 *
 * @example
 * ```tsx
 * const { handleClick, isLoading } = useLoadingButton(async () => {
 *   await saveData()
 * })
 *
 * <Button onClick={handleClick} disabled={isLoading}>
 *   {isLoading ? <Loader2 className="animate-spin" /> : 'Save'}
 * </Button>
 * ```
 */

import { useState, useCallback, useRef } from 'react'

export interface UseLoadingButtonOptions {
  /** Whether to reset error state on new click (default: true) */
  resetErrorOnClick?: boolean
}

export interface UseLoadingButtonReturn {
  /** Click handler that manages loading state */
  handleClick: () => void
  /** Whether the action is currently in progress */
  isLoading: boolean
  /** Error from the last action (if any) */
  error: Error | null
  /** Clear the error state */
  clearError: () => void
}

export function useLoadingButton(
  action: () => Promise<void>,
  options: UseLoadingButtonOptions = {}
): UseLoadingButtonReturn {
  const { resetErrorOnClick = true } = options

  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<Error | null>(null)
  const isPendingRef = useRef(false)

  const handleClick = useCallback(() => {
    // Prevent double-clicks
    if (isPendingRef.current) return

    isPendingRef.current = true
    setIsLoading(true)

    if (resetErrorOnClick) {
      setError(null)
    }

    action()
      .catch((err) => {
        setError(err instanceof Error ? err : new Error(String(err)))
      })
      .finally(() => {
        isPendingRef.current = false
        setIsLoading(false)
      })
  }, [action, resetErrorOnClick])

  const clearError = useCallback(() => {
    setError(null)
  }, [])

  return {
    handleClick,
    isLoading,
    error,
    clearError,
  }
}

/**
 * Simpler version that just returns a wrapped click handler
 *
 * @example
 * ```tsx
 * const handleClick = useDebouncedCallback(async () => {
 *   await saveData()
 * })
 *
 * <Button onClick={handleClick}>Save</Button>
 * ```
 */
export function useDebouncedCallback<T extends (...args: any[]) => any>(
  callback: T,
  delay: number = 300
): T {
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isPendingRef = useRef(false)

  return useCallback(
    (...args: Parameters<T>) => {
      // Prevent duplicate calls
      if (isPendingRef.current) return

      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current)
      }

      isPendingRef.current = true

      timeoutRef.current = setTimeout(() => {
        callback(...args).finally(() => {
          isPendingRef.current = false
          timeoutRef.current = null
        })
      }, delay)
    },
    [callback, delay]
  ) as T
}

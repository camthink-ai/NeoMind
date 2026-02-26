/**
 * Infinite Scroll Hook
 *
 * Provides intersection observer-based infinite scrolling for mobile devices.
 * Follows the pattern used in MergedMessageList for consistency.
 */

import { useEffect, useRef, useCallback, useState } from 'react'

export interface UseInfiniteScrollOptions {
  /** Distance in pixels before bottom to trigger load (default: 200px) */
  threshold?: number
  /** Whether loading is in progress */
  isLoading?: boolean
  /** Whether there are more items to load */
  hasMore?: boolean
  /** Callback to load more items */
  onLoadMore: () => void
  /** Root element for intersection observer (default: null = viewport) */
  root?: Element | null
  /** Enable/disable the observer */
  enabled?: boolean
}

/**
 * Hook for infinite scroll functionality using Intersection Observer
 *
 * @example
 * const { loadMoreRef, showLoadingIndicator } = useInfiniteScroll({
 *   threshold: 200,
 *   isLoading: loading,
 *   hasMore: hasMoreItems,
 *   onLoadMore: () => loadNextPage(),
 *   enabled: isMobile
 * })
 */
export function useInfiniteScroll({
  threshold = 200,
  isLoading = false,
  hasMore = true,
  onLoadMore,
  root = null,
  enabled = true,
}: UseInfiniteScrollOptions) {
  const loadMoreRef = useRef<HTMLDivElement>(null)
  const [hasIntersected, setHasIntersected] = useState(false)

  // Stable callback for load more
  const handleLoadMore = useCallback(() => {
    if (!isLoading && hasMore) {
      onLoadMore()
    }
  }, [isLoading, hasMore, onLoadMore])

  // Set up intersection observer
  useEffect(() => {
    if (!enabled || !loadMoreRef.current || !hasMore) {
      return
    }

    const trigger = loadMoreRef.current
    setHasIntersected(false)

    const observer = new IntersectionObserver(
      (entries) => {
        const [entry] = entries
        if (entry.isIntersecting && !hasIntersected) {
          setHasIntersected(true)
          handleLoadMore()
        }
      },
      {
        root,
        rootMargin: `${threshold}px`,
        threshold: 0.01,
      }
    )

    observer.observe(trigger)

    return () => {
      observer.disconnect()
    }
  }, [enabled, hasMore, threshold, root, handleLoadMore, hasIntersected])

  // Reset intersection state when loading completes
  useEffect(() => {
    if (!isLoading && hasIntersected) {
      setHasIntersected(false)
    }
  }, [isLoading, hasIntersected])

  return {
    /** Ref to attach to the trigger element at the bottom of the list */
    loadMoreRef,
    /** Whether to show a loading indicator */
    showLoadingIndicator: isLoading && hasMore,
  }
}

/**
 * Hook variant that works with scroll event instead of Intersection Observer
 * Useful when you need scroll-based detection within a specific container
 */
export interface UseScrollLoadOptions extends Omit<UseInfiniteScrollOptions, 'root'> {
  /** Container element to watch for scroll events */
  containerRef: React.RefObject<HTMLElement>
  /** Offset from bottom in pixels to trigger load (default: 100px) */
  offset?: number
}

export function useScrollLoad({
  containerRef,
  offset = 100,
  isLoading = false,
  hasMore = true,
  onLoadMore,
  enabled = true,
}: UseScrollLoadOptions) {
  const [showLoadingIndicator, setShowLoadingIndicator] = useState(false)

  useEffect(() => {
    if (!enabled || !containerRef.current) {
      return
    }

    const container = containerRef.current
    let timeoutId: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      if (!hasMore || isLoading) {
        return
      }

      const { scrollTop, scrollHeight, clientHeight } = container
      const distanceFromBottom = scrollHeight - scrollTop - clientHeight

      // Debounce the load trigger
      if (distanceFromBottom <= offset) {
        if (timeoutId) {
          clearTimeout(timeoutId)
        }
        timeoutId = setTimeout(() => {
          if (hasMore && !isLoading) {
            setShowLoadingIndicator(true)
            onLoadMore()
          }
        }, 100)
      }
    }

    container.addEventListener('scroll', handleScroll, { passive: true })

    return () => {
      container.removeEventListener('scroll', handleScroll)
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
    }
  }, [enabled, hasMore, isLoading, offset, onLoadMore, containerRef])

  // Hide loading indicator when loading completes
  useEffect(() => {
    if (!isLoading) {
      setShowLoadingIndicator(false)
    }
  }, [isLoading])

  return {
    showLoadingIndicator,
  }
}

/**
 * Hook that monitors window/document scroll for infinite scroll
 * Useful when the scroll container is not directly accessible
 */
export interface UseWindowScrollLoadOptions extends Omit<UseInfiniteScrollOptions, 'root'> {
  /** Offset from bottom in pixels to trigger load (default: 150px) */
  offset?: number
  /** Selector for scrollable container (if not window/document) */
  containerSelector?: string
}

export function useWindowScrollLoad({
  offset = 150,
  isLoading = false,
  hasMore = true,
  onLoadMore,
  enabled = true,
  containerSelector,
}: UseWindowScrollLoadOptions) {
  const [showLoadingIndicator, setShowLoadingIndicator] = useState(false)

  useEffect(() => {
    if (!enabled) {
      return
    }

    let timeoutId: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      if (!hasMore || isLoading) {
        return
      }

      // Find scroll container if selector provided
      let container: HTMLElement | Window | null = null
      if (containerSelector) {
        const el = document.querySelector(containerSelector)
        container = el as HTMLElement | null
      } else {
        container = window
      }

      // Get scroll dimensions
      let scrollTop = 0
      let scrollHeight = 0
      let clientHeight = 0

      if (container && 'scrollTop' in container) {
        const el = container as HTMLElement
        scrollTop = el.scrollTop
        scrollHeight = el.scrollHeight
        clientHeight = el.clientHeight
      } else {
        scrollTop = window.pageYOffset || document.documentElement.scrollTop
        scrollHeight = Math.max(
          document.documentElement.scrollHeight,
          document.body.scrollHeight
        )
        clientHeight = window.innerHeight
      }

      const distanceFromBottom = scrollHeight - scrollTop - clientHeight

      // Trigger load when near bottom
      if (distanceFromBottom <= offset) {
        if (timeoutId) {
          clearTimeout(timeoutId)
        }
        timeoutId = setTimeout(() => {
          if (hasMore && !isLoading) {
            setShowLoadingIndicator(true)
            onLoadMore()
          }
        }, 100)
      }
    }

    // Add scroll listener to window (or container if specified)
    const target = containerSelector
      ? (document.querySelector(containerSelector) as HTMLElement) || window
      : window

    target.addEventListener('scroll', handleScroll, { passive: true })

    // Also check on mount in case already scrolled
    handleScroll()

    return () => {
      target.removeEventListener('scroll', handleScroll)
      if (timeoutId) {
        clearTimeout(timeoutId)
      }
    }
  }, [enabled, hasMore, isLoading, offset, onLoadMore, containerSelector])

  // Hide loading indicator when loading completes
  useEffect(() => {
    if (!isLoading) {
      setShowLoadingIndicator(false)
    }
  }, [isLoading])

  return {
    showLoadingIndicator,
  }
}

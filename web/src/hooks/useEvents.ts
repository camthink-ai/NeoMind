// useEvents Hook for React Components
//
// Provides real-time event streaming to React components via custom hooks.

import { useEffect, useState, useCallback, useRef } from 'react'
import type { NeoTalkEvent, EventType, EventCategory } from '@/lib/events'
import { fetchAPI } from '@/lib/api'

export interface UseEventsOptions {
  /**
   * Filter by event category
   */
  category?: EventCategory

  /**
   * Filter by specific event types
   */
  eventTypes?: EventType[]

  /**
   * Use Server-Sent Events instead of WebSocket
   */
  useSSE?: boolean

  /**
   * Whether to connect automatically
   */
  enabled?: boolean

  /**
   * Event handler callback
   */
  onEvent?: (event: NeoTalkEvent) => void

  /**
   * Connection state change callback
   */
  onConnected?: (connected: boolean) => void

  /**
   * Error callback
   */
  onError?: (error: Error) => void
}

export interface UseEventsResult {
  /**
   * Whether the connection is active
   */
  isConnected: boolean

  /**
   * Recent events received
   */
  events: NeoTalkEvent[]

  /**
   * Clear the events buffer
   */
  clearEvents: () => void

  /**
   * Manually reconnect
   */
  reconnect: () => void

  /**
   * Get events by type
   */
  getEventsByType: <T extends EventType>(type: T) => Extract<NeoTalkEvent, { type: T }>[]
}

const DEFAULT_MAX_EVENTS = 100

/**
 * useEvents - Hook for subscribing to real-time NeoTalk events
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const { isConnected, events } = useEvents({
 *     category: 'device',
 *     onEvent: (event) => console.log('Event:', event)
 *   })
 *
 *   return (
 *     <div>
 *       <p>Connected: {isConnected}</p>
 *       <ul>
 *         {events.map(e => (
 *           <li key={e.id}>{e.type}: {JSON.stringify(e.data)}</li>
 *         ))}
 *       </ul>
 *     </div>
 *   )
 * }
 * ```
 */
export function useEvents(options: UseEventsOptions = {}): UseEventsResult {
  const {
    category = 'all',
    eventTypes,
    useSSE = false,
    enabled = true,
    onEvent,
    onConnected,
    onError,
  } = options

  const [isConnected, setIsConnected] = useState(false)
  const [events, setEvents] = useState<NeoTalkEvent[]>([])
  const connectionRef = useRef<ReturnType<typeof import('@/lib/events').getEventsConnection> | null>(null)
  const maxEvents = DEFAULT_MAX_EVENTS

  // Use refs to store latest callbacks without causing re-renders
  const onEventRef = useRef(onEvent)
  const onConnectedRef = useRef(onConnected)
  const onErrorRef = useRef(onError)

  // Update refs when callbacks change
  useEffect(() => {
    onEventRef.current = onEvent
  }, [onEvent])

  useEffect(() => {
    onConnectedRef.current = onConnected
  }, [onConnected])

  useEffect(() => {
    onErrorRef.current = onError
  }, [onError])

  const clearEvents = useCallback(() => {
    setEvents([])
  }, [])

  const reconnect = useCallback(() => {
    if (connectionRef.current) {
      connectionRef.current.disconnect()
      connectionRef.current.connect()
    }
  }, [])

  // Get events by type from the buffer
  const getEventsByType = useCallback(<T extends EventType>(type: T) => {
    return events.filter(e => e.type === type) as Extract<NeoTalkEvent, { type: T }>[]
  }, [events])

  useEffect(() => {
    if (!enabled) {
      return
    }

    let unsubscribeConnection: (() => void) | undefined
    let unsubscribeError: (() => void) | undefined
    let unsubscribeEvent: (() => void) | undefined
    let isMounted = true

    // Dynamically import to avoid SSR issues
    import('@/lib/events').then((eventsLib) => {
      if (!isMounted) return

      const getEventsConnection = eventsLib.getEventsConnection || (eventsLib as any).default?.getEventsConnection
      if (!getEventsConnection) {
        console.warn('getEventsConnection not found in events lib')
        return
      }

      // Create or get existing connection
      const connection = getEventsConnection(
        `events-${category}-${eventTypes?.join('-') || 'all'}`,
        { category, eventTypes, useSSE }
      )
      connectionRef.current = connection

      // Subscribe to connection state
      unsubscribeConnection = connection.onConnection((connected: boolean) => {
        if (isMounted) {
          setIsConnected(connected)
          onConnectedRef.current?.(connected)
        }
      })

      // Subscribe to errors
      unsubscribeError = connection.onError((error: unknown) => {
        if (isMounted) {
          onErrorRef.current?.(error instanceof Error ? error : new Error(String(error)))
        }
      })

      // Subscribe to events
      unsubscribeEvent = connection.onEvent((event: NeoTalkEvent) => {
        if (isMounted) {
          setEvents(prev => {
            const newEvents = [...prev, event]
            // Keep only the most recent events
            return newEvents.slice(-maxEvents)
          })
          onEventRef.current?.(event)
        }
      })
    }).catch((err) => {
      console.error('Failed to load events lib:', err)
    })

    // Cleanup on unmount
    return () => {
      isMounted = false
      unsubscribeConnection?.()
      unsubscribeError?.()
      unsubscribeEvent?.()
    }
  }, [category, eventTypes, useSSE, enabled, maxEvents])

  return {
    isConnected,
    events,
    clearEvents,
    reconnect,
    getEventsByType,
  }
}

/**
 * useDeviceEvents - Hook specifically for device events
 */
export function useDeviceEvents(options?: Omit<UseEventsOptions, 'category'>) {
  return useEvents({
    ...options,
    category: 'device',
  })
}

/**
 * useRuleEvents - Hook specifically for rule events
 */
export function useRuleEvents(options?: Omit<UseEventsOptions, 'category'>) {
  return useEvents({
    ...options,
    category: 'rule',
  })
}

/**
 * useWorkflowEvents - Hook specifically for workflow events
 */
export function useWorkflowEvents(options?: Omit<UseEventsOptions, 'category'>) {
  return useEvents({
    ...options,
    category: 'workflow',
  })
}

/**
 * useLlmEvents - Hook specifically for LLM/agent events
 */
export function useLlmEvents(options?: Omit<UseEventsOptions, 'category'>) {
  return useEvents({
    ...options,
    category: 'llm',
  })
}

/**
 * useAlertEvents - Hook specifically for alert events
 */
export function useAlertEvents(options?: Omit<UseEventsOptions, 'category'>) {
  return useEvents({
    ...options,
    category: 'alert',
  })
}

/**
 * useEventStats - Hook for event statistics
 */
export function useEventStats(enabled = true) {
  const [stats, setStats] = useState<{
    totalEvents: number
    eventsByType: Record<string, number>
    eventsByCategory: Record<string, number>
    activeSubscriptions: number
  } | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  const fetchStats = useCallback(async () => {
    setLoading(true)
    setError(null)

    try {
      const data = await fetchAPI<{
        totalEvents: number
        eventsByType: Record<string, number>
        eventsByCategory: Record<string, number>
        activeSubscriptions: number
      }>('/events/stats')
      setStats(data)
    } catch (err) {
      setError(err as Error)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (enabled) {
      fetchStats()
      // Refresh stats every 30 seconds
      const interval = setInterval(fetchStats, 30000)
      return () => clearInterval(interval)
    }
  }, [enabled, fetchStats])

  return { stats, loading, error, refetch: fetchStats }
}

/**
 * useEventHistory - Hook for querying historical events
 */
export interface EventHistoryOptions {
  start?: number
  end?: number
  eventTypes?: string[]
  category?: string
  limit?: number
  enabled?: boolean
}

export function useEventHistory(options: EventHistoryOptions = {}) {
  const {
    start,
    end,
    eventTypes,
    category,
    limit = 100,
    enabled = true,
  } = options

  const [events, setEvents] = useState<NeoTalkEvent[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<Error | null>(null)

  const fetchHistory = useCallback(async () => {
    if (!enabled) return

    setLoading(true)
    setError(null)

    try {
      const params = new URLSearchParams()
      if (start) params.set('start', start.toString())
      if (end) params.set('end', end.toString())
      if (category) params.set('category', category)
      params.set('limit', limit.toString())
      eventTypes?.forEach(type => params.append('event_type', type))

      const data = await fetchAPI<{ events: NeoTalkEvent[]; count: number }>(
        `/events/history?${params.toString()}`
      )
      setEvents(data.events || [])
    } catch (err) {
      setError(err as Error)
    } finally {
      setLoading(false)
    }
  }, [start, end, eventTypes, category, limit, enabled])

  // Fetch history when dependencies change
  useEffect(() => {
    fetchHistory()
  }, [start, end, eventTypes, category, limit])

  return { events, loading, error, refetch: fetchHistory }
}

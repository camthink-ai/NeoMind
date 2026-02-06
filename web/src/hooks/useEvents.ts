// useEvents Hook for React Components
//
// Provides real-time event streaming to React components via custom hooks.

import { useEffect, useState, useCallback, useRef } from 'react'
import type { NeoMindEvent, EventType, EventCategory } from '@/lib/events'
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
  onEvent?: (event: NeoMindEvent) => void

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
  events: NeoMindEvent[]

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
  getEventsByType: <T extends EventType>(type: T) => Extract<NeoMindEvent, { type: T }>[]
}

const DEFAULT_MAX_EVENTS = 100

/**
 * useEvents - Hook for subscribing to real-time NeoMind events
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
  const [events, setEvents] = useState<NeoMindEvent[]>([])
  const connectionRef = useRef<ReturnType<typeof import('@/lib/events').getEventsConnection> | null>(null)
  const maxEvents = DEFAULT_MAX_EVENTS

  // Use refs to store latest callbacks without causing re-renders
  const onEventRef = useRef(onEvent)
  const onConnectedRef = useRef(onConnected)
  const onErrorRef = useRef(onError)
  const eventTypesRef = useRef(eventTypes)

  // Update refs when callbacks change
  useEffect(() => {
    onEventRef.current = onEvent
  }, [onEvent])

  useEffect(() => {
    eventTypesRef.current = eventTypes
  }, [eventTypes])

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
    return events.filter(e => e.type === type) as Extract<NeoMindEvent, { type: T }>[]
  }, [events])

  useEffect(() => {
    if (!enabled) {
      return
    }

    let unsubscribeConnection: (() => void) | undefined
    let unsubscribeError: (() => void) | undefined
    let unsubscribeEvent: (() => void) | undefined
    let isMounted = true
    let isSubscribed = false

    // Dynamically import to avoid SSR issues
    import('@/lib/events').then((eventsLib) => {
      if (!isMounted) return

      const getEventsConnection = eventsLib.getEventsConnection || (eventsLib as any).default?.getEventsConnection
      if (!getEventsConnection) {
        console.warn('getEventsConnection not found in events lib')
        return
      }

      // Create or get existing connection
      // Use a single shared connection per category instead of per eventTypes combination
      // This prevents creating multiple WebSocket connections
      const connection = getEventsConnection(
        `events-${category}`,
        { category, useSSE }  // Don't pass eventTypes to backend, filter on client side
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
      unsubscribeEvent = connection.onEvent((event: NeoMindEvent) => {
        if (isMounted) {
          // Filter by eventTypes on client side
          const currentEventTypes = eventTypesRef.current
          if (currentEventTypes && !currentEventTypes.includes(event.type as any)) {
            return
          }
          setEvents(prev => {
            const newEvents = [...prev, event]
            // Keep only the most recent events
            return newEvents.slice(-maxEvents)
          })
          onEventRef.current?.(event)
        }
      })

      isSubscribed = true
    }).catch((err) => {
      console.error('Failed to load events lib:', err)
    })

    // Cleanup on unmount
    return () => {
      isMounted = false
      // Only unsubscribe if we successfully subscribed
      if (isSubscribed) {
        unsubscribeConnection?.()
        unsubscribeError?.()
        unsubscribeEvent?.()
      }
    }
  }, [category, useSSE, enabled, maxEvents])

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

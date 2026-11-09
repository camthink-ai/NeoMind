// Live message refresh: subscribe to backend MessageCreated events and
// coalesce them into one trailing refetch. Extracted from the messages
// page so the subscription + coalescing window is testable against the
// real useEvents wiring without rendering the whole page.
import { useEffect, useRef } from 'react'
import { useEvents } from '@/hooks/useEvents'

/** Coalescing window: a rule storm creates many messages at once, and each
 * refetch is a server-side paginated query — one fetch per burst, not one
 * per event. Bounded staleness: a message created just after a burst fires
 * its own event and refreshes in the next window. */
const COALESCE_MS = 1500

export function useLiveMessageRefresh(enabled: boolean, refetch: () => void): void {
  const timer = useRef<number | null>(null)
  // Latest refetch without resubscribing — useEvents stores the callback in
  // a ref, so the caller's useCallback identity churn (filters/page deps)
  // never reconnects the shared event stream.
  const refetchRef = useRef(refetch)
  useEffect(() => {
    refetchRef.current = refetch
  }, [refetch])

  useEvents({
    enabled,
    onEvent: (event) => {
      if (event.type !== 'MessageCreated') return
      if (timer.current !== null) return
      timer.current = window.setTimeout(() => {
        timer.current = null
        refetchRef.current()
      }, COALESCE_MS)
    },
  })

  // A pending refetch is useless once the page is left or the tab switched
  // away — cancel it instead of fetching data nobody sees.
  useEffect(() => {
    if (enabled) return
    if (timer.current !== null) {
      clearTimeout(timer.current)
      timer.current = null
    }
  }, [enabled])
  useEffect(() => {
    return () => {
      if (timer.current !== null) {
        clearTimeout(timer.current)
        timer.current = null
      }
    }
  }, [])
}

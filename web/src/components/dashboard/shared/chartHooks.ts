// Chart sizing/stagger/memoization hooks — split from ChartContainer.tsx
// (react-refresh requires component files to export only components).
import { type ReactNode, memo, useEffect, useRef, useState } from 'react'

// Global stagger counter — each chart gets a unique turn
// When all charts unmount, the counter resets to prevent unbounded growth
let chartTurn = 0
const activeTurns = new Set<number>()

// Max stagger delay cap — if turn exceeds this, recycle to keep delays reasonable
const MAX_STAGGER_TURN = 20

export function useChartDimensions() {
  const ref = useRef<HTMLDivElement>(null)
  const [dims, setDims] = useState({ width: 0, height: 0 })

  // Each instance claims a turn on first render, capped to prevent unbounded growth
  const turnRef = useRef(-1)
  if (turnRef.current === -1) {
    // Cap the turn number to prevent ever-increasing stagger delays
    const turn = chartTurn % (MAX_STAGGER_TURN + 1)
    chartTurn = turn + 1
    turnRef.current = turn
    activeTurns.add(turn)
  }

  useEffect(() => {
    const el = ref.current
    if (!el) return

    const measure = () => {
      const w = el.clientWidth
      const h = el.clientHeight
      if (w > 0 && h > 0) {
        setDims(prev => {
          if (prev.width === w && prev.height === h) return prev
          return { width: w, height: h }
        })
      }
    }

    // Stagger initial measurement: wait for (turn + 1) RAFs
    // This prevents all charts from rendering SVG in the same frame
    let rafId: number | null = null
    let remaining = turnRef.current + 1

    const scheduleMeasure = () => {
      if (remaining > 0) {
        remaining--
        rafId = requestAnimationFrame(scheduleMeasure)
      } else {
        measure()
        initialDone = true
      }
    }
    rafId = requestAnimationFrame(scheduleMeasure)

    // After initial measurement, use ResizeObserver for size changes
    let initialDone = false
    const ro = new ResizeObserver(() => {
      if (!initialDone) return // Skip RO during stagger phase
      measure()
    })
    ro.observe(el)

    return () => {
      if (rafId !== null) cancelAnimationFrame(rafId)
      ro.disconnect()
      // Track active turns and reset counter when all charts unmount
      activeTurns.delete(turnRef.current)
      if (activeTurns.size === 0) chartTurn = 0
    }
  }, [])

  return { ref, width: dims.width, height: dims.height, turn: turnRef.current }
}

/**
 * Staggers data updates across animation frames.
 *
 * When data changes (e.g. from API), each chart waits (turn + 1) RAFs
 * before rendering the new data. This prevents all charts from rendering
 * SVG in the same frame, which causes 6000ms+ frame spikes in WKWebView.
 */
export function useStaggeredData<T>(data: T, turn: number): T {
  const [rendered, setRendered] = useState(data)

  useEffect(() => {
    let remaining = turn + 1
    let rafId: number
    const schedule = () => {
      if (remaining > 0) {
        remaining--
        rafId = requestAnimationFrame(schedule)
      } else {
        setRendered(data)
      }
    }
    rafId = requestAnimationFrame(schedule)
    return () => cancelAnimationFrame(rafId)
  }, [data, turn])

  return rendered
}

/**
 * Creates a memoized chart renderer.
 *
 * The problem: when the store updates, ALL parent chart components re-render.
 * Even though useStaggeredData delays the data, React still re-renders the
 * *WithDimensions child → Recharts re-renders SVG unnecessarily (Recharts
 * doesn't use React.memo internally).
 *
 * Solution: wrap the Recharts render in a memo'd component. When staggered
 * data hasn't changed (same reference from useState), React.memo skips the
 * entire render — no Recharts execution, no SVG reconciliation.
 *
 * Usage:
 *   const MyRenderer = createMemoRenderer(function MyRenderer(props) {
 *     return <RechartsChart data={props.data} ... />
 *   })
 */
export function createMemoRenderer<P extends object>(
  render: (props: P) => ReactNode
) {
  return memo(render)
}

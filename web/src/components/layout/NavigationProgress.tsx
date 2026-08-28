import { useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router-dom'

/**
 * Top progress bar that animates on route changes.
 * Self-contained, no external dependencies.
 *
 * Lifecycle per navigation:
 *   0% → rAF → 80% (CSS transition) → 200ms → 100% → 350ms → hidden
 */
export function NavigationProgress() {
  const { pathname } = useLocation()
  const [progress, setProgress] = useState(0)
  const [visible, setVisible] = useState(false)
  const timers = useRef<ReturnType<typeof setTimeout>[]>([])
  const rafId = useRef<number | null>(null)

  // Clear all pending timers
  const clearTimers = () => {
    timers.current.forEach(clearTimeout)
    timers.current = []
    if (rafId.current !== null) {
      cancelAnimationFrame(rafId.current)
      rafId.current = null
    }
  }

  useEffect(() => {
    clearTimers()
    setVisible(true)
    setProgress(0)

    // Animate to 80% on next frame (CSS transition handles the easing)
    rafId.current = requestAnimationFrame(() => {
      setProgress(80)
    })

    // Complete after a short delay
    timers.current.push(
      setTimeout(() => {
        setProgress(100)
      }, 200)
    )

    // Hide after completion
    timers.current.push(
      setTimeout(() => {
        setVisible(false)
      }, 350)
    )

    return clearTimers
     
  }, [pathname])

  if (!visible) return null

  return (
    <div
      className="fixed top-[env(safe-area-inset-top,0px)] left-0 h-0.5 z-[300] bg-primary pointer-events-none"
      style={{
        width: `${progress}%`,
        transition: 'width 0.2s ease-out',
      }}
      aria-hidden="true"
    />
  )
}

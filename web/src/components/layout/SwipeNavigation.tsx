import { useEffect, useRef } from "react"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { useMobileNav } from "@/store/mobileNav"

/**
 * SwipeNavigation — app-level edge-swipe to go back / forward in history.
 *
 * - Swipe rightward from the LEFT edge → history back.
 * - Swipe leftward from the RIGHT edge → history forward.
 *
 * Disabled while a full-screen overlay (settings dialog) or the mobile nav
 * drawer is open, so the gesture doesn't fight those overlays. Touch-only by
 * nature (listens to touchstart/touchend), so desktop/mouse is unaffected.
 */
export function SwipeNavigation() {
  const navigate = useNavigate()
  const settingsOpen = useStore((s) => s.settingsDialogOpen)
  const drawerOpen = useMobileNav((s) => s.open)

  // Latest overlay state read inside the (stable) touch handlers.
  const blocked = useRef(false)
  blocked.current = settingsOpen || drawerOpen

  const start = useRef<{ x: number; y: number; edge: "left" | "right" | null }>({
    x: 0,
    y: 0,
    edge: null,
  })

  useEffect(() => {
    const EDGE = 28 // px from screen edge that qualifies as an edge swipe
    const THRESHOLD = 60 // min horizontal travel to count as a back/forward swipe
    const MAX_VERTICAL = 50 // reject if the gesture is mostly vertical (scroll)

    const onStart = (e: TouchEvent) => {
      const t = e.touches[0]
      const x = t.clientX
      const w = window.innerWidth
      const edge = x <= EDGE ? "left" : x >= w - EDGE ? "right" : null
      start.current = { x, y: t.clientY, edge }
    }

    const onEnd = (e: TouchEvent) => {
      const s = start.current
      if (!s.edge || blocked.current) return
      const t = e.changedTouches[0]
      const dx = t.clientX - s.x
      if (Math.abs(t.clientY - s.y) > MAX_VERTICAL) return
      if (s.edge === "left" && dx >= THRESHOLD) navigate(-1)
      else if (s.edge === "right" && dx <= -THRESHOLD) navigate(1)
    }

    window.addEventListener("touchstart", onStart, { passive: true })
    window.addEventListener("touchend", onEnd, { passive: true })
    return () => {
      window.removeEventListener("touchstart", onStart)
      window.removeEventListener("touchend", onEnd)
    }
  }, [navigate])

  return null
}

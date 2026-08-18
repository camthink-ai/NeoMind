/**
 * Page sidebar slot — lets a page hoist its own left column (chat sessions,
 * dashboard list) to FULL HEIGHT, left of the TopBar, next to the AppSidebar.
 * The shell renders the empty `#page-sidebar-slot` flex child; pages portal
 * their column into it. Without this, page sidebars render inside `main`,
 * below the TopBar, and the top chrome band spans their full width instead
 * of only the content area.
 *
 * `PageSidebarColumn` wraps the portaled content and tracks its width in
 * `--page-sidebar-width` (ResizeObserver — widths animate, e.g. the session
 * list collapse) so fixed surfaces can offset past BOTH sidebars:
 * ChatPage's keyboard-aware container does
 * `left: calc(var(--app-sidebar-width) + var(--page-sidebar-width))`.
 *
 * Mobile never uses the slot (the shell doesn't render it below md); pages
 * gate the portal on their own isDesktop check and fall back to their
 * in-flow/drawer layout when the slot is absent.
 */

import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react"

/** Resolve the shell's page-sidebar slot element. Watches the DOM: the slot
    unmounts/remounts when the window crosses the mobile breakpoint, and a
    one-shot query would leave a stale reference (the sidebar never comes
    back after widening). */
export function usePageSidebarSlot(): HTMLElement | null {
  const [slot, setSlot] = useState<HTMLElement | null>(null)
  useEffect(() => {
    const update = () => setSlot(document.getElementById("page-sidebar-slot"))
    update()
    const observer = new MutationObserver(update)
    observer.observe(document.body, { childList: true, subtree: true })
    return () => observer.disconnect()
  }, [])
  return slot
}

/**
 * Full-height column wrapper for content portaled into the slot. Publishes
 * its live width as --page-sidebar-width and resets it on unmount.
 */
export function PageSidebarColumn({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const apply = () =>
      document.documentElement.style.setProperty("--page-sidebar-width", `${el.offsetWidth}px`)
    apply()
    const observer = new ResizeObserver(apply)
    observer.observe(el)
    return () => {
      observer.disconnect()
      document.documentElement.style.setProperty("--page-sidebar-width", "0px")
    }
  }, [])

  return (
    <div ref={ref} className="flex h-full shrink-0">
      {children}
    </div>
  )
}

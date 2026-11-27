/**
 * EntityInspectorPane — desktop /chat docked right inspector column.
 *
 * Follows the exact same right-docked architecture as GlobalChatFab / PanelChatView:
 * - Fixed full-height (`inset-y-0 right-0`), flush to the very top and bottom of
 *   the window (`bg-background border-l border-border z-[15]`).
 * - Publishes `--dock-chat-width` on `:root` so `<main>` (including
 *   `GlobalControlsFloating` and the window drag strip) and `ChatPage` yield
 *   cleanly to the left — no top collision, no backdrop, side-by-side chatting.
 * - Left edge is draggable to resize width (persisted in localStorage).
 */

import { useEffect, useRef, useState } from "react"
import { RefreshCw, X } from "lucide-react"
import type { ChatAttachment, ChatReference } from "@/types"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { useEntityInspector } from "./EntityInspectorContent"

const INSPECTOR_WIDTH_KEY = "neomind_inspector_width"
const INSPECTOR_WIDTH_MIN = 360
const INSPECTOR_WIDTH_MAX = 720
const INSPECTOR_WIDTH_DEFAULT = 440

function useInspectorWidth() {
  const [width, setWidth] = useState(() => {
    if (typeof window === "undefined") return INSPECTOR_WIDTH_DEFAULT
    try {
      const raw = window.localStorage.getItem(INSPECTOR_WIDTH_KEY)
      const n = raw ? Number.parseInt(raw, 10) : NaN
      if (Number.isNaN(n)) return INSPECTOR_WIDTH_DEFAULT
      return Math.min(INSPECTOR_WIDTH_MAX, Math.max(INSPECTOR_WIDTH_MIN, n))
    } catch {
      return INSPECTOR_WIDTH_DEFAULT
    }
  })
  useEffect(() => {
    try {
      window.localStorage.setItem(INSPECTOR_WIDTH_KEY, String(Math.round(width)))
    } catch {
      // Ignore storage quota/privacy mode errors
    }
  }, [width])
  return [width, setWidth] as const
}

interface EntityInspectorPaneProps {
  attachment: ChatAttachment | null
  onClose: () => void
  onAttachReference?: (ref: ChatReference) => void
}

export function EntityInspectorPane({
  attachment,
  onClose,
  onAttachReference,
}: EntityInspectorPaneProps) {
  const active = !!attachment && attachment.kind !== "image"
  const panelRef = useRef<HTMLElement>(null)
  const [paneWidth, setPaneWidth] = useInspectorWidth()

  const {
    loading,
    isZh,
    kindMeta,
    refresh,
    body,
    footerButtons,
    testDialog,
  } = useEntityInspector({
    attachment,
    active,
    // Keep the docked pane open when clicking "@ 引用到聊天" so the user can
    // keep reading while typing their follow-up message on the left.
    keepOpenOnMention: true,
    onClose,
    onAttachReference,
  })

  // Publish --dock-chat-width so <main> (GlobalControlsFloating + drag strip)
  // and ChatPage's fixed shell yield smoothly to the left — identical to
  // GlobalChatFab's wide docked mode (which is unmounted on /chat).
  useEffect(() => {
    const px = active ? `${paneWidth}px` : "0px"
    document.documentElement.style.setProperty("--dock-chat-width", px)
    return () => {
      document.documentElement.style.setProperty("--dock-chat-width", "0px")
    }
  }, [active, paneWidth])

  // Escape closes the docked inspector cleanly unless a nested modal/lightbox is open.
  useEffect(() => {
    if (!active) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return
      if (document.querySelector('[role="dialog"][data-state="open"]')) return
      onClose()
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [active, onClose])

  const resizeCleanupRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    return () => {
      resizeCleanupRef.current?.()
      resizeCleanupRef.current = null
    }
  }, [])

  // Drag the panel's left edge to resize (right-anchored: width = rightEdge - clientX)
  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    const panelEl = panelRef.current
    if (!panelEl) return
    resizeCleanupRef.current?.()
    const rightEdge = panelEl.getBoundingClientRect().right
    const onMove = (ev: PointerEvent) => {
      const next = Math.round(
        Math.min(INSPECTOR_WIDTH_MAX, Math.max(INSPECTOR_WIDTH_MIN, rightEdge - ev.clientX))
      )
      setPaneWidth(next)
    }
    const cleanup = () => {
      window.removeEventListener("pointermove", onMove)
      window.removeEventListener("pointerup", cleanup)
      window.removeEventListener("pointercancel", cleanup)
      if (resizeCleanupRef.current === cleanup) {
        resizeCleanupRef.current = null
      }
    }
    resizeCleanupRef.current = cleanup
    window.addEventListener("pointermove", onMove)
    window.addEventListener("pointerup", cleanup)
    window.addEventListener("pointercancel", cleanup)
  }

  if (!active || !attachment || !kindMeta) return null

  const Icon = kindMeta.Icon

  return (
    <>
      <aside
        ref={panelRef}
        aria-label={attachment.title}
        data-testid="entity-inspector-pane"
        style={{ width: paneWidth }}
        className="fixed inset-y-0 right-0 z-[15] flex flex-col overflow-hidden border-l border-border bg-background animate-slide-in-from-right"
      >
        {/* Left-edge drag handle — same pattern as GlobalChatFab */}
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={isZh ? "调整面板宽度" : "Resize panel"}
          onPointerDown={startResize}
          className="absolute inset-y-0 left-0 z-10 w-2 cursor-col-resize touch-none select-none"
        />

        {/* Header — matched h-14 (56px) with footer */}
        <div className="flex h-14 flex-shrink-0 items-center justify-between gap-2 border-b border-glass-border px-5">
          <div className="flex min-w-0 flex-1 items-center gap-2.5">
            <div
              className={cn(
                "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
                kindMeta.iconBg
              )}
            >
              <Icon className={cn("h-4 w-4", kindMeta.color)} strokeWidth={1.75} aria-hidden="true" />
            </div>
            <h2
              className="truncate text-sm font-semibold leading-tight text-foreground"
              title={attachment.title}
            >
              {attachment.title}
            </h2>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={refresh}
              disabled={loading}
              aria-label={isZh ? "刷新" : "Refresh"}
              title={isZh ? "刷新数据" : "Refresh"}
              className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground sm:h-8 sm:w-8"
            >
              <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} />
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={onClose}
              aria-label={isZh ? "收起详情" : "Close inspector"}
              title={isZh ? "收起 (Esc)" : "Close (Esc)"}
              className="h-8 w-8 rounded-lg text-muted-foreground hover:text-foreground sm:h-8 sm:w-8"
            >
              <X className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Full-height scrollable body */}
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-5">
          {body}
        </div>

        {/* Sticky action footer — matched h-14 (56px) with header */}
        <div className="flex h-14 flex-shrink-0 items-center justify-end gap-2 border-t border-glass-border px-5">
          {footerButtons}
        </div>
      </aside>

      {testDialog}
    </>
  )
}

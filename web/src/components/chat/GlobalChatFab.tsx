/**
 * GlobalChatFab - Floating action button + chat panel.
 *
 * Shows a FAB on all non-chat pages. Clicking opens the panel:
 * - WIDE (>=1280px): a DOCKED right column — full height, 400px wide,
 *   in-flow beside the page content (which squeezes). Rendered by App
 *   inside the content row via `GlobalChatDock`.
 * - NARROW: a floating window anchored bottom-right, scale-up animation.
 *
 * The panel chat has its own independent session — does not affect the main chat page.
 */

import { useState, useEffect, useRef, lazy, Suspense } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { useTranslation } from "react-i18next"
import { MessageSquare } from "lucide-react"
import { notifyInfo } from "@/lib/notify"

// Lazy: the panel drags the whole markdown/syntax-highlight stack
// (vendor-markdown ~325KB) — only pay for it when the FAB is actually opened.
const PanelChatView = lazy(() =>
  import("./PanelChatView").then((m) => ({ default: m.PanelChatView }))
)
import { cn } from "@/lib/utils"

type PanelState = "closed" | "opening" | "open" | "closing"

function useIsWideViewport() {
  const [isWide, setIsWide] = useState(() =>
    typeof window !== "undefined" ? window.matchMedia("(min-width: 1280px)").matches : false
  )
  useEffect(() => {
    const mq = window.matchMedia("(min-width: 1280px)")
    const onChange = () => setIsWide(mq.matches)
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [])
  return isWide
}

export function GlobalChatFab() {
  const [panelState, setPanelState] = useState<PanelState>("closed")
  const [isStreaming, setIsStreaming] = useState(false)
  const location = useLocation()
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const { t } = useTranslation("chat")
  const fabRef = useRef<HTMLButtonElement>(null)
  const isWide = useIsWideViewport()

  const isOpen = panelState === "open" || panelState === "opening"

  // Detect chat pages: /, /chat, /chat/:sessionId
  const isChatPage = location.pathname === "/" || location.pathname.startsWith("/chat")

  // Auto-close on /chat navigation (delay if streaming)
  useEffect(() => {
    if (isChatPage && isOpen) {
      if (isStreaming) {
        notifyInfo(t("streamInProgress"))
        return
      }
      handleClose()
    }
  }, [isChatPage, isOpen, isStreaming, t])

  const handleOpen = () => setPanelState("open")
  const handleClose = () => setPanelState("closed")

  // Publish the dock width so main/page-footers can yield (wide mode only)
  useEffect(() => {
    const px = isWide && isOpen ? "400px" : "0px"
    document.documentElement.style.setProperty("--dock-chat-width", px)
    return () => {
      document.documentElement.style.setProperty("--dock-chat-width", "0px")
    }
  }, [isWide, isOpen])

  // Hide FAB entirely on chat pages
  if (isChatPage) return null

  const panel = (
    <Suspense fallback={null}>
      <PanelChatView
        onClose={handleClose}
        onStreamingChange={setIsStreaming}
        showMinimize={!isWide}
        onNavigateToSettings={() => openSettings()}
      />
    </Suspense>
  )

  return (
    <>
      {/* Floating action button — ink circle */}
      <button
        ref={fabRef}
        onClick={isOpen ? handleClose : handleOpen}
        aria-label={isOpen ? t("closePanel") : t("openPanel")}
        className={cn(
          "fixed bottom-[calc(5rem+var(--keyboard-offset,0px))] right-6 z-50",
          "w-14 h-14 rounded-full",
          "flex items-center justify-center",
          "transition-all duration-300 ease-out",
          "safe-bottom",
          "bg-primary text-primary-foreground",
          "border border-transparent",
          "shadow-lg",
          "hover:shadow-xl hover:bg-primary-hover",
          isOpen
            ? "scale-0 opacity-0 pointer-events-none"
            : "scale-100 opacity-100 hover:scale-105"
        )}
      >
        <MessageSquare className="h-5 w-5" />
      </button>

      {isWide ? (
        // WIDE: docked right column — fixed to the window's right edge;
        // main yields via the --dock-chat-width var (same pattern as
        // --app-sidebar-width), so the page content squeezes left
        <aside
          className={cn(
            "fixed inset-y-0 right-0 w-[400px] flex-col overflow-hidden border-l border-border bg-background z-[15]",
            isOpen ? "flex animate-slide-in-from-right" : "hidden"
          )}
        >
          {isOpen && panel}
        </aside>
      ) : (
        // NARROW: floating window anchored bottom-right
        <div
          className={cn(
            "fixed z-[90]",
            "origin-bottom-right",
            "transition-all duration-300 ease-out",
            panelState !== "closed"
              ? "bottom-[calc(6rem+var(--keyboard-offset,0px))] right-6 w-[calc(100dvw-3rem)] h-[70dvh] sm:w-[380px] sm:h-[560px] rounded-xl opacity-100 scale-100"
              : "bottom-20 right-6 w-14 h-14 rounded-full opacity-0 scale-0 pointer-events-none",
            "backdrop-blur-2xl",
            "border border-glass-border",
            "shadow-xl",
            "flex flex-col overflow-hidden"
          )}
          style={{ backgroundColor: "var(--surface-glass)", backdropFilter: "blur(40px) saturate(1.8)" }}
        >
          {panelState !== "closed" && panel}
        </div>
      )}
    </>
  )
}

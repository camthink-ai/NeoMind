/**
 * GlobalChatFab - Floating action button + floating chat window
 *
 * Shows a FAB on all non-chat pages. Clicking opens a floating chat window
 * anchored to the bottom-right, with smooth scale-up animation from the FAB position.
 * Minimize button collapses back to FAB with reverse animation.
 *
 * The panel chat has its own independent session — does not affect the main chat page.
 */

import { useState, useEffect, useRef } from "react"
import { useLocation, useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { useTranslation } from "react-i18next"
import { MessageSquare } from "lucide-react"
import { PanelChatView } from "./PanelChatView"
import { notifyInfo } from "@/lib/notify"
import { cn } from "@/lib/utils"

type PanelState = "closed" | "opening" | "open" | "closing"

export function GlobalChatFab() {
  const [panelState, setPanelState] = useState<PanelState>("closed")
  const [isStreaming, setIsStreaming] = useState(false)
  const location = useLocation()
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const { t } = useTranslation("chat")
  const fabRef = useRef<HTMLButtonElement>(null)

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

  const handleOpen = () => {
    setPanelState("opening")
    requestAnimationFrame(() => {
      setTimeout(() => setPanelState("open"), 300)
    })
  }

  const handleClose = () => {
    setPanelState("closing")
    setTimeout(() => setPanelState("closed"), 250)
  }

  // Hide FAB entirely on chat pages
  if (isChatPage) return null

  return (
    <>
      {/* Floating action button — glass + glow */}
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
          // Glass background with brand orange
          "bg-accent-orange-bg backdrop-blur-xl",
          "border border-accent-orange",
          "text-accent-orange",
          // Glow ring
          "shadow-[0_0_24px_var(--accent-orange-bg),0_0_48px_var(--accent-orange-bg)]",
          "hover:shadow-[0_0_32px_var(--accent-orange),0_0_64px_var(--accent-orange-bg)]",
          "hover:border-accent-orange",
          isOpen
            ? "scale-0 opacity-0 pointer-events-none"
            : "scale-100 opacity-100 hover:scale-105"
        )}
      >
        <MessageSquare className="h-5 w-5" />
      </button>

      {/* Floating chat window — bottom-right corner */}
      <div
        className={cn(
          "fixed z-[100]",
          // Origin from bottom-right (FAB position)
          "origin-bottom-right",
          // Smooth transitions
          "transition-all duration-300 ease-out",
          // State-dependent positioning & sizing
          panelState !== "closed"
            ? "bottom-[calc(6rem+var(--keyboard-offset,0px))] right-6 w-[calc(100dvw-3rem)] h-[70dvh] sm:w-[380px] sm:h-[560px] rounded-2xl opacity-100 scale-100"
            : "bottom-20 right-6 w-14 h-14 rounded-full opacity-0 scale-0 pointer-events-none",
          "backdrop-blur-2xl",
          "border border-glass-border",
          "shadow-2xl",
          "flex flex-col overflow-hidden"
        )}
        style={{ backgroundColor: "var(--surface-glass)", backdropFilter: "blur(40px) saturate(1.8)" }}
      >
        {/* Only render content when visible */}
        {panelState !== "closed" && (
          <PanelChatView
            onClose={handleClose}
            onStreamingChange={setIsStreaming}
            showMinimize
            onNavigateToSettings={() => openSettings()}
          />
        )}
      </div>
    </>
  )
}

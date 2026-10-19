/**
 * ChatScrollPill — the single "jump to latest" control shared by the chat
 * page and the floating panel. It lives outside the scroll container so it
 * stays visible while the user reads history; the old per-message chevron
 * scrolled away with the message it belonged to.
 */

import { useTranslation } from "react-i18next"
import { ArrowDown } from "lucide-react"
import { cn } from "@/lib/utils"

interface ChatScrollPillProps {
  visible: boolean
  /** While a turn streams and the user is scrolled up, say "new content". */
  hasNewContent?: boolean
  onClick: () => void
  className?: string
}

export function ChatScrollPill({
  visible,
  hasNewContent = false,
  onClick,
  className,
}: ChatScrollPillProps) {
  const { t } = useTranslation("chat")
  const label = hasNewContent ? t("newContent") : t("scrollToBottom")

  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      tabIndex={visible ? 0 : -1}
      className={cn(
        "absolute bottom-4 right-4 z-10 inline-flex h-8 items-center gap-1.5 rounded-full",
        "border border-border bg-card px-3 text-xs text-muted-foreground shadow-md",
        "transition-all duration-fast ease-out",
        "hover:text-foreground hover:bg-muted-50",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        visible
          ? "opacity-100 translate-y-0 pointer-events-auto"
          : "opacity-0 translate-y-1 pointer-events-none",
        className
      )}
    >
      {hasNewContent && (
        <span className="h-1.5 w-1.5 rounded-full bg-info animate-pulse" aria-hidden="true" />
      )}
      <ArrowDown className="h-3.5 w-3.5" aria-hidden="true" />
      <span className="hidden sm:inline">{label}</span>
    </button>
  )
}

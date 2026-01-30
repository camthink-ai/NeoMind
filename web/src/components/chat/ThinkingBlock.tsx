/**
 * ThinkingBlock - Displays AI reasoning process
 * Clean, modern design
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { BrainCircuit, ChevronDown } from "lucide-react"
import { cn } from "@/lib/utils"

interface ThinkingBlockProps {
  thinking: string
  isStreaming?: boolean
  defaultExpanded?: boolean
}

export function ThinkingBlock({
  thinking,
  isStreaming = false,
  defaultExpanded = false
}: ThinkingBlockProps) {
  const { t } = useTranslation("chat")
  const [isExpanded, setIsExpanded] = useState(defaultExpanded)

  if (!thinking) return null

  return (
    <div className="my-3 rounded-xl border border-border/60 bg-muted/30 overflow-hidden">
      {/* Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-3 px-4 py-2.5 text-left hover:bg-muted/50 transition-colors"
      >
        <BrainCircuit className={cn(
          "h-4 w-4",
          isStreaming ? "text-blue-500 animate-pulse" : "text-muted-foreground"
        )} />

        <span className="text-sm font-medium">{t("thinking.title")}</span>

        {isStreaming && (
          <span className="text-xs text-muted-foreground">{t("thinking.thinking")}</span>
        )}

        <div className="flex-1" />

        <span className="text-xs text-muted-foreground/70 mr-2">
          {thinking.length} {t("thinking.characters")}
        </span>

        <ChevronDown className={cn(
          "h-4 w-4 text-muted-foreground transition-transform duration-200",
          isExpanded && "rotate-180"
        )} />
      </button>

      {/* Content */}
      {isExpanded && (
        <div className="px-4 pb-3">
          <div className="text-sm leading-relaxed text-muted-foreground whitespace-pre-wrap break-words">
            {thinking}
            {isStreaming && <span className="inline-block w-1 h-4 bg-muted-foreground/30 animate-pulse ml-1 align-middle" />}
          </div>
        </div>
      )}
    </div>
  )
}

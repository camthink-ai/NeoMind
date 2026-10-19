import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Copy, Check } from "@/design-system/icons"
import { cn } from "@/lib/utils"
import { copyToClipboard } from '@/lib/clipboard'

interface CopyMessageButtonProps {
  content: string
  className?: string
}

/**
 * Copy-whole-message button.
 *
 * Copies the raw markdown source (not the rendered text) so code fences,
 * lists, and tables survive a paste into a markdown-aware target — the same
 * behavior as ChatGPT/Claude.
 *
 * Visibility is owned by the caller's action-row wrapper (hover/focus reveal
 * on desktop, always-on for touch). The button itself stays focusable so
 * keyboard users can reveal the row by tabbing to it.
 */
export function CopyMessageButton({ content, className }: CopyMessageButtonProps) {
  const [copied, setCopied] = useState(false)
  const { t } = useTranslation("chat")

  if (!content) return null

  const handleCopy = async () => {
    try {
      await copyToClipboard(content)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 2000)
    } catch {
      /* clipboard blocked (permissions / non-secure context) — silent */
    }
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={cn(
        "inline-flex h-8 w-8 sm:h-7 sm:w-7 items-center justify-center rounded text-muted-foreground transition-colors",
        "opacity-100",
        "hover:bg-muted hover:text-foreground",
        "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        className
      )}
      aria-label={copied ? t("code.copied", "已复制") : t("copyMessage", "复制消息")}
      title={copied ? t("code.copied", "已复制") : t("copyMessage", "复制消息")}
    >
      {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </button>
  )
}

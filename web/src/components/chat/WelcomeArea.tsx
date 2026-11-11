/**
 * WelcomeArea - Clean welcome area shown when no active conversation
 * Mobile keeps a quiet title and composer; desktop retains the richer brand
 * welcome and prompt suggestions.
 */

import { ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { ArrowRight } from "lucide-react"
import { BrandLogoHorizontal } from "@/components/shared/BrandName"
import { useIsMobile } from "@/hooks/useMobile"

interface WelcomeAreaProps {
  className?: string
  onQuickAction?: (prompt: string) => void
  /** The composer stays anchored at the bottom of the empty chat. */
  children?: ReactNode
}

export function WelcomeArea({ className, onQuickAction, children }: WelcomeAreaProps) {
  const { t } = useTranslation("common")
  const isMobile = useIsMobile()

  // Get greeting based on time
  const getGreetingKey = () => {
    const hour = new Date().getHours()
    if (hour < 6) return "welcome.greeting.earlyMorning"
    if (hour < 12) return "welcome.greeting.morning"
    if (hour < 18) return "welcome.greeting.afternoon"
    return "welcome.greeting.evening"
  }

  const promptSuggestions = [
    t("welcome.suggestionPrompts.checkDevices"),
    t("welcome.suggestionPrompts.createRule"),
    t("welcome.suggestionPrompts.checkAlerts"),
    t("welcome.suggestionPrompts.showLogs"),
    t("welcome.suggestionPrompts.createAgent"),
    t("welcome.suggestionPrompts.checkAgents"),
    t("welcome.suggestionPrompts.addLlmBackend"),
    t("welcome.suggestionPrompts.createVirtualDevice"),
  ]

  if (isMobile) {
    return (
      <div className={cn("relative flex min-h-full w-full flex-col items-center overflow-hidden", className)}>
        <div className="relative flex min-h-full w-full max-w-3xl flex-col">
          <div className="flex min-h-0 flex-1 items-center justify-center px-4 pb-6 text-center">
            <h1 className="text-xl font-semibold leading-snug tracking-[-0.02em] text-foreground">
              {t('common:welcome.askLine', { greeting: t(getGreetingKey()) })}
            </h1>
          </div>

          <div className="chat-welcome-composer w-full shrink-0">
            {children}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className={cn("relative flex min-h-full w-full flex-col items-center overflow-hidden p-6", className)}>
      <div aria-hidden="true" className="welcome-dots" />
      <div className="min-h-0 flex-1 shrink" />
      <div className="relative w-full max-w-3xl shrink-0 space-y-8">
        <div className="text-center">
          <div className="mb-4 flex justify-center">
            <BrandLogoHorizontal className="h-9" />
          </div>
          <h1 className="text-xl font-semibold leading-snug tracking-[-0.02em] text-foreground">
            {t('common:welcome.askLine', { greeting: t(getGreetingKey()) })}
          </h1>
        </div>

        <div className="chat-welcome-composer w-full">
          {children}
        </div>

        <div className="flex flex-wrap items-center justify-center gap-2">
          {promptSuggestions.map((prompt) => (
            <button
              key={prompt}
              type="button"
              onClick={() => onQuickAction?.(prompt)}
              className="group inline-flex h-9 max-w-full items-center gap-1.5 rounded-full border border-border bg-transparent px-4 text-sm text-muted-foreground transition-colors hover:border-border-hover hover:bg-muted hover:text-foreground"
            >
              <span className="truncate">{prompt}</span>
              {/* Keep a fixed icon slot — hover width growth reflows flex-wrap and flickers. */}
              <ArrowRight
                aria-hidden="true"
                className="h-3.5 w-3.5 shrink-0 opacity-0 transition-[opacity,transform] duration-150 group-hover:translate-x-0.5 group-hover:opacity-100"
              />
            </button>
          ))}
        </div>
      </div>
      <div className="min-h-0 flex-1 shrink" />
    </div>
  )
}

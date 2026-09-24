/**
 * WelcomeArea - Clean welcome area shown when no active conversation
 * Shows greeting and suggested prompts (composer slots in between)
 */

import { ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"
import { MessageSquare, ArrowRight } from "lucide-react"
import { useBrandMessages } from "@/hooks/useBrand"

interface WelcomeAreaProps {
  className?: string
  onQuickAction?: (prompt: string) => void
  /**
   * Slotted in as the middle of the centered group — the chat composer.
   * Sits between the greeting block and the prompt suggestions so
   * the whole group (greeting → input → suggestions) reads as one unit,
   * the mainstream empty-state pattern (ChatGPT / Claude).
   */
  children?: ReactNode
}

export function WelcomeArea({ className, onQuickAction, children }: WelcomeAreaProps) {
  const { t } = useTranslation("common")
  const { getWelcomeMessage } = useBrandMessages()

  // Get greeting based on time
  const getGreetingKey = () => {
    const hour = new Date().getHours()
    if (hour < 6) return "welcome.greeting.earlyMorning"
    if (hour < 12) return "welcome.greeting.morning"
    if (hour < 18) return "welcome.greeting.afternoon"
    return "welcome.greeting.evening"
  }

  // Prompt suggestions
  const promptSuggestions = [
    t("welcome.suggestionPrompts.checkDevices"),
    t("welcome.suggestionPrompts.createRule"),
    t("welcome.suggestionPrompts.checkAlerts"),
    t("welcome.suggestionPrompts.showLogs"),
  ]

  return (
    <div className={cn("relative flex min-h-full w-full flex-col items-center overflow-hidden p-6", className)}>
      {/* Flowing brand aurora — two large radial blobs drifting behind the
          centered group (see .welcome-aurora in index.css). Decorative. */}
      <div aria-hidden="true" className="welcome-aurora" />
      {/* Top spacer */}
      <div className="min-h-0 flex-1 shrink" />
      {/* max-w-3xl — same width as the conversation view's composer, so the
          slotted input keeps its conversational width and the suggestion
          grid spans it too. relative keeps it painting above the aurora. */}
      <div className="relative w-full max-w-3xl shrink-0 space-y-8">
        {/* Greeting */}
        <div className="text-center">
          <div className="flex items-center justify-center gap-2.5 mb-3">
            <div className="w-10 h-10 rounded-xl bg-primary-light flex items-center justify-center">
              <MessageSquare className="h-5 w-5 text-primary" />
            </div>
          </div>
          <h1 className="text-2xl font-semibold text-foreground mb-2">
            {t(getGreetingKey())}
          </h1>
          <p className="text-muted-foreground">
            {getWelcomeMessage("tagline")}
          </p>
        </div>

        {/* Slotted composer — the input lives inside the centered group */}
        {children}

        {/* Prompt suggestions — quiet pill chips, self-explanatory: no
            "try these questions" caption needed above them */}
        <div className="flex flex-wrap items-center justify-center gap-2">
          {promptSuggestions.map((prompt) => (
            <button
              key={prompt}
              onClick={() => onQuickAction?.(prompt)}
              className="group inline-flex max-w-full items-center gap-1 rounded-full border border-border bg-transparent px-4 h-9 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground hover:border-muted-foreground/40"
            >
              <span className="truncate">{prompt}</span>
              <ArrowRight className="h-3.5 w-3.5 shrink-0 transition-all opacity-0 -ml-1.5 w-0 group-hover:opacity-100 group-hover:ml-0 group-hover:w-3.5" />
            </button>
          ))}
        </div>
      </div>
      {/* Bottom spacer */}
      <div className="min-h-0 flex-1 shrink" />
    </div>
  )
}

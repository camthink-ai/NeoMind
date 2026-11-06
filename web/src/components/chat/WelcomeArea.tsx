/**
 * WelcomeArea - Clean welcome area shown when no active conversation
 * Shows one quiet greeting above the composer.
 */

import { ReactNode } from "react"
import { useTranslation } from "react-i18next"
import { cn } from "@/lib/utils"

interface WelcomeAreaProps {
  className?: string
  /** The composer stays anchored at the bottom of the empty chat. */
  children?: ReactNode
}

export function WelcomeArea({ className, children }: WelcomeAreaProps) {
  const { t } = useTranslation("common")

  // Get greeting based on time
  const getGreetingKey = () => {
    const hour = new Date().getHours()
    if (hour < 6) return "welcome.greeting.earlyMorning"
    if (hour < 12) return "welcome.greeting.morning"
    if (hour < 18) return "welcome.greeting.afternoon"
    return "welcome.greeting.evening"
  }

  return (
    <div className={cn("relative flex min-h-full w-full flex-col items-center overflow-hidden px-0 py-0 sm:p-6", className)}>
      {/* Dot field backdrop — each dot an edge device, masked to fade toward
          the edges (see .welcome-dots in index.css). Decorative. */}
      <div aria-hidden="true" className="welcome-dots max-sm:hidden" />
      <div className="relative flex min-h-full w-full max-w-3xl flex-col">
        <div className="flex min-h-0 flex-1 items-center justify-center px-4 pb-6 text-center sm:pb-8">
          <h1 className="text-[22px] font-semibold leading-snug tracking-[-0.02em] text-foreground sm:text-xl">
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

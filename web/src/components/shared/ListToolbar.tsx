import { ReactNode } from "react"
import { ArrowLeft } from "lucide-react"
import { cn } from "@/lib/utils"

interface ListToolbarProps {
  onBack: () => void
  /** Back-button label (caller passes the i18n string). */
  backLabel: string
  /** Already-sized icon node rendered inside the tinted container. */
  icon: ReactNode
  /** Background tint class for the icon container (e.g. "bg-primary-light"). */
  iconBg: string
  title: string
  description?: string
  /** Optional badges rendered inline with the title (e.g. LLM streaming / api-key). */
  badges?: ReactNode
  /** Use a responsive icon container (w-10 sm:w-12); default fixed w-10 h-10. */
  responsiveIcon?: boolean
}

/**
 * ListToolbar — sticky detail-view header shared by the LLM Backends and
 * Device Connections settings tabs. Back button + icon + title (with optional
 * badges) + description, stuck to the top of the scroll container. The
 * `::before` strip covers the 8px mobile scroll-padding gap so content
 * scrolling under it doesn't show through (see PageLayout mobile pt-2).
 */
export function ListToolbar({
  onBack,
  backLabel,
  icon,
  iconBg,
  title,
  description,
  badges,
  responsiveIcon = false,
}: ListToolbarProps) {
  return (
    <div className="sticky top-0 z-10 -mx-4 sm:-mx-6 md:-mx-8 px-4 sm:px-6 md:px-8 pb-2 bg-background flex flex-col sm:flex-row sm:items-center gap-3 sm:gap-4 mb-4 before:content-[''] before:absolute before:inset-x-0 before:-top-2 before:h-2 before:bg-background md:before:hidden">
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-sm text-muted-foreground hover:bg-muted-30 hover:text-foreground transition-colors self-start -ml-2"
      >
        <ArrowLeft className="w-4 h-4" />
        {backLabel}
      </button>
      <div className="flex items-center gap-3 min-w-0 flex-1">
        <div
          className={cn(
            "flex items-center justify-center rounded-lg shrink-0",
            responsiveIcon ? "w-10 sm:w-12 h-10 sm:h-12" : "w-10 h-10",
            iconBg
          )}
        >
          {icon}
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <h2 className="text-lg sm:text-2xl font-bold truncate">{title}</h2>
            {badges}
          </div>
          {description && (
            <p className="text-sm text-muted-foreground line-clamp-2">{description}</p>
          )}
        </div>
      </div>
    </div>
  )
}

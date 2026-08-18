/**
 * GlobalControls — the top-right entry points: theme, language, alerts.
 * (Instance selector and onboarding live in the AppSidebar rail.)
 * Self-contained; rendered inline at the right of every page's top toolbar,
 * or floating (GlobalControlsFloating) for toolbar-less pages (chat).
 */

import { useTranslation } from "react-i18next"
import { Languages } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ThemeToggle } from "./ThemeToggle"
import { AlertsMenu } from "./AlertsMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

function Controls() {
  const { t, i18n } = useTranslation("common")

  return (
    <TooltipProvider delayDuration={500}>
      <div className="flex shrink-0 items-center gap-1.5">
        <ThemeToggle />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t('userMenu.language', { defaultValue: 'Language' })}
              className="shrink-0 text-muted-foreground hover:text-foreground no-press-scale"
            >
              <Languages className="h-5 w-5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-32">
            <DropdownMenuItem onClick={() => i18n.changeLanguage('zh')} className="gap-2 cursor-pointer">
              <span className="text-sm">中文</span>
              {i18n.language === 'zh' && <span className="ml-auto text-xs text-foreground">✓</span>}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => i18n.changeLanguage('en')} className="gap-2 cursor-pointer">
              <span className="text-sm">English</span>
              {i18n.language === 'en' && <span className="ml-auto text-xs text-foreground">✓</span>}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <AlertsMenu compact align="end" side="bottom" tooltipSide="bottom" />
      </div>
    </TooltipProvider>
  )
}

/** Inline cluster — embed at the right of a page toolbar. */
export function GlobalControls() {
  return <Controls />
}

/** Floating cluster — for pages without a toolbar (chat): top-right overlay. */
export function GlobalControlsFloating() {
  return (
    <div className="pointer-events-none absolute right-4 sm:right-6 z-20" style={{ top: "calc(env(safe-area-inset-top, 0px) + 0.5rem)" }}>
      <div className="pointer-events-auto">
        <Controls />
      </div>
    </div>
  )
}

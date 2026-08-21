/**
 * GlobalControlsFloating — the top-right floating cluster: theme, language,
 * alerts. (Instance selector and onboarding live in the AppSidebar rail.)
 * Floats over the content area's top-right on desktop.
 */

import { useTranslation } from "react-i18next"
import { Languages } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ThemeToggle } from "./ThemeToggle"
import { BuiltinDownloadIndicator } from "@/components/llm/BuiltinDownloadIndicator"
import { AlertsMenu } from "./AlertsMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

export function GlobalControlsFloating() {
  const { t, i18n } = useTranslation("common")

  // Low-key floating cluster: no border/shadow (it only ever overlaps
  // content in narrow windows), just a faint surface so scrolled content
  // stays readable.
  return (
    <div className="pointer-events-none absolute right-4 sm:right-6 z-20" style={{ top: "calc(env(safe-area-inset-top, 0px) + 0.5rem)" }}>
      <div className="pointer-events-auto rounded-full bg-background/60 px-1 py-0.5 backdrop-blur-sm">
    <TooltipProvider delayDuration={500}>
      <div className="flex shrink-0 items-center gap-1.5">
        <BuiltinDownloadIndicator />
        <ThemeToggle />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t('userMenu.language', { defaultValue: 'Language' })}
              className="shrink-0 text-muted-foreground hover:text-foreground no-press-scale"
            >
              <Languages className="h-4 w-4" />
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
      </div>
    </div>
  )
}

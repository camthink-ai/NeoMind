/**
 * GlobalUtilityBar — the content area's top-right corner cluster:
 * instance selector, theme toggle, language switch.
 *
 * Floats OVER the content (absolute, right-aligned, above the page scroll
 * bar) so it costs zero layout height — pages keep the full window. The
 * AppSidebar rail keeps alerts/onboarding/settings/user; these three were
 * moved here per the feedback that they belong at the page's top-right.
 */

import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Languages } from "lucide-react"
import { Button } from "@/components/ui/button"
import { InstanceSelector } from "./InstanceSelector"
import { ThemeToggle } from "./ThemeToggle"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

export function GlobalUtilityBar() {
  const { t, i18n } = useTranslation("common")
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)

  return (
    <>
      <div
        className="pointer-events-none absolute right-4 sm:right-6 z-20 flex items-center gap-1.5"
        style={{ top: "calc(env(safe-area-inset-top, 0px) + 0.5rem)" }}
      >
        {/* pointer-events-none on the wrapper so page content can still be
            interacted with around the cluster; each child re-enables */}
        <div className="pointer-events-auto">
          <InstanceSelector
            compact
            onManageInstances={() => setInstanceManagerOpen(true)}
          />
        </div>
        <div className="pointer-events-auto">
          <ThemeToggle />
        </div>
        <div className="pointer-events-auto">
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
        </div>
      </div>

      {/* Owned dialog (self-contained cluster) */}
      <InstanceManagerDialog
        open={instanceManagerOpen}
        onOpenChange={setInstanceManagerOpen}
      />
    </>
  )
}

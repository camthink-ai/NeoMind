/**
 * GlobalControls — the five global entry points (instance, theme, language,
 * alerts, onboarding) + their dialogs. Self-contained; rendered inline at
 * the RIGHT of every page's top toolbar (DashboardToolbar, PageTabsBar) so
 * the controls sit in the same aligned row across all pages.
 *
 * `floating` renders the cluster in an absolutely-positioned wrapper for
 * pages without a toolbar (chat): top-right of the content area.
 */

import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Languages, Rocket } from "lucide-react"
import { Button } from "@/components/ui/button"
import { InstanceSelector } from "./InstanceSelector"
import { ThemeToggle } from "./ThemeToggle"
import { AlertsMenu } from "./AlertsMenu"
import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

function Controls({ alignDropdowns = "end" }: { alignDropdowns?: "start" | "end" }) {
  const { t, i18n } = useTranslation("common")
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()
  useEffect(() => {
    fetchOnboardingStatus()
  }, [fetchOnboardingStatus])
  const onboardingIncomplete =
    !!onboardingStatus &&
    !onboardingStatus.dismissed &&
    (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)

  return (
    <TooltipProvider delayDuration={500}>
      <div className="flex shrink-0 items-center gap-1.5">
        <InstanceSelector
          compact
          onManageInstances={() => setInstanceManagerOpen(true)}
        />
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
          <DropdownMenuContent align={alignDropdowns} className="w-32">
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
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t("onboarding.title")}
              className="relative shrink-0 text-muted-foreground hover:text-foreground no-press-scale"
              onClick={() => setOnboardingOpen(true)}
            >
              <Rocket className="h-4 w-4" />
              {onboardingIncomplete && (
                <span className="absolute top-1 right-1 h-1.5 w-1.5 rounded-full bg-primary" />
              )}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom" className="text-xs px-2 py-1">
            {t("onboarding.title")}
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Owned dialogs (self-contained cluster) */}
      <InstanceManagerDialog
        open={instanceManagerOpen}
        onOpenChange={setInstanceManagerOpen}
      />
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={setOnboardingOpen}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
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

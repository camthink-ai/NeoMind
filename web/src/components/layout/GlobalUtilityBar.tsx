/**
 * GlobalUtilityBar — a thin (40px) GLOBAL top row in the content column,
 * right-aligned: instance selector, theme toggle, language switch, alerts,
 * onboarding.
 *
 * In-flow (not floating): pages render BELOW it, so no page toolbar can
 * ever overlap it. The row's left area is empty drag space (macOS Tauri).
 * Hidden on mobile (per-page MobilePageHeader covers those needs).
 */

import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { Languages, Rocket } from "lucide-react"
import { cn } from "@/lib/utils"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
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

// Tauri window drag — same contract as the sidebar header: startDragging on
// mousedown over non-interactive areas.
const handleDragMouseDown = (e: React.MouseEvent) => {
  if (!isTauriEnv()) return
  const target = e.target as HTMLElement
  if (target.closest("button, a, input, select, textarea, [role='button'], [role='tab']")) return
  getCurrentWindow().startDragging()
}

export function GlobalUtilityBar() {
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
      <div
        className="relative z-20 flex h-10 shrink-0 items-center bg-[var(--chrome)] px-4 sm:px-6"
        onMouseDown={handleDragMouseDown}
      >
        {/* Empty drag space */}
        <div className="flex-1" />

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
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={t("onboarding.title")}
                className={cn("relative shrink-0 text-muted-foreground hover:text-foreground no-press-scale")}
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

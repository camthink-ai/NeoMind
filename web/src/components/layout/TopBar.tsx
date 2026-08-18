/**
 * TopBar — slim desktop chrome bar.
 * Carries the window drag region (macOS overlay titlebar) and the global
 * action cluster: instance selector, onboarding guide, alerts bell. The
 * user menu lives on the AppSidebar footer (bottom-left); settings entry
 * also lives in the sidebar footer. In-flow (not fixed) inside the app
 * shell's content column; height exported as --topnav-height.
 */

import { useState, useEffect, useRef, useCallback, forwardRef } from "react"
import { useTranslation } from "react-i18next"
import { Rocket } from "lucide-react"
import { useStore } from "@/store"
import { isTauriEnv } from "@/lib/api"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { Button } from "@/components/ui/button"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import { InstanceSelector } from "./InstanceSelector"
import { AlertsMenu } from "./AlertsMenu"

import { InstanceManagerDialog } from "@/components/instances/InstanceManagerDialog"
import { OnboardingDialog } from "@/components/onboarding/OnboardingDialog"
import { useOnboarding } from "@/hooks/useOnboarding"
import { setTopNavHeight } from "@/hooks/useVisualViewport"

export const TopBar = forwardRef<HTMLDivElement>((props, ref) => {
  const innerRef = useRef<HTMLDivElement>(null)

  // macOS Tauri overlay title bar: reserve a top strip for the traffic lights
  // (their own row, not pushing the logo right); the top bar doubles as the
  // drag region.
  const isMacTauri = isTauriEnv() && /Mac/i.test(navigator.platform || navigator.userAgent)

  // Tauri window drag — explicitly call startDragging() on mousedown instead of
  // relying on data-tauri-drag-region (which is unreliable in Tauri 2 overlay mode).
  // Only fires when clicking non-interactive areas (the drag handler checks target).
  const handleDragMouseDown = useCallback((e: React.MouseEvent) => {
    if (!isTauriEnv()) return
    const target = e.target as HTMLElement
    // Skip if clicking on interactive elements (buttons, links, inputs, etc.)
    if (target.closest("button, a, input, select, textarea, [role='button'], [role='tab']")) return
    getCurrentWindow().startDragging()
  }, [])

  // Expose the macOS title-bar (traffic-light) inset as a CSS var so full-screen
  // overlays (settings, onboarding) reserve the same top space and aren't
  // covered by the traffic lights.
  useEffect(() => {
    document.documentElement.style.setProperty("--titlebar-inset", isMacTauri ? "24px" : "0px")
  }, [isMacTauri])

  // Set the nav height in CSS variable after mount and on resize
  useEffect(() => {
    const updateNavHeight = () => {
      if (innerRef.current) {
        const height = innerRef.current.getBoundingClientRect().height
        setTopNavHeight(height)
      }
    }

    updateNavHeight()
    window.addEventListener('resize', updateNavHeight)
    return () => window.removeEventListener('resize', updateNavHeight)
  }, [])

  const { t } = useTranslation('common')
  const [instanceManagerOpen, setInstanceManagerOpen] = useState(false)
  const [onboardingOpen, setOnboardingOpen] = useState(false)

  // Onboarding status for the Rocket button badge
  const { status: onboardingStatus, dismiss: dismissOnboarding, fetchStatus: fetchOnboardingStatus } = useOnboarding()

  // Fetch onboarding status on mount
  useEffect(() => {
    fetchOnboardingStatus()
  }, [fetchOnboardingStatus])

  return (
    <TooltipProvider delayDuration={500}>
      <header
        id="app-topbar"
        ref={innerRef}
        className="relative z-20 flex shrink-0 items-center bg-[var(--chrome)] border-b border-border px-4 sm:px-6 h-12"
        style={{ paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))" }}
        onMouseDown={handleDragMouseDown}
      >
        {/* Drag region — the empty left area moves the window (macOS Tauri) */}
        <div className="flex-1 max-w-full" />

        {/* Right side: Instance + Guide + Alerts. User menu lives in the
            AppSidebar footer. */}
        <div className="ml-auto flex shrink-0 items-center gap-1.5 sm:gap-2.5">
          {/* Instance selector (identity anchor) */}
          <InstanceSelector onManageInstances={() => setInstanceManagerOpen(true)} />

          {/* Onboarding guide */}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="w-10 h-10 rounded-lg relative chrome-ghost"
                onClick={() => setOnboardingOpen(true)}
                aria-label={t('onboarding.title')}
              >
                <Rocket className="h-4 w-4" />
                {onboardingStatus && !onboardingStatus.dismissed && (
                  (!onboardingStatus.steps.llm.completed || !onboardingStatus.steps.device.completed)
                ) && (
                  <span className="absolute top-1.5 right-1.5 w-2 h-2 rounded-full bg-primary" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs px-2 py-1">
              {t('onboarding.title')}
            </TooltipContent>
          </Tooltip>

          {/* Alerts notification */}
          <AlertsMenu compact align="end" side="bottom" tooltipSide="bottom" />
        </div>
      </header>

      {/* Instance Manager Dialog */}
      <InstanceManagerDialog
        open={instanceManagerOpen}
        onOpenChange={setInstanceManagerOpen}
      />

      {/* Onboarding Dialog */}
      <OnboardingDialog
        open={onboardingOpen}
        onOpenChange={setOnboardingOpen}
        status={onboardingStatus}
        onDismiss={dismissOnboarding}
      />
    </TooltipProvider>
  )
})

TopBar.displayName = 'TopBar'

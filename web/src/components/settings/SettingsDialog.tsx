/**
 * SettingsDialog — full-bleed settings overlay (Onboarding-style chrome).
 *
 * Replaces the old /settings page so settings no longer has to match the
 * standard PageLayout of every other menu. Mirrors OnboardingDialog: opaque
 * full-bleed background, a back button (top-left, closes the dialog), and
 * centered content. The sidebar is a non-scrolling sibling of the scrolling
 * content area, so it stays put while only content scrolls.
 *
 * Open state is GLOBAL (store: settingsDialogOpen / settingsSection) so any
 * page can call openSettings(tab); the dialog is mounted once at the app
 * root. Nested dialogs opened from within the tabs (LLM editor, broker/webhook
 * config) use z-110 to stack above this z-100 overlay.
 *
 * Mobile uses a fixed bottom tab bar (PageTabsBottomNav) for section
 * switching instead of the desktop sidebar.
 */
import { useEffect, useState } from "react"
import { createPortal } from "react-dom"
import { useTranslation } from "react-i18next"
import { ArrowLeft } from "lucide-react"
import { useStore } from "@/store"
import { useIsMobile } from "@/hooks/useMobile"
import { useBodyScrollLock } from "@/hooks/useBodyScrollLock"
import { useThemeColor } from "@/hooks/useThemeColor"
import { MobileTabStrip } from "@/components/shared/MobileTabStrip"
import { AboutTab } from "@/pages/settings/AboutTab"
import { PreferencesTab } from "@/pages/settings/PreferencesTab"
import { UnifiedLLMBackendsTab } from "@/components/llm/UnifiedLLMBackendsTab"
import { UnifiedDeviceConnectionsTab } from "@/components/connections"
import { ImBridgesTab } from "@/components/im/ImBridgesTab"
import { SettingsNav, getSettingsSections } from "@/pages/settings/SettingsNav"
import type { SettingsSection } from "@/store/types"

export function SettingsDialog() {
  const { t } = useTranslation(["common", "settings", "extensions"])
  const isMobile = useIsMobile()

  const open = useStore((s) => s.settingsDialogOpen)
  const initialSection = useStore((s) => s.settingsSection)
  const closeSettings = useStore((s) => s.closeSettings)

  // LLM Backend actions from store
  const createBackend = useStore((state) => state.createBackend)
  const updateBackend = useStore((state) => state.updateBackend)
  const deleteBackend = useStore((state) => state.deleteBackend)
  const testBackend = useStore((state) => state.testBackend)

  const [activeSection, setActiveSection] = useState<SettingsSection>(initialSection)

  // Sync the PWA status-bar/safe-area color to the popover surface while the
  // dialog is open, so the notch strip doesn't read as a different color than
  // the dialog body (see useThemeColor).
  useThemeColor("popover", open)

  // Sync the active section to the store's requested section each time the
  // dialog opens (openSettings(tab) sets both atomically).
  useEffect(() => {
    if (open) setActiveSection(initialSection)
  }, [open, initialSection])

  // Esc to close + lock body scroll while open.
  useBodyScrollLock(open, { mobileOnly: true })
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault()
        closeSettings()
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [open, closeSettings])

  if (!open) return null

  const sections = getSettingsSections(t)

  const dialogRoot =
    typeof document !== "undefined"
      ? document.getElementById("dialog-root") || document.body
      : null
  if (!dialogRoot) return null

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={t("common:nav.settings")}
      className="fixed inset-0 z-[100] flex flex-col bg-popover animate-fade-in"
      style={{
        paddingTop: "calc(env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))",
      }}
    >
      <div className="flex-1 min-h-0 flex">
        <div className="w-full min-h-0 flex flex-col px-6 md:px-8 md:pt-12">
          {/* Mobile header bar — h-12 (48px) to match MobilePageHeader so the
              back button sits at the same Y as every other page's top chrome.
              The old pt-12 whitespace left it floating ~48px too low with no
              visual anchor. Full-bleed (-mx-6 px-6) so the border spans the
              overlay width. Desktop keeps pt-12; its back link is in sidebar. */}
          <div className="relative -mx-6 flex h-12 items-center border-b border-border px-4 md:hidden">
            <button
              type="button"
              onClick={closeSettings}
              className="inline-flex min-h-11 items-center gap-1 px-1 text-sm text-foreground transition-opacity active:opacity-60"
            >
              <ArrowLeft className="w-4 h-4" />
              {t("common:back", { defaultValue: "Back" })}
            </button>
            <h1 className="pointer-events-none absolute inset-x-16 truncate text-center text-base font-semibold">
              {t("common:nav.settings")}
            </h1>
          </div>

          {isMobile && (
            <MobileTabStrip
              items={sections}
              value={activeSection}
              onValueChange={setActiveSection}
              ariaLabel={t("common:nav.settings")}
              className="-mx-6 mb-4 px-4"
            />
          )}

          {/* Fixed sidebar (desktop) + independently scrolling content */}
          <div className="flex flex-1 min-h-0 gap-8">
            <SettingsNav
              sections={sections}
              activeSection={activeSection}
              onSectionChange={(s) => setActiveSection(s)}
              onBack={closeSettings}
            />
            <main className="flex-1 min-w-0 min-h-0 overflow-y-auto scrollbar-none">
              <div className="max-w-6xl mx-auto">
                {activeSection !== "about" && (
                  <h2 className="mb-5 hidden text-lg font-semibold tracking-tight sm:text-2xl md:block">
                    {sections.find((s) => s.value === activeSection)?.label}
                  </h2>
                )}
                {activeSection === "llm" && (
                  <UnifiedLLMBackendsTab
                    onCreateBackend={createBackend}
                    onUpdateBackend={updateBackend}
                    onDeleteBackend={deleteBackend}
                    onTestBackend={testBackend}
                  />
                )}
                {activeSection === "connections" && <UnifiedDeviceConnectionsTab />}
                {activeSection === "im" && <ImBridgesTab />}
                {activeSection === "preferences" && <PreferencesTab />}
                {activeSection === "about" && <AboutTab />}
              </div>
            </main>
          </div>
        </div>
      </div>

    </div>,
    dialogRoot
  )
}

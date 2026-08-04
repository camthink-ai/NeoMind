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
import { cn } from "@/lib/utils"
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
          <div className="md:hidden -mx-6 mb-4 flex h-12 items-center border-b border-border px-6">
            <button
              type="button"
              onClick={closeSettings}
              className="-ml-1 inline-flex items-center gap-1 rounded-lg px-2 py-1 text-sm text-muted-foreground hover:bg-muted-30 transition-colors"
            >
              <ArrowLeft className="w-4 h-4" />
              {t("common:back", { defaultValue: "Back" })}
            </button>
          </div>

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
                  <h2 className="text-2xl font-semibold tracking-tight mb-8">
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

      {/* Mobile: in-flow bottom tab bar for section switching (in-flow so the
          content fills above it — no gap, no overlap). */}
      {isMobile && (
        <div className="shrink-0 flex items-stretch justify-around gap-1 border-t border-border bg-bg-95 backdrop-blur-sm px-2 py-1 safe-bottom">
          {sections.map((s) => {
            const isActive = activeSection === s.value
            return (
              <button
                key={s.value}
                type="button"
                onClick={() => setActiveSection(s.value)}
                className={cn(
                  // py-2 + h-5 icon match PageTabsBottomNav so this bar is the
                  // same height (~58px + safe-bottom) as bottom tab bars on
                  // other pages. Icon size is overridden here via [&>svg] so
                  // getSettingsSections' h-4 w-4 (used by the desktop sidebar)
                  // is left untouched.
                  "flex flex-1 flex-col items-center gap-0.5 rounded-lg py-2 transition-colors",
                  isActive ? "text-primary" : "text-muted-foreground"
                )}
              >
                <span className="[&>svg]:h-5 [&>svg]:w-5">{s.icon}</span>
                <span className="text-[10px] font-medium leading-tight truncate w-full text-center">
                  {s.label}
                </span>
              </button>
            )
          })}
        </div>
      )}
    </div>,
    dialogRoot
  )
}

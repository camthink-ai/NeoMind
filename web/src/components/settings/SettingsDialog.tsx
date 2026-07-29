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
  const activeLabel = sections.find((s) => s.value === activeSection)?.label ?? ""
  const sectionDescriptions: Partial<Record<SettingsSection, string>> = {
    llm: t("settings:llmDesc", { defaultValue: "管理 LLM 后端、模型与实例" }),
    connections: t("settings:connectionsDesc", { defaultValue: "管理 MQTT / Webhook 设备连接" }),
    preferences: t("settings:preferencesDesc", { defaultValue: "语言、时间格式、数据保留等偏好" }),
  }
  const sectionDescription = sectionDescriptions[activeSection] ?? ""

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
        <div className="w-full min-h-0 flex flex-col px-6 md:px-8 pt-6">
          {/* Back button (closes the dialog) — mobile only (desktop back is in the sidebar) */}
          <button
            type="button"
            onClick={closeSettings}
            className="self-start shrink-0 -ml-1 mb-4 inline-flex items-center gap-1 rounded-lg px-2 py-1 text-sm text-muted-foreground hover:bg-muted-30 transition-colors md:hidden"
          >
            <ArrowLeft className="w-4 h-4" />
            {t("common:back", { defaultValue: "Back" })}
          </button>

          {/* Fixed sidebar (desktop) + independently scrolling content */}
          <div className="flex flex-1 min-h-0 gap-8">
            <SettingsNav
              sections={sections}
              activeSection={activeSection}
              onSectionChange={(s) => setActiveSection(s)}
              onBack={closeSettings}
            />
            <main className="flex-1 min-w-0 min-h-0 overflow-y-auto scrollbar-none">
              {/* Section header — title + one-line description (desktop; About has its own hero) */}
              {!isMobile && activeSection !== "about" && (
                <div className="mb-6">
                  <h2 className="text-2xl font-semibold">{activeLabel}</h2>
                  <p className="text-sm text-muted-foreground mt-2">{sectionDescription}</p>
                </div>
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
              {activeSection === "preferences" && <PreferencesTab />}
              {activeSection === "about" && <AboutTab />}
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
                  "flex flex-1 flex-col items-center gap-0.5 rounded-lg py-1 transition-colors",
                  isActive ? "text-primary" : "text-muted-foreground"
                )}
              >
                {s.icon}
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

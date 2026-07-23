import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useSearchParams } from "react-router-dom"
import { useStore } from "@/store"
import { useIsMobile } from "@/hooks/useMobile"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabsContent, PageTabsBottomNav } from "@/components/shared"
import { AboutTab } from "./settings/AboutTab"
import { PreferencesTab } from "./settings/PreferencesTab"
import { UnifiedLLMBackendsTab } from "@/components/llm/UnifiedLLMBackendsTab"
import { UnifiedDeviceConnectionsTab } from "@/components/connections"
import {
  SettingsNav,
  SettingsSection as SettingsSectionType,
  getSettingsSections,
} from "./settings/SettingsNav"

export function SettingsPage() {
  const { t } = useTranslation(["common", "settings", "extensions"])
  const isMobile = useIsMobile()
  const [searchParams, setSearchParams] = useSearchParams()
  const sectionFromUrl = searchParams.get("tab") as SettingsSectionType | null

  const validSections: SettingsSectionType[] = ["llm", "connections", "preferences", "about"]
  const [activeSection, setActiveSection] = useState<SettingsSectionType>(() => {
    return sectionFromUrl && validSections.includes(sectionFromUrl) ? sectionFromUrl : "preferences"
  })

  // Sync active section when the URL ?tab= param changes while already mounted
  // (e.g. clicking "Preferences" in the user menu while on /settings?tab=llm)
  useEffect(() => {
    if (sectionFromUrl && validSections.includes(sectionFromUrl) && sectionFromUrl !== activeSection) {
      setActiveSection(sectionFromUrl)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sectionFromUrl])

  // LLM Backend actions from store
  const createBackend = useStore((state) => state.createBackend)
  const updateBackend = useStore((state) => state.updateBackend)
  const deleteBackend = useStore((state) => state.deleteBackend)
  const testBackend = useStore((state) => state.testBackend)

  const sections = getSettingsSections(t)

  const handleSectionChange = (section: SettingsSectionType) => {
    setActiveSection(section)
    setSearchParams({ tab: section }, { replace: true })
  }

  // Shared tab descriptors — desktop sidebar uses sections directly, mobile
  // bottom nav uses the same set with shorter labels (mobileLabelKey not
  // needed here because section labels are already short: "LLM", "About"…).
  const mobileTabs = sections.map((s) => ({
    value: s.value,
    label: s.label,
    icon: s.icon,
  }))

  const llmEl = (
    <UnifiedLLMBackendsTab
      onCreateBackend={createBackend}
      onUpdateBackend={updateBackend}
      onDeleteBackend={deleteBackend}
      onTestBackend={testBackend}
    />
  )
  const connectionsEl = <UnifiedDeviceConnectionsTab />
  const preferencesEl = <PreferencesTab />
  const aboutEl = <AboutTab />

  return (
    <>
      <PageLayout
        title={t("settings:title")}
        borderedHeader={false}
        hasBottomNav={isMobile}
      >
        {isMobile ? (
          // Mobile: tabbed layout. Section switching happens via the bottom
          // nav (PageTabsBottomNav below). Each section's content is wrapped
          // in PageTabsContent so only the active one mounts.
          // NOTE: no extra top padding here — PageLayout's scroll container
          // already adds pt-2 on mobile, and the sticky drill-down headers
          // (LLM/MQTT/Webhook) use a ::before pseudo-element to cover exactly
          // that 8px gap. Adding pt-2 here would re-expose the gap.
          <div>
            <PageTabsContent value="llm" activeTab={activeSection}>{llmEl}</PageTabsContent>
            <PageTabsContent value="connections" activeTab={activeSection}>{connectionsEl}</PageTabsContent>
            <PageTabsContent value="preferences" activeTab={activeSection}>{preferencesEl}</PageTabsContent>
            <PageTabsContent value="about" activeTab={activeSection}>{aboutEl}</PageTabsContent>
          </div>
        ) : (
          // Desktop: sidebar + content
          <div className="flex gap-6 h-full overflow-hidden">
            <SettingsNav
              sections={sections}
              activeSection={activeSection}
              onSectionChange={handleSectionChange}
            />
            <div className="flex-1 min-w-0 overflow-y-auto pr-1">
              {activeSection === "llm" && llmEl}
              {activeSection === "connections" && connectionsEl}
              {activeSection === "preferences" && preferencesEl}
              {activeSection === "about" && aboutEl}
            </div>
          </div>
        )}
      </PageLayout>

      {/* Mobile: Bottom navigation bar — fixed at screen bottom, lets users
          switch between LLM / Connections / Preferences / About without
          drilling back through a list. Desktop is unchanged. */}
      {isMobile && (
        <PageTabsBottomNav
          tabs={mobileTabs}
          activeTab={activeSection}
          onTabChange={(v) => handleSectionChange(v as SettingsSectionType)}
        />
      )}
    </>
  )
}

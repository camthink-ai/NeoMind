import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabsBar, PageTabsContent, PageTabsBottomNav } from "@/components/shared"
import { AboutTab } from "./settings/AboutTab"
import { PreferencesTab } from "./settings/PreferencesTab"
import { UnifiedLLMBackendsTab } from "@/components/llm/UnifiedLLMBackendsTab"
import { UnifiedDeviceConnectionsTab } from "@/components/connections"
import { UnifiedAlertChannelsTab } from "@/components/alerts/UnifiedAlertChannelsTab"
import { Sliders, Info, Cpu, Plug, BellRing } from "lucide-react"

type SettingsTabValue = "llm" | "connections" | "alert-channels" | "preferences" | "about"

export function SettingsPage() {
  const { t } = useTranslation(["common", "settings", "llm", "connections", "extensions"])
  const [activeTab, setActiveTab] = useState<SettingsTabValue>("preferences")

  // LLM Backend actions from store
  const createBackend = useStore((state) => state.createBackend)
  const updateBackend = useStore((state) => state.updateBackend)
  const deleteBackend = useStore((state) => state.deleteBackend)
  const testBackend = useStore((state) => state.testBackend)

  const tabs = [
    { value: "llm" as SettingsTabValue, label: t("llmBackends", { defaultValue: "LLM Backends" }), icon: <Cpu className="h-4 w-4" /> },
    { value: "connections" as SettingsTabValue, label: t("deviceConnections", { defaultValue: "Device Connections" }), icon: <Plug className="h-4 w-4" /> },
    { value: "alert-channels" as SettingsTabValue, label: t("alertChannels", { defaultValue: "Message Channels" }), icon: <BellRing className="h-4 w-4" /> },
    { value: "preferences" as SettingsTabValue, label: t("settings:preferences"), icon: <Sliders className="h-4 w-4" /> },
    { value: "about" as SettingsTabValue, label: t("settings:about"), icon: <Info className="h-4 w-4" /> },
  ]

  return (
    <>
      <PageLayout
        title={t('settings:title')}
        subtitle={t('settings:description')}
        borderedHeader={false}
        headerContent={
          <PageTabsBar
            tabs={tabs}
            activeTab={activeTab}
            onTabChange={(v) => setActiveTab(v as SettingsTabValue)}
          />
        }
      >
        {/* LLM Backends Tab */}
        <PageTabsContent value="llm" activeTab={activeTab}>
          <UnifiedLLMBackendsTab
            onCreateBackend={createBackend}
            onUpdateBackend={updateBackend}
            onDeleteBackend={deleteBackend}
            onTestBackend={testBackend}
          />
        </PageTabsContent>

        {/* Device Connections Tab */}
        <PageTabsContent value="connections" activeTab={activeTab}>
          <UnifiedDeviceConnectionsTab />
        </PageTabsContent>

        {/* Alert Channels Tab */}
        <PageTabsContent value="alert-channels" activeTab={activeTab}>
          <UnifiedAlertChannelsTab />
        </PageTabsContent>

        {/* Preferences Tab */}
        <PageTabsContent value="preferences" activeTab={activeTab}>
          <PreferencesTab />
        </PageTabsContent>

        {/* About Tab */}
        <PageTabsContent value="about" activeTab={activeTab}>
          <AboutTab />
        </PageTabsContent>
      </PageLayout>

      {/* Mobile Bottom Navigation */}
      <PageTabsBottomNav
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as SettingsTabValue)}
      />
    </>
  )
}

import { useState } from "react"
import { useStore } from "@/store"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { LLMBackendsTab } from "@/components/llm/LLMBackendsTab"
import { ConnectionsTab } from "@/components/connections"

type PluginTabValue = "llm" | "connections"

export function PluginsPage() {
  const {
    // LLM Backend actions
    createBackend,
    updateBackend,
    deleteBackend,
    testBackend,
  } = useStore()

  const [activeTab, setActiveTab] = useState<PluginTabValue>("llm")

  const tabs = [
    { value: 'llm' as PluginTabValue, label: 'LLM 后端' },
    { value: 'connections' as PluginTabValue, label: '设备连接' },
  ]

  return (
    <PageLayout>
      <PageTabs
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => {
          setActiveTab(v as PluginTabValue)
        }}
      >
        {/* LLM Backends Tab */}
        <PageTabsContent value="llm" activeTab={activeTab}>
          <LLMBackendsTab
            onCreateBackend={createBackend}
            onUpdateBackend={updateBackend}
            onDeleteBackend={deleteBackend}
            onTestBackend={testBackend}
          />
        </PageTabsContent>

        {/* Device Connections Tab */}
        <PageTabsContent value="connections" activeTab={activeTab}>
          <ConnectionsTab />
        </PageTabsContent>
      </PageTabs>
    </PageLayout>
  )
}

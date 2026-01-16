import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { PluginUploadDialog } from "@/components/plugins"
import { UnifiedLLMBackendsTab } from "@/components/llm/UnifiedLLMBackendsTab"
import { UnifiedAlertChannelsTab } from "@/components/alerts/UnifiedAlertChannelsTab"
import { UnifiedDeviceConnectionsTab } from "@/components/connections"
import { Button } from "@/components/ui/button"
import { useToast } from "@/hooks/use-toast"
import { Upload } from "lucide-react"

type PluginTabValue = "llm" | "connections" | "alert-channels" | "extensions"

export function PluginsPage() {
  const { t } = useTranslation(["common", "plugins", "devices"])
  const { toast } = useToast()

  const {
    fetchPlugins,
    // LLM Backend actions
    createBackend,
    updateBackend,
    deleteBackend,
    testBackend,
  } = useStore()

  const [activeTab, setActiveTab] = useState<PluginTabValue>("llm")
  const [uploadDialogOpen, setUploadDialogOpen] = useState(false)

  // Fetch plugins on mount and when tab changes
  useEffect(() => {
    if (activeTab === "extensions") {
      fetchPlugins({ builtin: false })
    }
  }, [fetchPlugins, activeTab])

  const tabs = [
    { value: "llm" as PluginTabValue, label: t("plugins:llmBackends") },
    { value: "connections" as PluginTabValue, label: t("plugins:deviceConnections") },
    { value: "alert-channels" as PluginTabValue, label: t("plugins:alertChannels") },
    { value: "extensions" as PluginTabValue, label: t("plugins:extensionPlugins") },
  ]

  return (
    <PageLayout>
      <PageTabs
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as PluginTabValue)}
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

        {/* Extension Plugins Tab - External Only */}
        <PageTabsContent value="extensions" activeTab={activeTab}>
          <div className="space-y-4">
            {/* Header */}
            <div className="flex items-center justify-between mb-4">
              <div>
                <h2 className="text-2xl font-bold tracking-tight">{t("plugins:extensionPlugins")}</h2>
                <p className="text-muted-foreground text-sm">
                  {t("plugins:extensionsDesc")}
                </p>
              </div>
              <Button onClick={() => setUploadDialogOpen(true)}>
                <Upload className="mr-2 h-4 w-4" />
                {t("plugins:upload")}
              </Button>
            </div>

            {/* Info Card */}
            <div className="border rounded-lg p-6 bg-muted/30">
              <div className="flex items-start gap-4">
                <div className="p-2 rounded-lg bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400">
                  <Upload className="h-5 w-5" />
                </div>
                <div className="flex-1">
                  <h3 className="font-semibold mb-1">{t("plugins:registerPlugin")}</h3>
                  <p className="text-sm text-muted-foreground mb-3">
                    {t("plugins:registerDesc")}
                  </p>
                  <Button onClick={() => setUploadDialogOpen(true)} variant="outline" size="sm">
                    <Upload className="mr-2 h-4 w-4" />
                    {t("plugins:upload")}
                  </Button>
                </div>
              </div>
            </div>

            {/* External Plugins List */}
            <div className="border rounded-lg">
              <div className="p-4 border-b bg-muted/20">
                <h3 className="font-semibold">{t("plugins:extensionPlugins")}</h3>
              </div>
              <div className="p-8 text-center text-muted-foreground">
                <Upload className="h-12 w-12 mx-auto mb-4 opacity-50" />
                <p>{t("plugins:noPlugins")}</p>
                <p className="text-sm mt-2">{t("plugins:noPluginsDesc")}</p>
              </div>
            </div>
          </div>
        </PageTabsContent>
      </PageTabs>

      {/* Upload Plugin Dialog */}
      <PluginUploadDialog
        open={uploadDialogOpen}
        onOpenChange={(open) => {
          setUploadDialogOpen(open)
          if (!open) {
            fetchPlugins({ builtin: false })
          }
        }}
        onUploadComplete={(pluginId) => {
          toast({
            title: t("plugins:pluginLoaded", { id: pluginId }),
          })
          fetchPlugins({ builtin: false })
        }}
      />
    </PageLayout>
  )
}

import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { PluginGrid } from "@/components/plugins/PluginGrid"
import { PluginUploadDialog, AlertChannelPluginConfigDialog } from "@/components/plugins"
import { LLMBackendsTab } from "@/components/llm/LLMBackendsTab"
import { ConnectionsTab } from "@/components/connections"
import { Button } from "@/components/ui/button"
import { useToast } from "@/hooks/use-toast"
import { Upload } from "lucide-react"

type PluginTabValue = "llm" | "connections" | "alerts" | "extensions"

export function PluginsPage() {
  const { t } = useTranslation(["common", "plugins"])
  const { toast } = useToast()

  const {
    // Plugin actions
    plugins,
    pluginsLoading,
    fetchPlugins,
    enablePlugin,
    disablePlugin,
    startPlugin,
    stopPlugin,
    unregisterPlugin,
    getPluginConfig,
    getPluginStats,
    // LLM Backend actions
    createBackend,
    updateBackend,
    deleteBackend,
    testBackend,
  } = useStore()

  const [activeTab, setActiveTab] = useState<PluginTabValue>("llm")
  const [uploadDialogOpen, setUploadDialogOpen] = useState(false)

  // Alert channel config dialog state
  const [configDialogOpen, setConfigDialogOpen] = useState(false)
  const [configPluginId, setConfigPluginId] = useState("")
  const [configPluginName, setConfigPluginName] = useState("")

  // Fetch plugins on mount and when tab changes
  useEffect(() => {
    // For extensions tab, fetch only dynamic plugins (exclude built-in)
    if (activeTab === "extensions") {
      fetchPlugins({ builtin: false })
    } else {
      // For other tabs, fetch all plugins
      fetchPlugins()
    }
  }, [fetchPlugins, activeTab])

  // Plugin actions
  const handleToggle = async (id: string, enabled: boolean): Promise<boolean> => {
    const result = enabled ? await enablePlugin(id) : await disablePlugin(id)
    if (result) {
      toast({ title: enabled ? t("plugins:pluginEnabled") : t("plugins:pluginDisabled") })
    }
    return result
  }

  const handleStart = async (id: string): Promise<boolean> => {
    const result = await startPlugin(id)
    if (result) {
      toast({ title: t("plugins:pluginStarted") })
      await fetchPlugins(activeTab === "extensions" ? { builtin: false } : undefined)
    }
    return result
  }

  const handleStop = async (id: string): Promise<boolean> => {
    const result = await stopPlugin(id)
    if (result) {
      toast({ title: t("plugins:pluginStopped") })
      await fetchPlugins(activeTab === "extensions" ? { builtin: false } : undefined)
    }
    return result
  }

  const handleDelete = async (id: string): Promise<boolean> => {
    const result = await unregisterPlugin(id)
    if (result) {
      toast({ title: t("plugins:unregisterSuccess") })
      await fetchPlugins(activeTab === "extensions" ? { builtin: false } : undefined)
    }
    return result
  }

  const handleRefresh = async (id: string): Promise<boolean> => {
    await getPluginStats(id)
    await fetchPlugins(activeTab === "extensions" ? { builtin: false } : undefined)
    return true
  }

  const handleConfigure = async (id: string) => {
    // For alert channel plugins, show the alert channel config dialog
    if (id.startsWith("alert-channel-")) {
      // Find the plugin to get its name
      const plugin = plugins.find((p: { id: string }) => p.id === id)
      setConfigPluginId(id)
      setConfigPluginName(plugin?.name || t("plugins:configure"))
      setConfigDialogOpen(true)
    } else {
      // For other plugins, show generic config (placeholder for now)
      const config = await getPluginConfig(id)
      console.log("Configure plugin:", id, config)
    }
  }

  const handleViewDevices = (id: string) => {
    window.location.href = `/devices?adapter=${id}`
  }

  const tabs = [
    { value: "llm" as PluginTabValue, label: t("plugins:llmBackends") },
    { value: "connections" as PluginTabValue, label: t("plugins:deviceConnections") },
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

        {/* Extension Plugins Tab */}
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

            {/* Plugins Grid */}
            <PluginGrid
              plugins={plugins}
              loading={pluginsLoading}
              onToggle={handleToggle}
              onStart={handleStart}
              onStop={handleStop}
              onConfigure={handleConfigure}
              onDelete={handleDelete}
              onRefresh={handleRefresh}
              onViewDevices={handleViewDevices}
            />
          </div>
        </PageTabsContent>
      </PageTabs>

      {/* Upload Plugin Dialog */}
      <PluginUploadDialog
        open={uploadDialogOpen}
        onOpenChange={(open) => {
          setUploadDialogOpen(open)
          if (!open) {
            fetchPlugins(activeTab === "extensions" ? { builtin: false } : undefined)
          }
        }}
        onUploadComplete={(pluginId) => {
          toast({
            title: t("plugins:pluginLoaded", { id: pluginId }),
          })
        }}
      />

      {/* Alert Channel Plugin Config Dialog */}
      <AlertChannelPluginConfigDialog
        open={configDialogOpen}
        onOpenChange={setConfigDialogOpen}
        pluginId={configPluginId}
        pluginName={configPluginName}
      />
    </PageLayout>
  )
}

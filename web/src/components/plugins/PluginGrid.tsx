import { useMemo } from "react"
import { useTranslation } from "react-i18next"
import { PluginCard } from "./PluginCard"
import { LoadingState } from "@/components/shared"
import { Plus, Brain, Plug, Bell, Package } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import type { Plugin } from "@/types"

export interface PluginGridProps {
  plugins: Plugin[]
  loading?: boolean
  // Actions
  onToggle?: (id: string, enabled: boolean) => Promise<boolean>
  onStart?: (id: string) => Promise<boolean>
  onStop?: (id: string) => Promise<boolean>
  onConfigure?: (id: string) => void
  onDelete?: (id: string) => Promise<boolean>
  onRefresh?: (id: string) => Promise<boolean>
  onViewDevices?: (id: string) => void
  onAddPlugin?: () => void
}

export function PluginGrid({
  plugins,
  loading = false,
  onToggle,
  onStart,
  onStop,
  onConfigure,
  onDelete,
  onRefresh,
  onViewDevices,
  onAddPlugin,
}: PluginGridProps) {
  const { t } = useTranslation(["common", "plugins"])

  // Group plugins by category
  const pluginsByCategory = useMemo(() => {
    const grouped: Record<string, Plugin[]> = {
      ai: [],
      devices: [],
      notify: [],
    }

    for (const plugin of plugins) {
      const cat = plugin.category || "devices"
      if (!grouped[cat]) {
        grouped[cat] = []
      }
      grouped[cat].push(plugin)
    }

    return grouped
  }, [plugins])

  // Category info with icons (no emojis)
  const categories = useMemo(() => ({
    ai: {
      name: t("plugins:categories.ai"),
      description: t("plugins:categories.aiDesc"),
      icon: Brain,
      iconBg: "bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400",
      color: "bg-purple-100 text-purple-800 dark:bg-purple-900 dark:text-purple-300",
    },
    devices: {
      name: t("plugins:categories.devices"),
      description: t("plugins:categories.devicesDesc"),
      icon: Plug,
      iconBg: "bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400",
      color: "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-300",
    },
    notify: {
      name: t("plugins:categories.notify"),
      description: t("plugins:categories.notifyDesc"),
      icon: Bell,
      iconBg: "bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400",
      color: "bg-orange-100 text-orange-800 dark:bg-orange-900 dark:text-orange-300",
    },
  }), [t])

  // Get stats by category
  const stats = useMemo(() => {
    const result: Record<string, { total: number; running: number }> = {}
    for (const [cat, catPlugins] of Object.entries(pluginsByCategory)) {
      result[cat] = {
        total: catPlugins.length,
        running: catPlugins.filter((p) => p.state === "Running").length,
      }
    }
    return result
  }, [pluginsByCategory])

  if (loading) {
    return <LoadingState text={t("plugins:loading")} />
  }

  if (plugins.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-16 px-4">
        <div className="flex items-center justify-center w-16 h-16 rounded-full bg-muted mb-4">
          <Package className="h-8 w-8 text-muted-foreground" />
        </div>
        <h3 className="text-lg font-semibold mb-2">{t("plugins:noPlugins")}</h3>
        <p className="text-sm text-muted-foreground text-center max-w-md mb-6">
          {t("plugins:noPluginsDesc")}
        </p>
        {onAddPlugin && (
          <Button onClick={onAddPlugin}>
            <Plus className="mr-2 h-4 w-4" />
            {t("plugins:addPlugin")}
          </Button>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-6">
      {/* Toolbar */}
      <div className="flex items-center justify-end border-b pb-4">
        <div className="text-sm text-muted-foreground">
          {t("plugins:totalCount", { count: plugins.length })}
        </div>
      </div>

      {/* Render categories */}
      {(Object.keys(pluginsByCategory) as Array<"ai" | "devices" | "notify">).map((category) => {
        const catPlugins = pluginsByCategory[category]
        if (catPlugins.length === 0) return null

        const catInfo = categories[category]
        const catStats = stats[category]
        const IconComponent = catInfo.icon

        return (
          <div key={category} className="space-y-4">
            {/* Category header */}
            <div className="flex items-center gap-3">
              <div className={`flex items-center justify-center w-10 h-10 rounded-lg ${catInfo.iconBg}`}>
                <IconComponent className="h-5 w-5" />
              </div>
              <div className="flex-1">
                <h3 className="text-lg font-semibold">{catInfo.name}</h3>
                <p className="text-sm text-muted-foreground">{catInfo.description}</p>
              </div>
              <Badge className={catInfo.color} variant="secondary">
                {catStats.running}/{catStats.total} {t("plugins:running")}
              </Badge>
            </div>

            {/* Plugin cards */}
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {catPlugins.map((plugin) => (
                <PluginCard
                  key={plugin.id}
                  id={plugin.id}
                  name={plugin.name}
                  description={plugin.description}
                  type={plugin.plugin_type}
                  enabled={plugin.enabled}
                  running={plugin.state === "Running"}
                  version={plugin.version}
                  deviceCount={undefined}
                  stats={plugin.stats}
                  badge={undefined}
                  onToggle={onToggle}
                  onStart={onStart}
                  onStop={onStop}
                  onConfigure={onConfigure}
                  onDelete={onDelete}
                  onRefresh={onRefresh}
                  onViewDevices={undefined}
                />
              ))}
            </div>
          </div>
        )
      })}
    </div>
  )
}

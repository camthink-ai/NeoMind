import type { MqttStatus, ExternalBroker, HassDiscoveryStatus, HassDiscoveredDevice } from "@/types"
import { Badge } from "@/components/ui/badge"
import { StatusBadge } from "@/components/shared"
import { Server, ExternalLink, Home, Wifi, WifiOff, Sparkles, ChevronRight } from "lucide-react"

type DataSourceView = "list" | "builtin" | "external" | "hass"

interface DataSourceListProps {
  mqttStatus: MqttStatus | null
  externalBrokers: ExternalBroker[]
  hassDiscoveryStatus: HassDiscoveryStatus | null
  hassDiscoveredDevices: HassDiscoveredDevice[]
  dataSourceView: DataSourceView
  setDataSourceView: (view: DataSourceView) => void
}

export function DataSourceList({
  mqttStatus,
  externalBrokers,
  hassDiscoveryStatus,
  hassDiscoveredDevices,
  setDataSourceView,
}: DataSourceListProps) {
  return (
    <div className="py-6 space-y-6">
      {/* Cards grid - max 3 per row */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {/* Built-in Broker Card */}
        <button
          onClick={() => setDataSourceView("builtin")}
          className="text-left group border rounded-lg p-5 hover:border-primary/50 hover:bg-muted/30 transition-all"
        >
          <div className="flex flex-col h-full">
            <div className="flex items-start justify-between mb-3">
              <div className={`p-3 rounded-lg ${mqttStatus?.connected ? "badge-success" : "badge-error"}`}>
                <Server className="h-6 w-6" />
              </div>
              <ChevronRight className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
            </div>
            <div className="flex-1">
              <h3 className="text-lg font-semibold flex items-center gap-2 mb-2">
                内置 MQTT Broker
                {mqttStatus?.connected ? (
                  <Badge variant="outline" className="text-green-600 border-green-600">
                    <Wifi className="h-3 w-3 mr-1" />
                    运行中
                  </Badge>
                ) : (
                  <Badge variant="outline" className="text-red-600 border-red-600">
                    <WifiOff className="h-3 w-3 mr-1" />
                    未连接
                  </Badge>
                )}
              </h3>
              <p className="text-sm text-muted-foreground">
                {mqttStatus ? `${mqttStatus.server_ip}:${mqttStatus.listen_port || 1883}` : "加载中..."} · {mqttStatus?.devices_count || 0} 个设备
              </p>
            </div>
          </div>
        </button>

        {/* External Brokers Card */}
        <button
          onClick={() => setDataSourceView("external")}
          className="text-left group border rounded-lg p-5 hover:border-primary/50 hover:bg-muted/30 transition-all"
        >
          <div className="flex flex-col h-full">
            <div className="flex items-start justify-between mb-3">
              <div className="p-3 rounded-lg badge-info">
                <ExternalLink className="h-6 w-6" />
              </div>
              <ChevronRight className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
            </div>
            <div className="flex-1">
              <h3 className="text-lg font-semibold flex items-center gap-2 mb-2">
                外部 MQTT Broker
                <Badge variant="secondary">{externalBrokers.length} 个</Badge>
              </h3>
              <p className="text-sm text-muted-foreground">
                {externalBrokers.filter(b => b.enabled && b.connected).length > 0
                  ? `${externalBrokers.filter(b => b.enabled && b.connected).length} 个已连接`
                  : "已配置的外部数据源"}
              </p>
            </div>
          </div>
        </button>

        {/* HASS Devices Card */}
        <button
          onClick={() => setDataSourceView("hass")}
          className="text-left group border rounded-lg p-5 hover:border-primary/50 hover:bg-muted/30 transition-all"
        >
          <div className="flex flex-col h-full">
            <div className="flex items-start justify-between mb-3">
              <div className={`p-3 rounded-lg ${hassDiscoveryStatus?.hass_discovery?.enabled ? "bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400" : "bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400"}`}>
                <Home className="h-6 w-6" />
              </div>
              <ChevronRight className="h-5 w-5 text-muted-foreground group-hover:text-foreground transition-colors" />
            </div>
            <div className="flex-1">
              <h3 className="text-lg font-semibold flex items-center gap-2 mb-2">
                HASS 设备发现
                {hassDiscoveryStatus?.hass_discovery?.enabled ? (
                  <Badge variant="outline" className="text-purple-600 border-purple-600">
                    <Sparkles className="h-3 w-3 mr-1" />
                    运行中
                  </Badge>
                ) : (
                  <Badge variant="secondary">未启动</Badge>
                )}
              </h3>
              <p className="text-sm text-muted-foreground">
                {hassDiscoveryStatus?.hass_discovery?.enabled
                  ? `已发现 ${hassDiscoveredDevices.length} 个设备`
                  : "Home Assistant MQTT 设备发现"}
              </p>
            </div>
          </div>
        </button>
      </div>
    </div>
  )
}

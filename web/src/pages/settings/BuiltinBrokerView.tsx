import type { MqttStatus } from "@/types"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { SubPageHeader } from "@/components/layout"
import { Server, RefreshCw } from "lucide-react"

interface BuiltinBrokerViewProps {
  mqttStatus: MqttStatus | null
  isRefreshingStatus: boolean
  onRefreshStatus: () => void
  onBack: () => void
}

export function BuiltinBrokerView({
  mqttStatus,
  isRefreshingStatus,
  onRefreshStatus,
  onBack,
}: BuiltinBrokerViewProps) {
  return (
    <div className="py-6 space-y-6">
      <SubPageHeader
        title="内置 MQTT Broker"
        description="系统内置的 MQTT Broker 状态信息"
        icon={<Server className="h-6 w-6" />}
        onBack={onBack}
      />

      {/* Status Card */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <CardTitle>连接状态</CardTitle>
            <Button
              variant="outline"
              size="sm"
              onClick={onRefreshStatus}
              disabled={isRefreshingStatus}
            >
              <RefreshCw className={`h-4 w-4 mr-2 ${isRefreshingStatus ? "animate-spin" : ""}`} />
              刷新
            </Button>
          </div>
        </CardHeader>
        <CardContent>

        {mqttStatus ? (
          <div className="space-y-4">
            <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4">
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">状态</p>
                <div className="flex items-center gap-2">
                  <div className={`h-2 w-2 rounded-full ${mqttStatus.connected ? "bg-green-500" : "bg-red-500"}`} />
                  <span className="text-sm font-medium">{mqttStatus.connected ? "运行中" : "未连接"}</span>
                </div>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">访问地址</p>
                <p className="text-sm font-medium font-mono">{mqttStatus.server_ip}</p>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">监听端口</p>
                <p className="text-sm font-medium">{mqttStatus.listen_port}</p>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">客户端数量</p>
                <p className="text-sm font-medium">{mqttStatus.clients_count}</p>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">设备数量</p>
                <p className="text-sm font-medium">{mqttStatus.devices_count}</p>
              </div>
            </div>

            {/* External brokers connected */}
            {mqttStatus.external_brokers && mqttStatus.external_brokers.length > 0 && (
              <div className="border-t pt-4 mt-4">
                <h4 className="text-sm font-medium mb-3">已订阅的外部 Broker</h4>
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                  {mqttStatus.external_brokers.map((broker) => (
                    <Card key={broker.id} className="p-3 space-y-2">
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <div className={`h-2 w-2 rounded-full ${broker.connected ? "bg-green-500" : "bg-red-500"}`} />
                          <span className="font-medium text-sm">{broker.name}</span>
                        </div>
                        {!broker.enabled && (
                          <Badge variant="secondary" className="text-xs">已禁用</Badge>
                        )}
                      </div>
                      <p className="text-xs text-muted-foreground font-mono">
                        {broker.tls ? "mqtts" : "mqtt"}://{broker.broker}:{broker.port}
                      </p>
                      {!broker.connected && broker.last_error && (
                        <p className="text-xs text-destructive">{broker.last_error}</p>
                      )}
                    </Card>
                  ))}
                </div>
              </div>
            )}
          </div>
        ) : (
          <div className="text-sm text-muted-foreground">正在加载状态...</div>
        )}
      </CardContent>
    </Card>
    </div>
  )
}

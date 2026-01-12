import type { ExternalBroker } from "@/types"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { ExternalLink, Plus, Power, RefreshCw, Trash2, Wifi } from "lucide-react"

interface ExternalBrokersViewProps {
  externalBrokers: ExternalBroker[]
  testingBrokerId: string | null
  onBack: () => void
  onAddBroker: () => void
  onEditBroker: (broker: ExternalBroker) => void
  onToggleBroker: (broker: ExternalBroker) => void
  onTestBroker: (id: string) => void
  onDeleteBroker: (id: string) => void
}

export function ExternalBrokersView({
  externalBrokers,
  testingBrokerId,
  onBack,
  onAddBroker,
  onEditBroker,
  onToggleBroker,
  onTestBroker,
  onDeleteBroker,
}: ExternalBrokersViewProps) {
  const handleTestBroker = async (id: string) => {
    await onTestBroker(id)
  }

  return (
    <div className="py-6 space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-4">
          <Button variant="ghost" size="sm" onClick={onBack} className="gap-1">
            返回
          </Button>
          <div>
            <h2 className="text-2xl font-bold flex items-center gap-2">
              <ExternalLink className="h-6 w-6" />
              外部 MQTT Broker
            </h2>
            <p className="text-sm text-muted-foreground">管理外部 MQTT Broker 连接</p>
          </div>
        </div>
        <Button onClick={onAddBroker} size="sm">
          <Plus className="h-4 w-4 mr-2" />
          添加 Broker
        </Button>
      </div>

      {/* Broker List */}
      {externalBrokers.length === 0 ? (
        <div className="text-center py-12 border rounded-lg bg-muted/30">
          <ExternalLink className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
          <p className="text-muted-foreground">暂无外部 Broker</p>
          <p className="text-xs text-muted-foreground mt-2">点击上方按钮添加外部数据源</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {externalBrokers.map((broker) => (
            <div key={broker.id} className="border rounded-lg p-4 space-y-3">
              <div className="flex items-start justify-between">
                <div className="space-y-1 flex-1">
                  <div className="flex items-center gap-2 flex-wrap">
                    <h4 className="font-medium">{broker.name || "未命名"}</h4>
                    <Badge variant={broker.enabled ? "default" : "secondary"} className="text-xs">
                      {broker.enabled ? "已启用" : "已禁用"}
                    </Badge>
                    {broker.tls && (
                      <Badge variant="outline" className="text-blue-600 text-xs">
                        🔒 TLS
                      </Badge>
                    )}
                    {broker.connected && (
                      <Badge variant="outline" className="text-green-600 text-xs">
                        <Wifi className="h-3 w-3 mr-1" />
                        已连接
                      </Badge>
                    )}
                  </div>
                  <code className="text-xs bg-muted px-2 py-1 rounded block">
                    {broker.tls ? "mqtts" : "mqtt"}://{broker.broker}:{broker.port}
                  </code>
                </div>
              </div>
              {broker.username && (
                <div className="text-xs text-muted-foreground">
                  用户名: {broker.username}
                </div>
              )}
              {broker.last_error && (
                <div className="text-xs text-destructive">
                  错误: {broker.last_error}
                </div>
              )}
              <div className="flex items-center gap-1 pt-2 border-t">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onEditBroker(broker)}
                  className="h-8 px-2 text-xs"
                >
                  编辑
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onToggleBroker(broker)}
                  className="h-8 px-2"
                >
                  <Power className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => handleTestBroker(broker.id)}
                  disabled={testingBrokerId === broker.id}
                  className="h-8 px-2"
                >
                  <RefreshCw className={`h-4 w-4 ${testingBrokerId === broker.id ? "animate-spin" : ""}`} />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => onDeleteBroker(broker.id)}
                  className="h-8 px-2 text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Deduplication Info */}
      <div className="p-4 border rounded-lg bg-muted/50">
        <h4 className="font-medium text-sm mb-2">关于数据去重</h4>
        <p className="text-xs text-muted-foreground">
          当多个 Broker 订阅相同的 Topic 时，系统会根据数据的时间戳自动去重，
          保留最早到达的数据。这确保了即使从多个数据源接收到相同数据，也只处理一次。
        </p>
      </div>
    </div>
  )
}

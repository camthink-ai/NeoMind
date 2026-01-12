import { useState } from "react"
import type { HassDiscoveryStatus, HassDiscoveredDevice } from "@/types"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Home, RefreshCw, Radar, Square, Trash2 } from "lucide-react"
import { toast } from "@/components/ui/use-toast"

interface HassDiscoveryViewProps {
  hassDiscoveryStatus: HassDiscoveryStatus | null
  hassDiscoveredDevices: HassDiscoveredDevice[]
  onBack: () => void
  fetchHassDiscoveryStatus: () => Promise<void>
  fetchHassDiscoveredDevices: () => Promise<void>
  startHassDiscovery: (req: { auto_register: boolean }) => Promise<{ message: string }>
  stopHassDiscovery: () => Promise<{ success: boolean; message: string }>
  clearHassDiscoveredDevices: () => void
  registerHassDevice: (deviceId: string) => Promise<boolean>
  stopHassDiscoveryPolling: () => void
  fetchDevices: () => Promise<void>
  deleteDevice: (id: string) => Promise<boolean>
}

export function HassDiscoveryView({
  hassDiscoveryStatus,
  hassDiscoveredDevices,
  onBack,
  fetchHassDiscoveryStatus,
  fetchHassDiscoveredDevices,
  startHassDiscovery,
  stopHassDiscovery,
  clearHassDiscoveredDevices,
  registerHassDevice,
  stopHassDiscoveryPolling,
  fetchDevices,
  deleteDevice,
}: HassDiscoveryViewProps) {
  const [isStartingHassDiscovery, setIsStartingHassDiscovery] = useState(false)
  const [isStoppingHassDiscovery, setIsStoppingHassDiscovery] = useState(false)
  const [hassMessage, setHassMessage] = useState("")

  const handleBack = () => {
    onBack()
    stopHassDiscoveryPolling()
  }

  const handleClearDevices = async () => {
    clearHassDiscoveredDevices()
    await fetchHassDiscoveredDevices()
  }

  const handleStopDiscovery = async () => {
    setIsStoppingHassDiscovery(true)
    try {
      const result = await stopHassDiscovery()
      setHassMessage(result.message || "HASS 发现已停止")
    } catch (e) {
      setHassMessage(`停止失败: ${(e as Error).message}`)
    } finally {
      setIsStoppingHassDiscovery(false)
      await fetchHassDiscoveryStatus()
    }
  }

  const handleStartDiscovery = async () => {
    setIsStartingHassDiscovery(true)
    try {
      const result = await startHassDiscovery({ auto_register: false })
      setHassMessage(result.message || "HASS 发现已启动")
    } catch (e) {
      setHassMessage(`启动失败: ${(e as Error).message}`)
    } finally {
      setIsStartingHassDiscovery(false)
      await fetchHassDiscoveryStatus()
    }
  }

  const handleRegisterDevice = async (device: HassDiscoveredDevice) => {
    const result = await registerHassDevice(device.device_id)
    if (result) {
      await fetchHassDiscoveredDevices()
      await fetchDevices()
    }
  }

  const handleUnregisterDevice = async (device: HassDiscoveredDevice) => {
    if (confirm(`确定要取消注册设备 "${device.name || device.device_id}" 吗？`)) {
      const result = await deleteDevice(device.device_id)
      if (result) {
        toast({
          title: "取消注册成功",
          description: `设备 "${device.name || device.device_id}" 已取消注册`,
        })
        await fetchHassDiscoveredDevices()
        await fetchDevices()
      }
    }
  }

  return (
    <div className="py-6 space-y-6">
      {/* Header */}
      <div className="flex items-center gap-4">
        <Button variant="ghost" size="sm" onClick={handleBack} className="gap-1">
          返回
        </Button>
        <div>
          <h2 className="text-2xl font-bold flex items-center gap-2">
            <Home className="h-6 w-6" />
            HASS 设备发现
          </h2>
          <p className="text-sm text-muted-foreground">Home Assistant MQTT 设备自动发现</p>
        </div>
      </div>

      {/* Status Card */}
      <div className="border rounded-lg p-6">
        <div className="flex items-center justify-between mb-6">
          <h3 className="text-lg font-semibold">发现状态</h3>
          <Button
            variant="outline"
            size="sm"
            onClick={async () => {
              await fetchHassDiscoveryStatus()
              await fetchHassDiscoveredDevices()
            }}
          >
            <RefreshCw className="h-4 w-4 mr-2" />
            刷新
          </Button>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <div className="space-y-1">
            <p className="text-xs text-muted-foreground">订阅主题</p>
            <p className="text-sm font-medium font-mono">homeassistant/+/config</p>
          </div>
          <div className="space-y-1">
            <p className="text-xs text-muted-foreground">运行状态</p>
            <p className="text-sm font-medium">
              {hassDiscoveryStatus?.hass_discovery?.enabled ? (
                <span className="text-green-600">运行中</span>
              ) : (
                <span className="text-muted-foreground">未启动</span>
              )}
            </p>
          </div>
          <div className="space-y-1">
            <p className="text-xs text-muted-foreground">已发现设备</p>
            <p className="text-sm font-medium">{hassDiscoveredDevices.length} 个</p>
          </div>
          <div className="space-y-1">
            <p className="text-xs text-muted-foreground">支持组件</p>
            <p className="text-sm font-medium">{hassDiscoveryStatus?.component_count || 0} 种</p>
          </div>
        </div>

        {/* Start/Stop Discovery */}
        <div className="mt-6 pt-6 border-t flex gap-3">
          {hassDiscoveryStatus?.hass_discovery?.enabled ? (
            <>
              <div className="flex-1">
                <div className="p-3 rounded-md bg-green-50 dark:bg-green-950/20 text-sm text-green-700 dark:text-green-400">
                  ✓ HASS 发现正在运行，等待设备发布配置消息...
                </div>
              </div>
              <Button variant="outline" onClick={handleClearDevices}>
                清空设备
              </Button>
              <Button
                variant="destructive"
                onClick={handleStopDiscovery}
                disabled={isStoppingHassDiscovery}
              >
                {isStoppingHassDiscovery ? (
                  <>
                    <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                    停止中...
                  </>
                ) : (
                  <>
                    <Square className="mr-2 h-4 w-4" />
                    停止发现
                  </>
                )}
              </Button>
            </>
          ) : (
            <Button onClick={handleStartDiscovery} disabled={isStartingHassDiscovery}>
              {isStartingHassDiscovery ? (
                <>
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                  启动中...
                </>
              ) : (
                <>
                  <Radar className="mr-2 h-4 w-4" />
                  启动发现
                </>
              )}
            </Button>
          )}
        </div>

        {hassMessage && (
          <div className={`mt-4 p-3 rounded-md text-sm ${
            hassMessage.includes("失败") || hassMessage.includes("错误")
              ? "bg-destructive/10 text-destructive"
              : "bg-green-50 dark:bg-green-950/20 text-green-700 dark:text-green-400"
          }`}>
            {hassMessage}
          </div>
        )}
      </div>

      {/* Discovered Devices */}
      <div className="border rounded-lg p-6">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-lg font-semibold">已发现的设备</h3>
        </div>

        {hassDiscoveredDevices.length === 0 ? (
          <div className="text-center py-12 text-muted-foreground">
            <Home className="h-12 w-12 mx-auto mb-4 opacity-50" />
            <p>暂无已发现的设备</p>
            <p className="text-xs mt-2">
              {hassDiscoveryStatus?.hass_discovery?.enabled
                ? "等待 HASS 设备发布配置消息..."
                : "启动 HASS 发现以自动检测设备"}
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {hassDiscoveredDevices.map((device) => (
              <div key={device.device_id} className="border rounded-lg p-4 space-y-3">
                <div className="flex items-start justify-between">
                  <div className="flex-1">
                    <h4 className="font-medium">{device.name || device.device_id}</h4>
                    <p className="text-xs text-muted-foreground font-mono">{device.device_id}</p>
                    <p className="text-xs text-muted-foreground mt-1">{device.description}</p>
                  </div>
                  <Badge
                    variant={device.already_registered ? "default" : "outline"}
                    className="text-xs"
                  >
                    {device.entity_count} 实体
                  </Badge>
                </div>

                {/* Entities list */}
                {device.entities && device.entities.length > 0 && (
                  <div className="space-y-1">
                    <p className="text-xs text-muted-foreground">包含实体:</p>
                    <div className="flex flex-wrap gap-1">
                      {device.entities.slice(0, 4).map((entity) => (
                        <Badge key={entity.entity_id} variant="secondary" className="text-xs">
                          {entity.name || entity.component}
                        </Badge>
                      ))}
                      {device.entities.length > 4 && (
                        <Badge variant="secondary" className="text-xs">
                          +{device.entities.length - 4}
                        </Badge>
                      )}
                    </div>
                  </div>
                )}

                <div className="space-y-1 text-xs text-muted-foreground">
                  <div className="flex justify-between">
                    <span>总指标:</span>
                    <span>{device.total_metrics}</span>
                  </div>
                  <div className="flex justify-between">
                    <span>总命令:</span>
                    <span>{device.total_commands}</span>
                  </div>
                </div>

                <div className="pt-2 border-t flex items-center justify-between gap-2">
                  {device.already_registered ? (
                    <>
                      <Badge variant="default" className="text-xs">已注册</Badge>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="text-xs h-7 text-red-500 hover:text-red-600"
                        onClick={() => handleUnregisterDevice(device)}
                      >
                        <Trash2 className="h-3 w-3 mr-1" />
                        取消注册
                      </Button>
                    </>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="text-xs h-7 w-full"
                      onClick={() => handleRegisterDevice(device)}
                    >
                      注册设备
                    </Button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Info Card */}
      <div className="p-4 border rounded-lg bg-muted/50">
        <h4 className="font-medium text-sm mb-2">关于 HASS MQTT 发现</h4>
        <p className="text-xs text-muted-foreground mb-2">
          Home Assistant 设备通过 MQTT 发布配置消息来声明自己的存在。
          支持的设备包括 Tasmota、Shelly、ESPHome 等 HASS 生态系统设备。
        </p>
        <p className="text-xs text-muted-foreground">
          设备会发布配置到 <code>homeassistant/&lt;component&gt;/&lt;object_id&gt;/config</code> 主题。
          启动发现后，系统会自动监听这些主题并解析设备信息。
        </p>
      </div>
    </div>
  )
}

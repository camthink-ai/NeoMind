import { useState } from 'react'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Radar, Square, RefreshCw } from 'lucide-react'
import { cn } from '@/lib/utils'
import { toast } from '@/components/ui/use-toast'

interface HassDiscoveryDialogProps {
  open: boolean
  onClose: () => void
  hassDiscoveryStatus: any
  hassDiscoveredDevices: any[]
  hassDiscovering: boolean
  onStartDiscovery: () => Promise<void>
  onStopDiscovery: () => Promise<void>
  onRefresh: () => Promise<void>
  onRegisterDevice: (device: any) => Promise<boolean>
  onClearDevices: () => void
}

export function HassDiscoveryDialog({
  open,
  onClose,
  hassDiscoveryStatus,
  hassDiscoveredDevices,
  hassDiscovering,
  onStartDiscovery,
  onStopDiscovery,
  onRefresh,
  onRegisterDevice,
  onClearDevices,
}: HassDiscoveryDialogProps) {
  const [message, setMessage] = useState('')

  const handleStart = async () => {
    setMessage('')
    try {
      await onStartDiscovery()
      // 立即刷新状态
      await onRefresh()
      setMessage('发现已启动，等待设备发布配置消息...')
    } catch (e) {
      setMessage('启动失败: ' + (e as Error).message)
    }
  }

  const handleStop = async () => {
    setMessage('')
    try {
      await onStopDiscovery()
      // 立即刷新状态
      await onRefresh()
      setMessage('发现已停止')
    } catch (e) {
      setMessage('停止失败: ' + (e as Error).message)
    }
  }

  const handleRefresh = async () => {
    setMessage('')
    try {
      await onRefresh()
      setTimeout(() => setMessage(''), 2000)
    } catch (e) {
      setMessage('刷新失败: ' + (e as Error).message)
    }
  }

  const handleRegister = async (device: any) => {
    const success = await onRegisterDevice(device)
    if (success) {
      toast({
        title: "注册成功",
        description: '设备 "' + (device.name || device.device_id) + '" 已注册',
      })
      await onRefresh()
    }
  }

  const handleClear = () => {
    onClearDevices()
    setMessage('已清空设备列表')
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-3xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Radar className="h-5 w-5" />
            HASS 设备发现
          </DialogTitle>
          <DialogDescription>
            通过 MQTT 发现支持 Home Assistant 协议的设备（Tasmota、Shelly、ESPHome 等）
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="border rounded-lg p-4">
            <div className="flex items-center justify-between mb-4">
              <h4 className="font-semibold">发现状态</h4>
              <Button variant="outline" size="sm" onClick={handleRefresh}>
                <RefreshCw className="h-4 w-4 mr-2" />
                刷新
              </Button>
            </div>

            <div className="grid grid-cols-3 gap-4 mb-4">
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

            <div className="flex gap-2">
              {hassDiscoveryStatus?.hass_discovery?.enabled ? (
                <>
                  <Button variant="outline" onClick={handleClear}>
                    清空设备
                  </Button>
                  <Button variant="destructive" onClick={handleStop} disabled={hassDiscovering}>
                    <Square className="h-4 w-4 mr-2" />
                    停止发现
                  </Button>
                </>
              ) : (
                <Button onClick={handleStart} disabled={hassDiscovering}>
                  <Radar className="h-4 w-4 mr-2" />
                  启动发现
                </Button>
              )}
            </div>

            {message && (
              <div className={cn(
                "mt-3 p-3 rounded-md text-sm",
                message.includes('失败') || message.includes('错误')
                  ? "bg-destructive/10 text-destructive"
                  : "bg-green-50 dark:bg-green-950/20 text-green-700 dark:text-green-400"
              )}>
                {message}
              </div>
            )}
          </div>

          <div className="border rounded-lg p-4">
            <h4 className="font-semibold mb-3">已发现的设备</h4>

            {hassDiscoveredDevices.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                <Radar className="h-10 w-10 mx-auto mb-3 opacity-50" />
                <p>暂无已发现的设备</p>
                <p className="text-xs mt-2">
                  {hassDiscoveryStatus?.hass_discovery?.enabled
                    ? "等待 HASS 设备发布配置消息..."
                    : "启动 HASS 发现以自动检测设备"}
                </p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-3 max-h-[400px] overflow-y-auto">
                {hassDiscoveredDevices.map((device) => (
                  <div key={device.device_id} className="border rounded-lg p-3 space-y-2">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <h5 className="font-medium">{device.name || device.device_id}</h5>
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

                    {device.entities && device.entities.length > 0 && (
                      <div className="flex flex-wrap gap-1">
                        {device.entities.slice(0, 4).map((entity: any) => (
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
                    )}

                    <div className="flex items-center gap-4 text-xs text-muted-foreground">
                      <span>指标: {device.total_metrics}</span>
                      <span>命令: {device.total_commands}</span>
                    </div>

                    <div className="pt-2 border-t flex items-center justify-between">
                      {device.already_registered ? (
                        <Badge variant="default" className="text-xs">已注册</Badge>
                      ) : (
                        <Button
                          size="sm"
                          variant="outline"
                          className="text-xs h-7"
                          onClick={() => handleRegister(device)}
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

          <div className="p-3 border rounded-lg bg-muted/50 text-sm text-muted-foreground">
            <p>• 确保设备已连接到同一 MQTT Broker</p>
            <p>• 设备会自动发布配置到 homeassistant/+/config 主题</p>
            <p>• 支持 Tasmota、Shelly、ESPHome 等 HASS 生态设备</p>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>关闭</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

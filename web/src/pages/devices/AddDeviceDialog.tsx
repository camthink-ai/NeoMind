import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { toast } from "@/components/ui/use-toast"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { RefreshCw } from "lucide-react"
import type { DeviceType, AddDeviceRequest, ConnectionConfig } from "@/types"

function generateRandomId(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
  let result = ''
  for (let i = 0; i < 10; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  return result
}

interface AddDeviceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceTypes: DeviceType[]
  onAdd: (request: AddDeviceRequest) => Promise<boolean>
  adding: boolean
}

export function AddDeviceDialog({
  open,
  onOpenChange,
  deviceTypes,
  onAdd,
  adding,
}: AddDeviceDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [selectedDeviceType, setSelectedDeviceType] = useState("")
  const [deviceId, setDeviceId] = useState("")
  const [deviceName, setDeviceName] = useState("")
  const [adapterType, setAdapterType] = useState<"mqtt" | "http" | "webhook">("mqtt")
  const [connectionConfig, setConnectionConfig] = useState<ConnectionConfig>({})

  useEffect(() => {
    if (open && !deviceId) {
      setDeviceId(generateRandomId())
    }
  }, [open])

  useEffect(() => {
    // Set defaults based on adapter type
    if (adapterType === 'mqtt' && selectedDeviceType && deviceId) {
      const defaultTopic = `device/${selectedDeviceType}/${deviceId}/uplink`
      const defaultCommandTopic = `device/${selectedDeviceType}/${deviceId}/downlink`
      setConnectionConfig({
        telemetry_topic: defaultTopic,
        command_topic: defaultCommandTopic,
      })
    } else if (adapterType === 'http') {
      setConnectionConfig({
        url: `http://192.168.1.100/api/telemetry`,
        method: 'GET',
        poll_interval: 30,
      })
    } else {
      setConnectionConfig({})
    }
   
  }, [adapterType, selectedDeviceType, deviceId])

  const handleAdd = async () => {
    if (!selectedDeviceType) return

    const request: AddDeviceRequest = {
      device_id: deviceId || undefined,
      name: deviceName || deviceId || selectedDeviceType,
      device_type: selectedDeviceType,
      adapter_type: adapterType,
      connection_config: connectionConfig,
    }

    const success = await onAdd(request)
    if (success) {
      setSelectedDeviceType("")
      setDeviceId(generateRandomId())
      setDeviceName("")
      setAdapterType("mqtt")
      setConnectionConfig({})
      onOpenChange(false)
      toast({
        title: t('devices:add.success'),
        description: deviceId ? t('devices:add.successWithId', { deviceId }) : t('devices:add.successGeneric'),
      })
    } else {
      toast({
        title: t('devices:add.error'),
        description: t('devices:add.retryMessage'),
        variant: "destructive",
      })
    }
  }

  const handleOpenChange = (open: boolean) => {
    if (open) {
      setDeviceId(generateRandomId())
      setConnectionConfig({})
    } else {
      setSelectedDeviceType("")
      setDeviceId("")
      setDeviceName("")
      setAdapterType("mqtt")
      setConnectionConfig({})
    }
    onOpenChange(open)
  }

  const selectedTemplate = deviceTypes.find(t => t.device_type === selectedDeviceType)
  const hasCommands = (selectedTemplate?.commands?.length || 0) > 0

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t('devices:add.title')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Device Type */}
          <div className="space-y-2">
            <Label htmlFor="device-type">
              {t('devices:deviceType')} <span className="text-destructive">*</span>
            </Label>
            <Select value={selectedDeviceType} onValueChange={setSelectedDeviceType}>
              <SelectTrigger id="device-type">
                <SelectValue placeholder={t('devices:add.typePlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {deviceTypes.map((type) => (
                  <SelectItem key={type.device_type} value={type.device_type}>
                    {type.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Device ID & Name */}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="device-id">{t('devices:deviceId')}</Label>
              <div className="flex gap-2">
                <Input
                  id="device-id"
                  value={deviceId}
                  onChange={(e) => setDeviceId(e.target.value)}
                  placeholder={t('devices:id.autoGenerate')}
                  className="font-mono"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={() => setDeviceId(generateRandomId())}
                  title={t('devices:id.regenerate')}
                >
                  <RefreshCw className="h-4 w-4" />
                </Button>
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="device-name">{t('devices:deviceName')}</Label>
              <Input
                id="device-name"
                value={deviceName}
                onChange={(e) => setDeviceName(e.target.value)}
                placeholder={t('common:optional')}
              />
            </div>
          </div>

          {/* Adapter Type */}
          <div className="space-y-2">
            <Label htmlFor="adapter-type">{t('devices:add.adapterType')}</Label>
            <Select
              value={adapterType}
              onValueChange={(v) => setAdapterType(v as "mqtt" | "http" | "webhook")}
            >
              <SelectTrigger id="adapter-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="mqtt">MQTT</SelectItem>
                <SelectItem value="http">HTTP</SelectItem>
                <SelectItem value="webhook">Webhook</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Adapter Config */}
          {adapterType === 'mqtt' && (
            <div className="space-y-3">
              <div className="space-y-2">
                <Label htmlFor="telemetry-topic">{t('devices:add.telemetryTopic')}</Label>
                <Input
                  id="telemetry-topic"
                  value={connectionConfig.telemetry_topic || ''}
                  onChange={(e) => setConnectionConfig({ ...connectionConfig, telemetry_topic: e.target.value })}
                  placeholder="device/{type}/{id}/uplink"
                  className="font-mono text-sm"
                />
              </div>
              {hasCommands && (
                <div className="space-y-2">
                  <Label htmlFor="command-topic">{t('devices:add.commandTopic')}</Label>
                  <Input
                    id="command-topic"
                    value={connectionConfig.command_topic || ''}
                    onChange={(e) => setConnectionConfig({ ...connectionConfig, command_topic: e.target.value })}
                    placeholder="device/{type}/{id}/downlink"
                    className="font-mono text-sm"
                  />
                </div>
              )}
            </div>
          )}

          {adapterType === 'http' && (
            <div className="space-y-3">
              <div className="space-y-2">
                <Label htmlFor="http-url">{t('devices:add.httpUrl')}</Label>
                <Input
                  id="http-url"
                  value={connectionConfig.url || ''}
                  onChange={(e) => setConnectionConfig({ ...connectionConfig, url: e.target.value })}
                  placeholder="http://192.168.1.100/api/telemetry"
                  className="font-mono text-sm"
                />
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label htmlFor="http-method">{t('devices:add.requestMethod')}</Label>
                  <Select
                    value={connectionConfig.method || 'GET'}
                    onValueChange={(v) => setConnectionConfig({ ...connectionConfig, method: v })}
                  >
                    <SelectTrigger id="http-method">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="GET">GET</SelectItem>
                      <SelectItem value="POST">POST</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-2">
                  <Label htmlFor="poll-interval">{t('devices:add.pollInterval')}</Label>
                  <Input
                    id="poll-interval"
                    type="number"
                    min="1"
                    value={connectionConfig.poll_interval || 30}
                    onChange={(e) => setConnectionConfig({ ...connectionConfig, poll_interval: parseInt(e.target.value) || 30 })}
                  />
                </div>
              </div>
            </div>
          )}

          {adapterType === 'webhook' && (
            <div className="rounded-lg border bg-muted p-4">
              <p className="text-sm text-muted-foreground mb-2">
                {t('devices:add.webhookUrlDescription')}
              </p>
              <code className="text-xs break-all block">
                {window.location.origin}/api/devices/webhook/{deviceId}
              </code>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:cancel')}
          </Button>
          <Button onClick={handleAdd} disabled={!selectedDeviceType || adding}>
            {adding ? t('devices:adding') : t('common:add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

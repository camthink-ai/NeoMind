import { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { toast } from "@/components/ui/use-toast"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { RefreshCw, Plus, KeyRound } from "lucide-react"
import type { DeviceType, AddDeviceRequest, ConnectionConfig } from "@/types"
import { FormSection, FormSectionGroup } from "@/components/ui/form-section"
import { FormField } from "@/components/ui/field"
import { useServerUrl } from "@/lib/server-url"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"

function generateRandomId(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
  let result = ''
  for (let i = 0; i < 10; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  return result
}

function generateWebhookToken(): string {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789'
  let result = 'whk_'
  for (let i = 0; i < 32; i++) {
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
  const serverUrl = useServerUrl()

  const [selectedDeviceType, setSelectedDeviceType] = useState("")
  const [deviceId, setDeviceId] = useState("")
  const [deviceName, setDeviceName] = useState("")
  const [adapterType, setAdapterType] = useState<"mqtt" | "webhook">("mqtt")
  const [connectionConfig, setConnectionConfig] = useState<ConnectionConfig>({})
  const [webhookToken, setWebhookToken] = useState("")
  const [errors, setErrors] = useState<Record<string, string>>({})

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
    } else {
      setConnectionConfig({})
    }
    if (adapterType !== 'webhook') {
      setWebhookToken("")
    }
  }, [adapterType, selectedDeviceType, deviceId])

  const handleAdd = async () => {
    // Validate required fields
    const newErrors: Record<string, string> = {}
    if (!selectedDeviceType) {
      newErrors.deviceType = t('devices:deviceType') + ' is required'
    }
    setErrors(newErrors)
    if (Object.keys(newErrors).length > 0) return

    const request: AddDeviceRequest = {
      device_id: deviceId || undefined,
      name: deviceName || deviceId || selectedDeviceType,
      device_type: selectedDeviceType,
      adapter_type: adapterType,
      connection_config: {
        ...connectionConfig,
        ...(adapterType === 'webhook' && webhookToken ? { webhook_token: webhookToken } : {}),
      },
    }

    const success = await onAdd(request)
    if (success) {
      setSelectedDeviceType("")
      setDeviceId(generateRandomId())
      setDeviceName("")
      setAdapterType("mqtt")
      setConnectionConfig({})
      setWebhookToken("")
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

  const handleClose = useCallback(() => {
    if (!adding) {
      setSelectedDeviceType("")
      setDeviceId("")
      setDeviceName("")
      setAdapterType("mqtt")
      setConnectionConfig({})
      setWebhookToken("")
      setErrors({})
      onOpenChange(false)
    }
  }, [adding, onOpenChange])

  const selectedTemplate = deviceTypes.find(t => t.device_type === selectedDeviceType)
  const _hasCommands = (selectedTemplate?.commands?.length || 0) > 0

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={(newOpen) => { if (!newOpen && !adding) handleClose() }}
      title={t('devices:add.title')}
      icon={<Plus className="h-5 w-5 text-primary" />}
      width="sm"
      onSubmit={handleAdd}
      isSubmitting={adding}
      submitLabel={t('common:add')}
      submitDisabled={!selectedDeviceType}
    >
      <FormSectionGroup>
        {/* Device Type */}
        <FormField
          label={t('devices:deviceType')}
          required
          error={errors.deviceType}
        >
          <Select value={selectedDeviceType} onValueChange={(v) => { setSelectedDeviceType(v); setErrors(prev => { const next = { ...prev }; delete next.deviceType; return next }) }}>
            <SelectTrigger>
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
        </FormField>

        {/* Device ID & Name */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <FormField label={t('devices:deviceId')}>
            <div className="flex gap-2">
              <Input
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
          </FormField>
          <FormField label={t('devices:deviceName')}>
            <Input
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={t('common:optional')}
            />
          </FormField>
        </div>

        {/* Adapter Type */}
        <FormField label={t('devices:add.adapterType')}>
          <Select
            value={adapterType}
            onValueChange={(v) => setAdapterType(v as "mqtt" | "webhook")}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="mqtt">MQTT</SelectItem>
              <SelectItem value="webhook">Webhook</SelectItem>
            </SelectContent>
          </Select>
        </FormField>

        {/* Adapter Config */}
        {adapterType === 'mqtt' && (
          <FormSection
            title={t('devices:add.mqttConfig', { defaultValue: 'MQTT Configuration' })}
            collapsible
            defaultExpanded
          >
            <div className="space-y-3">
              <FormField label={t('devices:add.telemetryTopic')}>
                <Input
                  value={connectionConfig.telemetry_topic || ''}
                  onChange={(e) => setConnectionConfig({ ...connectionConfig, telemetry_topic: e.target.value })}
                  placeholder="device/{type}/{id}/uplink"
                  className="font-mono text-sm"
                />
              </FormField>
              <FormField
                label={t('devices:add.commandTopic')}
                helpText={t('devices:add.commandTopicHint', {
                  defaultValue: 'Topic the device subscribes to for commands. Leave blank if the device has no downlink.',
                })}
              >
                <Input
                  value={connectionConfig.command_topic || ''}
                  onChange={(e) => setConnectionConfig({ ...connectionConfig, command_topic: e.target.value })}
                  placeholder="device/{type}/{id}/downlink"
                  className="font-mono text-sm"
                />
              </FormField>
            </div>
          </FormSection>
        )}

        {adapterType === 'webhook' && (
          <FormSection
            title={t('devices:add.webhookConfig', { defaultValue: 'Webhook Configuration' })}
          >
            <div className="space-y-4">
              {/* Webhook URL preview */}
              <div className="rounded-lg border bg-muted p-4">
                <p className="text-sm text-muted-foreground mb-2">
                  {t('devices:add.webhookUrlDescription')}
                </p>
                <code className="text-xs break-all block">
                  {serverUrl}/api/devices/{deviceId}/webhook
                </code>
              </div>

              {/* Webhook Token */}
              <FormField
                label={t('devices:add.webhookToken')}
                error={undefined}
              >
                <div className="flex gap-2">
                  <Input
                    value={webhookToken}
                    onChange={(e) => setWebhookToken(e.target.value)}
                    placeholder={t('devices:add.webhookTokenPlaceholder')}
                    className="font-mono text-sm"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={() => setWebhookToken(generateWebhookToken())}
                    title={t('devices:add.webhookTokenGenerate')}
                  >
                    <KeyRound className="h-4 w-4" />
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {t('devices:add.webhookTokenDesc')}
                </p>
              </FormField>
            </div>
          </FormSection>
        )}

      </FormSectionGroup>
    </UnifiedFormDialog>
  )
}

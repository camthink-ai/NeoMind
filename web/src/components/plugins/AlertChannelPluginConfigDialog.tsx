import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Terminal, Database, Webhook, Mail, Check, X, Plus, Trash2, TestTube } from "lucide-react"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import type { PluginConfigSchema } from "@/types"

const CHANNEL_TYPE_INFO: Record<string, { icon: React.ReactNode; color: string; name: string; nameZh: string }> = {
  console: {
    icon: <Terminal className="h-6 w-6" />,
    color: "text-blue-500",
    name: "Console",
    nameZh: "控制台",
  },
  memory: {
    icon: <Database className="h-6 w-6" />,
    color: "text-purple-500",
    name: "Memory",
    nameZh: "内存",
  },
  webhook: {
    icon: <Webhook className="h-6 w-6" />,
    color: "text-green-500",
    name: "Webhook",
    nameZh: "Webhook",
  },
  email: {
    icon: <Mail className="h-6 w-6" />,
    color: "text-orange-500",
    name: "Email",
    nameZh: "邮件",
  },
}

interface AlertChannel {
  name: string
  channel_type: string
  enabled: boolean
}

interface AlertChannelPluginConfigDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  pluginId: string
  pluginName: string
}

// Convert JsonSchema to PluginConfigSchema for ConfigFormBuilder
function convertToPluginConfigSchema(jsonSchema: any): PluginConfigSchema {
  const properties: Record<string, any> = {}

  for (const [key, prop] of Object.entries(jsonSchema.properties || {})) {
    const typedProp = prop as any
    properties[key] = {
      type: typedProp.type || 'string',
      description: typedProp.description || typedProp.description_zh,
      default: typedProp.default,
      enum: typedProp.enum,
      minimum: typedProp.minimum,
      maximum: typedProp.maximum,
      secret: typedProp.x_secret || false,
    }
  }

  return {
    type: 'object',
    properties,
    required: jsonSchema.required || [],
    ui_hints: jsonSchema.ui_hints || {},
  }
}

export function AlertChannelPluginConfigDialog({
  open,
  onOpenChange,
  pluginId,
  pluginName,
}: AlertChannelPluginConfigDialogProps) {
  const { t } = useTranslation(['common', 'alerts', 'plugins'])
  const { toast } = useToast()

  const [loading, setLoading] = useState(false)
  const [channels, setChannels] = useState<AlertChannel[]>([])
  const [channelSchema, setChannelSchema] = useState<any>(null)
  const [newChannelName, setNewChannelName] = useState("")
  const [showCreateForm, setShowCreateForm] = useState(false)
  const [testingChannel, setTestingChannel] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Extract channel type from plugin ID (e.g., "alert-channel-webhook" -> "webhook")
  const channelType = pluginId.replace("alert-channel-", "")

  const typeInfo = CHANNEL_TYPE_INFO[channelType]

  // Fetch channels and schema when dialog opens
  useEffect(() => {
    if (open) {
      fetchChannels()
      fetchChannelSchema()
    }
  }, [open, channelType])

  const fetchChannels = async () => {
    try {
      const response = await api.listAlertChannels()
      // Filter channels by this type
      const typeChannels = response.channels.filter((ch: AlertChannel) => ch.channel_type === channelType)
      setChannels(typeChannels)
    } catch (error) {
      console.error("Failed to fetch channels:", error)
    }
  }

  const fetchChannelSchema = async () => {
    try {
      const schema = await api.getChannelSchema(channelType)
      setChannelSchema(schema)
    } catch (error) {
      console.error("Failed to fetch channel schema:", error)
    }
  }

  const handleCreateChannel = async (values: Record<string, unknown>) => {
    if (!newChannelName.trim()) {
      toast({ title: t('common:failed'), description: "Missing channel name", variant: "destructive" })
      return
    }

    setLoading(true)
    try {
      const config = {
        name: newChannelName,
        channel_type: channelType,
        ...values,
      }
      await api.createAlertChannel(config as any)
      toast({ title: t('common:success'), description: t('alerts:channelCreated') })
      setNewChannelName("")
      setShowCreateForm(false)
      await fetchChannels()
    } catch (error) {
      toast({ title: t('common:failed'), description: String(error), variant: "destructive" })
    } finally {
      setLoading(false)
    }
  }

  const handleDeleteChannel = async (name: string) => {
    const confirmed = await confirm({
      title: t('common:delete'),
      description: `Delete channel "${name}"?`,
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    setLoading(true)
    try {
      await api.deleteAlertChannel(name)
      toast({ title: t('common:success'), description: t('alerts:channelDeleted') })
      await fetchChannels()
    } catch (error) {
      toast({ title: t('common:failed'), description: String(error), variant: "destructive" })
    } finally {
      setLoading(false)
    }
  }

  const handleTestChannel = async (name: string) => {
    setTestingChannel(name)
    try {
      const result = await api.testAlertChannel(name)
      setTestResults((prev) => ({
        ...prev,
        [name]: { success: result.success, message: result.message },
      }))
      if (result.success) {
        toast({ title: t('common:success'), description: result.message })
      } else {
        toast({ title: t('common:failed'), description: result.message, variant: "destructive" })
      }
    } catch (error) {
      const message = String(error)
      setTestResults((prev) => ({
        ...prev,
        [name]: { success: false, message },
      }))
      toast({ title: t('common:failed'), description: message, variant: "destructive" })
    } finally {
      setTestingChannel(null)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span className={typeInfo?.color}>{typeInfo?.icon}</span>
            {pluginName}
          </DialogTitle>
          <DialogDescription>
            {t('alerts:channelsDesc')}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          {/* Existing Channels */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h3 className="font-medium">{t('alerts:activeChannels')}</h3>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setShowCreateForm(!showCreateForm)}
              >
                <Plus className="mr-2 h-4 w-4" />
                {t('alerts:addChannel')}
              </Button>
            </div>

            {channels.length === 0 ? (
              <div className="text-center py-8 border rounded-lg bg-muted/30">
                <p className="text-sm text-muted-foreground">
                  {t('alerts:noChannels')}
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                {channels.map((channel) => {
                  const testResult = testResults[channel.name]
                  return (
                    <div
                      key={channel.name}
                      className="flex items-center justify-between p-3 border rounded-lg bg-background"
                    >
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{channel.name}</span>
                          <Badge variant={channel.enabled ? "default" : "secondary"}>
                            {channel.enabled ? t('alerts:enabled') : t('alerts:disabled')}
                          </Badge>
                        </div>
                        {testResult && (
                          <div className={`text-xs mt-1 ${testResult.success ? 'text-green-500' : 'text-red-500'}`}>
                            {testResult.success ? <Check className="inline h-3 w-3 mr-1" /> : <X className="inline h-3 w-3 mr-1" />}
                            {testResult.message}
                          </div>
                        )}
                      </div>
                      <div className="flex items-center gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleTestChannel(channel.name)}
                          disabled={testingChannel === channel.name}
                        >
                          <TestTube className="h-4 w-4 mr-1" />
                          {testingChannel === channel.name ? t('common:testing') : t('plugins:test')}
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="text-destructive hover:text-destructive"
                          onClick={() => handleDeleteChannel(channel.name)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  )
                })}
              </div>
            )}
          </div>

          {/* Create New Channel Form */}
          {showCreateForm && channelSchema && (
            <div className="border-t pt-4 space-y-4">
              <div>
                <Label htmlFor="channel-name">{t('alerts:channelName')}</Label>
                <Input
                  id="channel-name"
                  value={newChannelName}
                  onChange={(e) => setNewChannelName(e.target.value)}
                  placeholder={t('alerts:channelNamePlaceholder')}
                  disabled={loading}
                />
              </div>

              <ConfigFormBuilder
                schema={convertToPluginConfigSchema(channelSchema.config_schema)}
                onSubmit={handleCreateChannel}
                loading={loading}
                submitLabel={t('common:create')}
              />
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

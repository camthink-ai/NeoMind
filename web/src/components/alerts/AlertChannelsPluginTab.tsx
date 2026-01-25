import { useEffect, useState, useRef } from "react"
import { useTranslation } from "react-i18next"
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Switch } from "@/components/ui/switch"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Plus, Trash2, TestTube, Check, X, Terminal, Database, Webhook, Mail, Settings } from "lucide-react"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import type { AlertChannel, ChannelTypeInfo, ChannelStats, PluginConfigSchema } from "@/types"

const CHANNEL_TYPE_INFO: Record<string, { icon: React.ReactNode; color: string; name: string; nameZh: string; description: string; descriptionZh: string }> = {
  console: {
    icon: <Terminal className="h-6 w-6" />,
    color: "text-blue-500",
    name: "Console",
    nameZh: "控制台",
    description: "Print alerts to console output",
    descriptionZh: "将告警打印到控制台输出",
  },
  memory: {
    icon: <Database className="h-6 w-6" />,
    color: "text-purple-500",
    name: "Memory",
    nameZh: "内存",
    description: "Store alerts in memory for testing",
    descriptionZh: "将告警存储在内存中用于测试",
  },
  webhook: {
    icon: <Webhook className="h-6 w-6" />,
    color: "text-green-500",
    name: "Webhook",
    nameZh: "Webhook",
    description: "Send alerts via HTTP POST webhook",
    descriptionZh: "通过 HTTP POST Webhook 发送告警",
  },
  email: {
    icon: <Mail className="h-6 w-6" />,
    color: "text-orange-500",
    name: "Email",
    nameZh: "邮件",
    description: "Send alerts via email SMTP",
    descriptionZh: "通过 SMTP 邮件发送告警",
  },
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

export function AlertChannelsPluginTab() {
  const { t } = useTranslation(['common', 'alerts', 'plugins'])
  const { toast } = useToast()

  // Data state
  const [channels, setChannels] = useState<AlertChannel[]>([])
  const [stats, setStats] = useState<ChannelStats | null>(null)
  const [channelTypes, setChannelTypes] = useState<ChannelTypeInfo[]>([])
  const [loading, setLoading] = useState(false)

  // Dialog state
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [selectedChannelType, setSelectedChannelType] = useState<string | null>(null)
  const [channelSchema, setChannelSchema] = useState<any>(null)
  const [newChannelName, setNewChannelName] = useState("")

  // Testing state
  const [testingChannel, setTestingChannel] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Fetch channels on mount
  const hasFetched = useRef(false)
  useEffect(() => {
    if (!hasFetched.current) {
      hasFetched.current = true
      fetchChannels()
      fetchChannelTypes()
    }
  }, [])

  const fetchChannels = async () => {
    setLoading(true)
    try {
      const response = await api.listAlertChannels()
      setChannels(response.channels)
      setStats(response.stats)
    } catch (error) {
      console.error("Failed to fetch channels:", error)
    } finally {
      setLoading(false)
    }
  }

  const fetchChannelTypes = async () => {
    try {
      const response = await api.listChannelTypes()
      setChannelTypes(response.types)
    } catch (error) {
      console.error("Failed to fetch channel types:", error)
    }
  }

  const handleCreateChannel = async (values: Record<string, unknown>) => {
    if (!selectedChannelType || !newChannelName.trim()) {
      toast({ title: t('common:failed'), description: "Missing channel name or type", variant: "destructive" })
      return
    }

    setLoading(true)
    try {
      const config = {
        name: newChannelName,
        channel_type: selectedChannelType,
        ...values,
      }
      await api.createAlertChannel(config as any)
      toast({ title: t('common:success'), description: t('alerts:channelCreated') })
      setCreateDialogOpen(false)
      setNewChannelName("")
      setSelectedChannelType(null)
      setChannelSchema(null)
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

  const handleChannelTypeSelect = async (typeId: string) => {
    setSelectedChannelType(typeId)
    try {
      const schema = await api.getChannelSchema(typeId)
      setChannelSchema(schema)
    } catch (error) {
      toast({ title: t('common:failed'), description: "Failed to load channel schema", variant: "destructive" })
      setSelectedChannelType(null)
      setChannelSchema(null)
    }
  }

  const closeCreateDialog = () => {
    setCreateDialogOpen(false)
    setNewChannelName("")
    setSelectedChannelType(null)
    setChannelSchema(null)
    setTestResults({})
  }

  // Render channel card
  const renderChannelCard = (channel: AlertChannel) => {
    const typeInfo = CHANNEL_TYPE_INFO[channel.channel_type]
    const testResult = testResults[channel.name]

    return (
      <Card key={channel.name} className="relative group">
        <CardHeader className="pb-3">
          <div className="flex items-start justify-between">
            <div className="flex items-center gap-3">
              <div className={`p-3 rounded-lg bg-muted ${typeInfo?.color || ''}`}>
                {typeInfo?.icon || <Settings className="h-6 w-6" />}
              </div>
              <div>
                <CardTitle className="text-lg">{channel.name}</CardTitle>
                <CardDescription className="text-xs">
                  {typeInfo?.name || channel.channel_type}
                </CardDescription>
              </div>
            </div>
            <Switch
              checked={channel.enabled}
              disabled
              className="opacity-50"
            />
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <p className="text-sm text-muted-foreground">
            {typeInfo?.description || ''}
          </p>

          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              className="flex-1"
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

          {testResult && (
            <div className={`text-xs p-2 rounded ${testResult.success ? 'bg-green-500/10 text-green-500' : 'bg-red-500/10 text-red-500'}`}>
              {testResult.success ? <Check className="inline h-3 w-3 mr-1" /> : <X className="inline h-3 w-3 mr-1" />}
              {testResult.message}
            </div>
          )}
        </CardContent>
      </Card>
    )
  }

  // Render channel type card (for adding new channel)
  const renderTypeCard = (type: ChannelTypeInfo) => {
    const typeInfo = CHANNEL_TYPE_INFO[type.id]
    const hasChannel = channels.some((ch) => ch.channel_type === type.id)

    return (
      <Card
        key={type.id}
        className="hover:shadow-md transition-shadow cursor-pointer border-dashed"
        onClick={() => !hasChannel && handleChannelTypeSelect(type.id)}
      >
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-3">
              <div className={`p-3 rounded-lg bg-muted ${typeInfo?.color || ''}`}>
                {typeInfo?.icon || <Settings className="h-6 w-6" />}
              </div>
              <div>
                <CardTitle className="text-lg">{type.name}</CardTitle>
                <CardDescription className="text-xs">
                  {type.name_zh}
                </CardDescription>
              </div>
            </div>
            {hasChannel && (
              <Badge variant="secondary">{channels.filter((ch) => ch.channel_type === type.id).length}</Badge>
            )}
          </div>
        </CardHeader>
        <CardContent>
          <p className="text-sm text-muted-foreground mb-4">
            {type.description}
          </p>
          {!hasChannel && (
            <Button
              variant="outline"
              size="sm"
              className="w-full"
              onClick={(e) => {
                e.stopPropagation()
                handleChannelTypeSelect(type.id)
              }}
            >
              <Plus className="h-4 w-4 mr-1" />
              {t('plugins:add')}
            </Button>
          )}
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="space-y-6">
      {/* Stats Overview */}
      {stats && (
        <div className="grid grid-cols-4 gap-4">
          <Card className="p-4">
            <div className="text-2xl font-bold">{stats.total}</div>
            <div className="text-sm text-muted-foreground">{t('alerts:totalChannels')}</div>
          </Card>
          <Card className="p-4">
            <div className="text-2xl font-bold text-green-500">{stats.enabled}</div>
            <div className="text-sm text-muted-foreground">{t('alerts:enabledChannels')}</div>
          </Card>
          <Card className="p-4">
            <div className="text-2xl font-bold text-muted-foreground">{stats.disabled}</div>
            <div className="text-sm text-muted-foreground">{t('alerts:disabledChannels')}</div>
          </Card>
          <Card className="p-4">
            <div className="text-2xl font-bold">{Object.keys(stats.by_type || {}).length}</div>
            <div className="text-sm text-muted-foreground">{t('alerts:channelTypes')}</div>
          </Card>
        </div>
      )}

      {/* Active Channels */}
      <div>
        <div className="flex items-center justify-between mb-4">
          <div>
            <h2 className="text-2xl font-bold tracking-tight">{t('alerts:activeChannels', { defaultValue: 'Active Channels' })}</h2>
            <p className="text-muted-foreground text-sm">
              {t('alerts:activeChannelsDesc', { defaultValue: 'Currently configured alert notification channels' })}
            </p>
          </div>
          <Button onClick={() => setCreateDialogOpen(true)} disabled={loading}>
            <Plus className="mr-2 h-4 w-4" />
            {t('alerts:addChannel')}
          </Button>
        </div>

        {channels.length === 0 ? (
          <Card className="p-8 text-center">
            <p className="text-muted-foreground">{t('alerts:noChannels', { defaultValue: 'No channels configured' })}</p>
          </Card>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {channels.map(renderChannelCard)}
          </div>
        )}
      </div>

      {/* Available Channel Types */}
      <div>
        <div className="mb-4">
          <h2 className="text-2xl font-bold tracking-tight">{t('alerts:availableChannelTypes', { defaultValue: 'Available Channel Types' })}</h2>
          <p className="text-muted-foreground text-sm">
            {t('alerts:availableChannelTypesDesc', { defaultValue: 'Add a new notification channel by clicking on a channel type' })}
          </p>
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          {channelTypes.map(renderTypeCard)}
        </div>
      </div>

      {/* Create Channel Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={(open) => !open && closeCreateDialog()}>
        <DialogContent className="max-w-lg max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{t('alerts:addChannel')}</DialogTitle>
            <DialogDescription>
              {t('alerts:addChannelDesc')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4">
            {/* Channel Name */}
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

            {/* Channel Type Selection */}
            <div>
              <Label htmlFor="channel-type">{t('alerts:channelType')}</Label>
              <Select value={selectedChannelType || ""} onValueChange={handleChannelTypeSelect} disabled={loading}>
                <SelectTrigger id="channel-type">
                  <SelectValue placeholder={t('alerts:selectChannelType')} />
                </SelectTrigger>
                <SelectContent>
                  {channelTypes.map((type) => (
                    <SelectItem key={type.id} value={type.id}>
                      <div className="flex items-center gap-2">
                        <span className={CHANNEL_TYPE_INFO[type.id]?.color}>
                          {CHANNEL_TYPE_INFO[type.id]?.icon}
                        </span>
                        <span>{type.name}</span>
                        <span className="text-muted-foreground text-xs ml-2">- {type.description}</span>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Dynamic Configuration Form */}
            {selectedChannelType && channelSchema && (
              <div className="space-y-4 border-t pt-4">
                <div className="flex items-center gap-2">
                  <span className={CHANNEL_TYPE_INFO[selectedChannelType]?.color}>
                    {CHANNEL_TYPE_INFO[selectedChannelType]?.icon}
                  </span>
                  <div>
                    <h4 className="font-medium">{channelSchema.name}</h4>
                    <p className="text-sm text-muted-foreground">{channelSchema.description}</p>
                  </div>
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
            <Button variant="outline" onClick={closeCreateDialog} disabled={loading}>
              {t('common:cancel')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

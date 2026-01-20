import { useEffect, useState, useRef } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Card } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { Plus, Trash2, TestTube, Check, X, Terminal, Database, Webhook, Mail, Loader2, Bell } from "lucide-react"
import { EmptyState, EmptyStateInline, ActionBar, LoadingState } from "@/components/shared"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { ConfigFormBuilder } from "@/components/plugins/ConfigFormBuilder"
import type { AlertChannel, ChannelTypeInfo, ChannelStats, PluginConfigSchema } from "@/types"

const CHANNEL_ICONS: Record<string, React.ReactNode> = {
  console: <Terminal className="h-4 w-4" />,
  memory: <Database className="h-4 w-4" />,
  webhook: <Webhook className="h-4 w-4" />,
  email: <Mail className="h-4 w-4" />,
}

const CHANNEL_COLORS: Record<string, string> = {
  console: "bg-blue-500/10 text-blue-500",
  memory: "bg-purple-500/10 text-purple-500",
  webhook: "bg-green-500/10 text-green-500",
  email: "bg-orange-500/10 text-orange-500",
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

export function AlertChannelsTab() {
  const { t } = useTranslation(['common', 'alerts'])
  const { toast } = useToast()

  // Data state
  const [channels, setChannels] = useState<AlertChannel[]>([])
  const [stats, setStats] = useState<ChannelStats | null>(null)
  const [channelTypes, setChannelTypes] = useState<ChannelTypeInfo[]>([])
  const [loading, setLoading] = useState(true)  // Start with true for initial loading
  const [initialLoading, setInitialLoading] = useState(true)  // Track initial load separately

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
      toast({ title: t('common:failed'), description: String(error), variant: "destructive" })
    } finally {
      setLoading(false)
      setInitialLoading(false)  // Clear initial loading flag
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
      // Merge name and channel_type with the config values
      const config = {
        name: newChannelName,
        channel_type: selectedChannelType,
        ...values,
      }
      await api.createAlertChannel(config as any)
      toast({ title: t('common:success'), description: "Channel created successfully" })
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
    if (!confirm(`Delete channel "${name}"?`)) return

    setLoading(true)
    try {
      await api.deleteAlertChannel(name)
      toast({ title: t('common:success'), description: "Channel deleted successfully" })
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

  const selectedType = channelTypes.find((t) => t.id === selectedChannelType)

  // Initial loading state
  if (initialLoading) {
    return (
      <>
        <ActionBar
          title={t('alerts:channels')}
          titleIcon={<Bell className="h-5 w-5" />}
          description={t('alerts:channelsDesc')}
          onRefresh={fetchChannels}
          refreshLoading={loading}
        />
        <LoadingState variant="page" text={t('common:loading')} />
      </>
    )
  }

  // Empty state when no channel types are available
  if (channelTypes.length === 0 && !loading) {
    return (
      <>
        <ActionBar
          title={t('alerts:channels')}
          titleIcon={<Bell className="h-5 w-5" />}
          description={t('alerts:channelsDesc')}
          onRefresh={fetchChannels}
          refreshLoading={loading}
        />
        <EmptyState
          icon="alert"
          title={t('alerts:noChannelTypes')}
          description={t('alerts:noChannelTypesDesc')}
          action={{ label: t('common:retry'), onClick: () => { fetchChannels(); fetchChannelTypes(); }, icon: <Loader2 className="h-4 w-4" /> }}
        />
      </>
    )
  }

  return (
    <>
      {/* Header */}
      <ActionBar
        title={t('alerts:channels')}
        titleIcon={<Bell className="h-5 w-5" />}
        description={t('alerts:channelsDesc')}
        actions={[
          {
            label: t('alerts:addChannel'),
            icon: <Plus className="h-4 w-4" />,
            onClick: () => setCreateDialogOpen(true),
          },
        ]}
        onRefresh={fetchChannels}
        refreshLoading={loading}
      />

      {/* Stats Cards */}
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

      {/* Channels Table */}
      {loading ? (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('alerts:channelName')}</TableHead>
                <TableHead>{t('alerts:channelType')}</TableHead>
                <TableHead>{t('common:status')}</TableHead>
                <TableHead>{t('common:actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <EmptyStateInline title={t('common:loading')} colSpan={4} />
            </TableBody>
          </Table>
        </Card>
      ) : channels.length === 0 ? (
        // Full-page empty state when no channels - consistent with LLM Backends and Device Connections
        <EmptyState
          icon="alert"
          title={t('alerts:noChannels')}
          description={t('alerts:noChannelsDesc')}
          action={{
            label: t('alerts:addChannel'),
            onClick: () => setCreateDialogOpen(true),
            icon: <Plus className="h-4 w-4" />,
          }}
        />
      ) : (
        <Card>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('alerts:channelName')}</TableHead>
                <TableHead>{t('alerts:channelType')}</TableHead>
                <TableHead>{t('common:status')}</TableHead>
                <TableHead>{t('common:actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {channels.map((channel) => {
                const testResult = testResults[channel.name]
                return (
                  <TableRow key={channel.name}>
                    <TableCell className="font-medium">{channel.name}</TableCell>
                    <TableCell>
                      <Badge className={CHANNEL_COLORS[channel.channel_type] || ""}>
                        <span className="mr-1">{CHANNEL_ICONS[channel.channel_type]}</span>
                        {channel.channel_type}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      {channel.enabled ? (
                        <Badge variant="default" className="bg-green-500">
                          <Check className="mr-1 h-3 w-3" />
                          {t('alerts:enabled')}
                        </Badge>
                      ) : (
                        <Badge variant="outline">
                          <X className="mr-1 h-3 w-3" />
                          {t('alerts:disabled')}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleTestChannel(channel.name)}
                          disabled={testingChannel === channel.name}
                        >
                          <TestTube className="h-4 w-4 mr-1" />
                          {testingChannel === channel.name ? t('common:testing') : t('alerts:testChannel')}
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleDeleteChannel(channel.name)}
                          className="h-8 w-8 text-destructive hover:text-destructive"
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                      {testResult && (
                        <div className={`text-xs mt-1 ${testResult.success ? 'text-green-500' : 'text-red-500'}`}>
                          {testResult.message}
                        </div>
                      )}
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        </Card>
      )}

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
                        <span>{CHANNEL_ICONS[type.id]}</span>
                        <span>{type.name}</span>
                        <span className="text-muted-foreground text-xs">- {type.description}</span>
                      </div>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Dynamic Configuration Form */}
            {selectedType && channelSchema && (
              <div className="space-y-4 border-t pt-4">
                <div className="flex items-center gap-2">
                  <span className={CHANNEL_COLORS[selectedType.id]}>
                    {CHANNEL_ICONS[selectedType.id]}
                  </span>
                  <div>
                    <h4 className="font-medium">{selectedType.name}</h4>
                    <p className="text-sm text-muted-foreground">{selectedType.description}</p>
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
    </>
  )
}

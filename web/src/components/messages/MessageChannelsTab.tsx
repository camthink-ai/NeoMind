// MessageChannelsTab Component
// Displays and manages message notification channels

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { EmptyState } from '@/components/shared/EmptyState'
import { LoadingState } from '@/components/shared/LoadingState'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Plus, Trash2, TestTube, Check, X, RefreshCw } from 'lucide-react'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import type {
  MessageChannel,
  ChannelTypeInfo,
  ChannelStats,
  CreateMessageChannelRequest,
} from '@/types'

interface MessageChannelsTabProps {
  channels: MessageChannel[]
  channelTypes: ChannelTypeInfo[]
  stats: ChannelStats | null
  isLoading: boolean
  error: Error | null
  onCreateChannel: (req: CreateMessageChannelRequest) => Promise<void>
  onDeleteChannel: (name: string) => Promise<void>
  onTestChannel: (name: string) => Promise<{ success: boolean; message?: string }>
  onRefresh: () => Promise<void>
}

// Channel type icons and colors
const CHANNEL_CONFIG: Record<string, { icon: string; color: string }> = {
  console: { icon: 'Terminal', color: 'bg-gray-500' },
  memory: { icon: 'Database', color: 'bg-blue-500' },
  webhook: { icon: 'Webhook', color: 'bg-green-500' },
  email: { icon: 'Mail', color: 'bg-purple-500' },
}

export function MessageChannelsTab({
  channels,
  channelTypes,
  stats,
  isLoading,
  error,
  onCreateChannel,
  onDeleteChannel,
  onTestChannel,
  onRefresh,
}: MessageChannelsTabProps) {
  const { t } = useTranslation()
  const { handleError } = useErrorHandler()
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [newChannelName, setNewChannelName] = useState('')
  const [newChannelType, setNewChannelType] = useState('console')
  const [newChannelConfig, setNewChannelConfig] = useState<Record<string, unknown>>({})
  const [testingChannel, setTestingChannel] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message?: string }>>({})

  // Get selected channel type schema
  const selectedChannelType = channelTypes.find(ct => ct.id === newChannelType)

  // Handle create channel
  const handleCreateChannel = async () => {
    if (!newChannelName.trim()) return

    try {
      await onCreateChannel({
        name: newChannelName.trim(),
        channel_type: newChannelType,
        ...newChannelConfig,
      })
      setCreateDialogOpen(false)
      setNewChannelName('')
      setNewChannelType('console')
      setNewChannelConfig({})
    } catch (err) {
      handleError(err, { operation: 'Create channel', showToast: false })
    }
  }

  // Handle test channel
  const handleTestChannel = async (name: string) => {
    setTestingChannel(name)
    try {
      const result = await onTestChannel(name)
      setTestResults(prev => ({ ...prev, [name]: result }))
    } catch (err) {
      setTestResults(prev => ({
        ...prev,
        [name]: { success: false, message: String(err) },
      }))
    } finally {
      setTestingChannel(null)
    }
  }

  // Handle delete channel
  const handleDeleteChannel = async (name: string) => {
    try {
      await onDeleteChannel(name)
      setTestResults(prev => {
        const next = { ...prev }
        delete next[name]
        return next
      })
    } catch (err) {
      handleError(err, { operation: 'Delete channel', showToast: false })
    }
  }

  // Build create dialog fields based on channel type schema
  const renderConfigFields = () => {
    if (!selectedChannelType) return null

    return (
      <div className="space-y-4">
        <Label>{t('messages.channels.config')}</Label>
        {selectedChannelType.id === 'webhook' && (
          <>
            <div>
              <Label htmlFor="webhook-url" className="flex items-center gap-2">
                {t('messages.channels.webhookUrl')}
                <a
                  href="https://webhook.site"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-xs text-blue-500 hover:underline"
                >
                  (Get free test URL →)
                </a>
              </Label>
              <Input
                id="webhook-url"
                type="url"
                placeholder={t('messages.channels.webhookUrlPlaceholder')}
                value={(newChannelConfig.url as string) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, url: e.target.value }))}
              />
              <p className="text-xs text-muted-foreground mt-1">
                {t('messages.channels.webhookHint', 'Visit')} <a href="https://webhook.site" target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:underline">webhook.site</a> {t('messages.channels.webhookHint2', 'to get a free test URL')}
              </p>
            </div>
            <div>
              <Label htmlFor="webhook-headers">{t('messages.channels.webhookHeaders')}</Label>
              <Input
                id="webhook-headers"
                placeholder={t('messages.channels.headersPlaceholder')}
                value={(newChannelConfig.headers as string) || ''}
                onChange={(e) => {
                  try {
                    const headers = JSON.parse(e.target.value)
                    setNewChannelConfig(prev => ({ ...prev, headers }))
                  } catch {
                    // Invalid JSON, ignore
                  }
                }}
              />
            </div>
          </>
        )}
        {selectedChannelType.id === 'email' && (
          <>
            <div className="bg-muted/50 rounded-lg p-3 text-sm text-muted-foreground">
              <p className="font-medium mb-2">{t('messages.channels.quickTestOptions', 'Quick Test Options:')}</p>
              <ul className="space-y-1 text-xs">
                <li>• <strong>MailHog</strong>: Run <code className="bg-background px-1 py-0.5 rounded">mailhog</code> (localhost:1025)</li>
                <li>• <strong>Ethereal</strong>: <a href="https://ethereal.email" target="_blank" rel="noopener noreferrer" className="text-blue-500 hover:underline">ethereal.email</a></li>
                <li>• <strong>Gmail</strong>: smtp.gmail.com:587 (use App Password)</li>
              </ul>
            </div>
            <div>
              <Label htmlFor="smtp-server">{t('messages.channels.smtpServer')}</Label>
              <Input
                id="smtp-server"
                placeholder={t('messages.channels.smtpServerPlaceholder')}
                value={(newChannelConfig.smtp_server as string) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, smtp_server: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="smtp-port">{t('messages.channels.smtpPort')}</Label>
              <Input
                id="smtp-port"
                type="number"
                placeholder={t('messages.channels.portPlaceholder')}
                value={(newChannelConfig.smtp_port as number) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, smtp_port: parseInt(e.target.value) || 1025 }))}
              />
            </div>
            <div>
              <Label htmlFor="email-username">{t('messages.channels.emailUsername')}</Label>
              <Input
                id="email-username"
                placeholder={t('messages.channels.optionalPlaceholder')}
                value={(newChannelConfig.username as string) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, username: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="email-password">{t('messages.channels.emailPassword')}</Label>
              <Input
                id="email-password"
                type="password"
                placeholder={t('messages.channels.optionalPlaceholder')}
                value={(newChannelConfig.password as string) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, password: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="email-from">{t('messages.channels.emailFrom')}</Label>
              <Input
                id="email-from"
                type="email"
                placeholder={t('messages.channels.emailFromPlaceholder')}
                value={(newChannelConfig.from_address as string) || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, from_address: e.target.value }))}
              />
            </div>
            <div>
              <Label htmlFor="email-to">{t('messages.channels.emailTo')}</Label>
              <Input
                id="email-to"
                type="email"
                placeholder={t('messages.channels.yourEmailPlaceholder')}
                value={(newChannelConfig.recipients as string[])?.[0] || ''}
                onChange={(e) => setNewChannelConfig(prev => ({ ...prev, recipients: [e.target.value] }))}
              />
            </div>
          </>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-4">
      {/* Stats */}
      {stats && (
        <div className="grid grid-cols-2 md:grid-cols-3 gap-4">
          <Card>
            <CardContent className="p-4">
              <div className="flex items-center gap-3">
                <div className="w-2 h-8 rounded-full bg-blue-500" />
                <div>
                  <p className="text-sm text-muted-foreground">{t('messages.channels.total')}</p>
                  <p className="text-2xl font-bold">{stats.total || channels.length}</p>
                </div>
              </div>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="p-4">
              <div className="flex items-center gap-3">
                <div className="w-2 h-8 rounded-full bg-green-500" />
                <div>
                  <p className="text-sm text-muted-foreground">{t('messages.channels.enabled')}</p>
                  <p className="text-2xl font-bold">{stats.enabled || channels.filter(c => c.enabled).length}</p>
                </div>
              </div>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="p-4">
              <div className="flex items-center gap-3">
                <div className="w-2 h-8 rounded-full bg-gray-500" />
                <div>
                  <p className="text-sm text-muted-foreground">{t('messages.channels.types')}</p>
                  <p className="text-2xl font-bold">{channelTypes.length}</p>
                </div>
              </div>
            </CardContent>
          </Card>
        </div>
      )}

      {/* Actions */}
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold">{t('messages.channels.title')}</h3>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={onRefresh} disabled={isLoading}>
            <RefreshCw className={`h-4 w-4 ${isLoading ? 'animate-spin' : ''}`} />
            {t('common.refresh')}
          </Button>
          <Button size="sm" onClick={() => setCreateDialogOpen(true)}>
            <Plus className="h-4 w-4 mr-1" />
            {t('messages.channels.create')}
          </Button>
        </div>
      </div>

      {/* Channels List */}
      {isLoading ? (
        <LoadingState />
      ) : error ? (
        <EmptyState
          icon={<RefreshCw className="h-12 w-12" />}
          title={t('messages.channels.error')}
          description={error.message}
          action={{ label: t('common.retry'), onClick: onRefresh }}
        />
      ) : channels.length === 0 ? (
        <EmptyState
          icon={<Plus className="h-12 w-12" />}
          title={t('messages.channels.empty.title')}
          description={t('messages.channels.empty.description')}
          action={{
            label: t('messages.channels.create'),
            onClick: () => setCreateDialogOpen(true)
          }}
        />
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {channels.map((channel) => {
            const config = CHANNEL_CONFIG[channel.channel_type] || CHANNEL_CONFIG.console
            const testResult = testResults[channel.name]

            return (
              <Card key={channel.name}>
                <CardHeader className="pb-3">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <div className={`w-8 h-8 rounded ${config.color} flex items-center justify-center text-white text-xs font-bold`}>
                        {channel.channel_type[0].toUpperCase()}
                      </div>
                      <CardTitle className="text-base">{channel.name}</CardTitle>
                      <Badge variant="outline">{channel.channel_type}</Badge>
                    </div>
                    <div className="flex items-center gap-2">
                      <Switch
                        checked={channel.enabled}
                        disabled={channel.channel_type === 'console' || channel.channel_type === 'memory'}
                      />
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="space-y-3">
                  {/* Test result */}
                  {testResult && (
                    <div
                      className={`flex items-center gap-2 text-sm p-2 rounded ${
                        testResult.success ? 'bg-green-50 text-green-700' : 'bg-red-50 text-red-700'
                      }`}
                    >
                      {testResult.success ? (
                        <Check className="h-4 w-4" />
                      ) : (
                        <X className="h-4 w-4" />
                      )}
                      <span className="flex-1 truncate">{testResult.message || (testResult.success ? 'OK' : 'Failed')}</span>
                    </div>
                  )}

                  {/* Actions */}
                  <div className="flex items-center justify-end gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => handleTestChannel(channel.name)}
                      disabled={testingChannel === channel.name}
                    >
                      <TestTube className="h-4 w-4 mr-1" />
                      {testingChannel === channel.name ? t('common.testing') : t('messages.channels.test')}
                    </Button>
                    {channel.channel_type !== 'console' && channel.channel_type !== 'memory' && (
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleDeleteChannel(channel.name)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    )}
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      )}

      {/* Create Channel Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('messages.channels.createTitle')}</DialogTitle>
            <DialogDescription>
              {t('messages.channels.createDescription')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-4 py-4">
            <div>
              <Label htmlFor="channel-name">{t('messages.channels.name')}</Label>
              <Input
                id="channel-name"
                value={newChannelName}
                onChange={(e) => setNewChannelName(e.target.value)}
                placeholder={t('messages.channels.channelNamePlaceholder')}
              />
            </div>

            <div>
              <Label htmlFor="channel-type">{t('messages.channels.type')}</Label>
              <Select value={newChannelType} onValueChange={setNewChannelType}>
                <SelectTrigger id="channel-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {channelTypes.map((ct) => (
                    <SelectItem key={ct.id} value={ct.id}>
                      {ct.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {renderConfigFields()}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={handleCreateChannel} disabled={!newChannelName.trim()}>
              {t('common.create')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

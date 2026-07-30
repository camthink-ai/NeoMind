// ChannelEditorDialog Component
// FullScreenDialog with sidebar for creating/editing notification channels
// Replaces the inline UnifiedFormDialog in messages.tsx

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Megaphone,
  Mail,
  Send,
  MessageSquare,
  MessageCircle,
  Hash,
  MessagesSquare,
  Plus,
  Settings,
  Trash2,
  Check,
  type LucideIcon,
} from 'lucide-react'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { useIsMobile } from '@/hooks/useMobile'
import type { MessageChannel } from '@/types'

import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { FormField } from '@/components/ui/field'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogSidebar,
  FullScreenDialogMain,
} from '@/components/automation/dialog/FullScreenDialog'
import { cn } from '@/lib/utils'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface ChannelEditorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  editingChannel?: MessageChannel | null
  onSaved: () => void
}

interface ChannelTypeDef {
  value: string
  icon: LucideIcon
  labelKey: string
  descKey: string
  color: string
}

// ---------------------------------------------------------------------------
// Channel type definitions (sidebar entries)
// ---------------------------------------------------------------------------

const CHANNEL_TYPES: ChannelTypeDef[] = [
  {
    value: 'webhook',
    icon: Megaphone,
    labelKey: 'messages.channels.typeWebhook',
    descKey: 'messages.channels.typeWebhookDesc',
    color: 'bg-success-light text-success',
  },
  {
    value: 'email',
    icon: Mail,
    labelKey: 'messages.channels.typeEmail',
    descKey: 'messages.channels.typeEmailDesc',
    color: 'bg-info-light text-info',
  },
  {
    value: 'telegram',
    icon: Send,
    labelKey: 'messages.channels.typeTelegram',
    descKey: 'messages.channels.typeTelegramDesc',
    color: 'bg-warning-light text-warning',
  },
  {
    value: 'wecom',
    icon: MessageSquare,
    labelKey: 'messages.channels.typeWeCom',
    descKey: 'messages.channels.typeWeComDesc',
    color: 'bg-success-light text-success',
  },
  {
    value: 'dingtalk',
    icon: MessageCircle,
    labelKey: 'messages.channels.typeDingTalk',
    descKey: 'messages.channels.typeDingTalkDesc',
    color: 'bg-info-light text-info',
  },
  {
    value: 'slack',
    icon: Hash,
    labelKey: 'messages.channels.typeSlack',
    descKey: 'messages.channels.typeSlackDesc',
    color: 'bg-warning-light text-warning',
  },
  {
    value: 'feishu',
    icon: MessagesSquare,
    labelKey: 'messages.channels.typeFeishu',
    descKey: 'messages.channels.typeFeishuDesc',
    color: 'bg-info-light text-info',
  },
]

// ---------------------------------------------------------------------------
// Config form sections per channel type
// ---------------------------------------------------------------------------

function WebhookConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.webhookUrl', 'URL')} required>
        <Input
          value={(config.url as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, url: e.target.value }))}
          placeholder="https://example.com/webhook"
        />
      </FormField>

      <FormField label={t('messages.channels.authType')}>
        <Select
          value={(config._authType as string) || 'none'}
          onValueChange={(value) => {
            onChange(prev => {
              const next: Record<string, unknown> = { ...prev, _authType: value }
              delete next._bearerToken
              delete next._basicUser
              delete next._basicPass
              delete next._apiKeyHeader
              delete next._apiKeyValue
              delete next._customHeaders
              return next
            })
          }}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{t('messages.channels.authNone')}</SelectItem>
            <SelectItem value="bearer">{t('messages.channels.authBearer')}</SelectItem>
            <SelectItem value="basic">{t('messages.channels.authBasic')}</SelectItem>
            <SelectItem value="apikey">{t('messages.channels.authApiKey')}</SelectItem>
            <SelectItem value="custom">{t('messages.channels.authCustom')}</SelectItem>
          </SelectContent>
        </Select>
      </FormField>

      {(config._authType as string) === 'bearer' && (
        <FormField label={t('messages.channels.bearerToken')} required>
          <PasswordInput
            value={(config._bearerToken as string) || ''}
            onChange={(e) => onChange(prev => ({ ...prev, _bearerToken: e.target.value }))}
            placeholder={t('messages.channels.bearerTokenPlaceholder')}
          />
        </FormField>
      )}

      {(config._authType as string) === 'basic' && (
        <div className="space-y-3">
          <FormField label={t('messages.channels.basicUser')} required>
            <Input
              value={(config._basicUser as string) || ''}
              onChange={(e) => onChange(prev => ({ ...prev, _basicUser: e.target.value }))}
              placeholder={t('messages.channels.basicUserPlaceholder')}
            />
          </FormField>
          <FormField label={t('messages.channels.basicPass')} required>
            <PasswordInput
              value={(config._basicPass as string) || ''}
              onChange={(e) => onChange(prev => ({ ...prev, _basicPass: e.target.value }))}
              placeholder={t('messages.channels.basicPassPlaceholder')}
            />
          </FormField>
        </div>
      )}

      {(config._authType as string) === 'apikey' && (
        <div className="space-y-3">
          <FormField label={t('messages.channels.apiKeyHeader')}>
            <Input
              value={(config._apiKeyHeader as string) || 'X-API-Key'}
              onChange={(e) => onChange(prev => ({ ...prev, _apiKeyHeader: e.target.value }))}
              placeholder="X-API-Key"
            />
          </FormField>
          <FormField label={t('messages.channels.apiKeyValue')} required>
            <PasswordInput
              value={(config._apiKeyValue as string) || ''}
              onChange={(e) => onChange(prev => ({ ...prev, _apiKeyValue: e.target.value }))}
              placeholder={t('messages.channels.apiKeyValuePlaceholder')}
            />
          </FormField>
        </div>
      )}

      {(config._authType as string) === 'custom' && (
        <div className="space-y-2">
          {(((config._customHeaders as Array<{ key: string; value: string }>) || [])).map((header, index) => (
            <div key={index} className="flex items-center gap-2">
              <Input
                value={header.key}
                onChange={(e) => {
                  const headers = ((config._customHeaders as Array<{ key: string; value: string }>) || [])
                  const updated = [...headers]
                  updated[index] = { ...updated[index], key: e.target.value }
                  onChange(prev => ({ ...prev, _customHeaders: updated }))
                }}
                placeholder={t('messages.channels.headerName')}
                className="flex-1"
              />
              <Input
                value={header.value}
                onChange={(e) => {
                  const headers = ((config._customHeaders as Array<{ key: string; value: string }>) || [])
                  const updated = [...headers]
                  updated[index] = { ...updated[index], value: e.target.value }
                  onChange(prev => ({ ...prev, _customHeaders: updated }))
                }}
                placeholder={t('messages.channels.headerValue')}
                className="flex-1"
              />
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  const headers = ((config._customHeaders as Array<{ key: string; value: string }>) || [])
                  onChange(prev => ({
                    ...prev,
                    _customHeaders: headers.filter((_, i) => i !== index)
                  }))
                }}
              >
                <Trash2 className="h-4 w-4 text-destructive" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              const headers = ((config._customHeaders as Array<{ key: string; value: string }>) || [])
              onChange(prev => ({
                ...prev,
                _customHeaders: [...headers, { key: '', value: '' }]
              }))
            }}
          >
            <Plus className="h-4 w-4 mr-1" />
            {t('messages.channels.addHeader')}
          </Button>
        </div>
      )}

      <FormField label={t('messages.channels.timeout')}>
        <Input
          type="number"
          value={(config.timeout_secs as unknown as string) || '30'}
          onChange={(e) => onChange(prev => ({ ...prev, timeout_secs: parseInt(e.target.value) || 30 }))}
          placeholder="30"
          min={1}
          max={300}
        />
      </FormField>
    </div>
  )
}

function EmailConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.smtpServer')} required>
        <Input
          value={(config.smtp_server as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, smtp_server: e.target.value }))}
          placeholder="smtp.example.com"
        />
      </FormField>
      <FormField label={t('messages.channels.smtpPort')}>
        <Input
          type="number"
          value={(config.smtp_port as string) || '587'}
          onChange={(e) => onChange(prev => ({ ...prev, smtp_port: parseInt(e.target.value) || 587 }))}
          placeholder="587"
        />
      </FormField>
      <FormField label={t('messages.channels.smtpUser')} required>
        <Input
          value={(config.username as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, username: e.target.value }))}
          placeholder={t('messages.channels.smtpUserPlaceholder')}
        />
      </FormField>
      <FormField label={t('messages.channels.smtpPass')} required>
        <PasswordInput
          value={(config.password as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, password: e.target.value }))}
          placeholder={t('messages.channels.smtpPassPlaceholder')}
        />
      </FormField>
      <FormField label={t('messages.channels.fromAddress')} required>
        <Input
          value={(config.from_address as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, from_address: e.target.value }))}
          placeholder="noreply@example.com"
        />
      </FormField>
    </div>
  )
}

function TelegramConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.botToken')} required>
        <PasswordInput
          value={(config.token as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, token: e.target.value }))}
          placeholder="bot123:ABC-xxx"
        />
      </FormField>
      <FormField label={t('messages.channels.chatId')} required>
        <Input
          value={(config.chat_id as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, chat_id: e.target.value }))}
          placeholder="-100xxx"
        />
      </FormField>
    </div>
  )
}

function WeComConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.webhookKey')} required>
        <Input
          value={(config.key as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, key: e.target.value }))}
          placeholder="xxx-xxx-xxx"
        />
      </FormField>
    </div>
  )
}

function DingTalkConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.accessToken')} required>
        <Input
          value={(config.access_token as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, access_token: e.target.value }))}
          placeholder="xxx"
        />
      </FormField>
      <FormField label={t('messages.channels.secret')}>
        <PasswordInput
          value={(config.secret as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, secret: e.target.value }))}
          placeholder={t('messages.channels.secretOptional')}
        />
      </FormField>
    </div>
  )
}

function SlackConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.webhookUrl')} required>
        <Input
          type="url"
          value={(config.webhook_url as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, webhook_url: e.target.value }))}
          placeholder="https://hooks.slack.com/services/Txxx/Bxxx/xxxx"
        />
      </FormField>
    </div>
  )
}

function FeishuConfig({ config, onChange }: {
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}) {
  const { t } = useTranslation()

  return (
    <div className="space-y-3">
      <FormField label={t('messages.channels.hookId')} required>
        <Input
          value={(config.hook_id as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, hook_id: e.target.value }))}
          placeholder="xxx"
        />
      </FormField>
      <FormField label={t('messages.channels.secret')}>
        <PasswordInput
          value={(config.secret as string) || ''}
          onChange={(e) => onChange(prev => ({ ...prev, secret: e.target.value }))}
          placeholder={t('messages.channels.secretOptional')}
        />
      </FormField>
    </div>
  )
}

// Map channel type to config component
const CONFIG_COMPONENTS: Record<string, React.ComponentType<{
  config: Record<string, unknown>
  onChange: (updater: (prev: Record<string, unknown>) => Record<string, unknown>) => void
}>> = {
  webhook: WebhookConfig,
  email: EmailConfig,
  telegram: TelegramConfig,
  wecom: WeComConfig,
  dingtalk: DingTalkConfig,
  slack: SlackConfig,
  feishu: FeishuConfig,
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export function ChannelEditorDialog({
  open,
  onOpenChange,
  editingChannel,
  onSaved,
}: ChannelEditorDialogProps) {
  const { t } = useTranslation()
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const isMobile = useIsMobile()

  // Form state
  const [channelName, setChannelName] = useState('')
  const [channelType, setChannelType] = useState('webhook')
  const [channelConfig, setChannelConfig] = useState<Record<string, unknown>>({})
  const [creating, setCreating] = useState(false)

  // Reset form when dialog opens / editingChannel changes
  const resetForm = useCallback((channel?: MessageChannel | null) => {
    if (channel) {
      setChannelName(channel.name)
      setChannelType(channel.channel_type)
      const cfg: Record<string, unknown> = channel.config ? { ...channel.config } : {}
      if (channel.channel_type === 'webhook') {
        const headers = (cfg.headers as Record<string, string>) || {}
        if (headers['Authorization']?.startsWith('Bearer ')) {
          cfg._authType = 'bearer'
          cfg._bearerToken = headers['Authorization'].replace('Bearer ', '')
        } else if (headers['Authorization']?.startsWith('Basic ')) {
          cfg._authType = 'basic'
          try {
            const decoded = atob(headers['Authorization'].replace('Basic ', ''))
            const [user, ...passParts] = decoded.split(':')
            cfg._basicUser = user
            cfg._basicPass = passParts.join(':')
          } catch { /* ignore */ }
        } else {
          cfg._authType = 'none'
        }
      }
      setChannelConfig(cfg)
    } else {
      setChannelName('')
      setChannelType('webhook')
      setChannelConfig({})
    }
  }, [])

  // Initialize form when dialog opens
  useEffect(() => {
    if (open) {
      resetForm(editingChannel)
    }
  }, [open, editingChannel, resetForm])

  // Submit handler (create or update)
  const handleSubmit = useCallback(async () => {
    if (!channelName.trim()) return
    setCreating(true)
    try {
      const config: Record<string, unknown> = { ...channelConfig }
      if (channelType === 'webhook' && config._authType) {
        const headers: Record<string, string> = (config.headers as Record<string, string>) || {}
        const authType = config._authType as string
        delete config._authType

        if (authType === 'bearer' && config._bearerToken) {
          headers['Authorization'] = `Bearer ${config._bearerToken}`
          delete config._bearerToken
        } else if (authType === 'basic' && config._basicUser && config._basicPass) {
          headers['Authorization'] = `Basic ${btoa(`${config._basicUser}:${config._basicPass}`)}`
          delete config._basicUser
          delete config._basicPass
        } else if (authType === 'apikey' && config._apiKeyValue) {
          const headerName = (config._apiKeyHeader as string) || 'X-API-Key'
          headers[headerName] = config._apiKeyValue as string
          delete config._apiKeyHeader
          delete config._apiKeyValue
        } else if (authType === 'custom' && config._customHeaders) {
          const customHeaders = config._customHeaders as Array<{ key: string; value: string }>
          for (const h of customHeaders) {
            if (h.key.trim()) {
              headers[h.key.trim()] = h.value
            }
          }
        }
        config.headers = headers
      }
      // Remove internal fields
      delete config._authType
      delete config._bearerToken
      delete config._basicUser
      delete config._basicPass
      delete config._apiKeyHeader
      delete config._apiKeyValue
      delete config._customHeaders

      if (editingChannel) {
        await api.updateMessageChannel(editingChannel.name, config)
        toast({ title: t('common:success'), description: t('messages.channels.updateSuccess') })
      } else {
        await api.createMessageChannel({
          name: channelName.trim(),
          channel_type: channelType,
          ...config,
        })
        toast({ title: t('common:success'), description: t('messages.channels.createSuccess') })
      }
      onOpenChange(false)
      onSaved()
    } catch (error) {
      handleError(error, { operation: editingChannel ? 'Update channel' : 'Create channel', showToast: true })
    } finally {
      setCreating(false)
    }
  }, [channelName, channelType, channelConfig, editingChannel, onOpenChange, onSaved, handleError, toast, t])

  // Resolve the active config component
  const ConfigComponent = CONFIG_COMPONENTS[channelType]

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={editingChannel
          ? <Settings className="h-5 w-5" />
          : <Plus className="h-5 w-5" />
        }
        iconBg={editingChannel ? 'bg-muted-50' : 'bg-success-light'}
        iconColor={editingChannel ? 'text-muted-foreground' : 'text-success'}
        title={editingChannel
          ? t('messages.channels.editChannel')
          : t('messages.channels.createChannel')
        }
        onClose={() => onOpenChange(false)}
      />

      <FullScreenDialogContent>
        {/* Sidebar — hidden on mobile */}
        <FullScreenDialogSidebar>
          <div className="p-2 space-y-1">
            {CHANNEL_TYPES.map((ct) => {
              const Icon = ct.icon
              const isActive = channelType === ct.value
              return (
                <button
                  key={ct.value}
                  onClick={() => {
                    if (!editingChannel) {
                      setChannelType(ct.value)
                      setChannelConfig({})
                    }
                  }}
                  disabled={!!editingChannel}
                  className={cn(
                    "w-full flex items-center gap-3 px-3 py-2.5 rounded-lg transition-all text-left",
                    isActive
                      ? "bg-primary-light text-primary font-medium"
                      : "text-muted-foreground hover:bg-muted-50 hover:text-foreground",
                    !!editingChannel && "cursor-not-allowed opacity-60"
                  )}
                >
                  <div className={cn(
                    "shrink-0 flex items-center justify-center w-8 h-8 rounded-lg",
                    isActive ? ct.color : "bg-muted-50"
                  )}>
                    <Icon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm truncate">{t(ct.labelKey)}</div>
                    <div className="text-xs text-muted-foreground truncate">
                      {t(ct.descKey, { defaultValue: '' })}
                    </div>
                  </div>
                  {isActive && <Check className="h-4 w-4 shrink-0 text-primary" />}
                </button>
              )
            })}
          </div>
        </FullScreenDialogSidebar>

        <div className="flex-1 flex flex-col min-w-0">
          {/* Mobile: 4-col grid (7 channels wrap to 4+3), single-row on wider phones */}
          {isMobile && (
            <div className="shrink-0 px-3 pt-3 pb-2">
              <div className="grid grid-cols-4 gap-1.5">
                {CHANNEL_TYPES.map((ct) => {
                  const Icon = ct.icon
                  const isActive = channelType === ct.value
                  return (
                    <button
                      key={ct.value}
                      onClick={() => {
                        if (!editingChannel) {
                          setChannelType(ct.value)
                          setChannelConfig({})
                        }
                      }}
                      disabled={!!editingChannel}
                      className={cn(
                        "flex flex-col items-center justify-center gap-1 py-1.5 min-w-0 rounded-lg transition-colors",
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "bg-muted-30 text-muted-foreground",
                        !!editingChannel && "cursor-not-allowed opacity-60"
                      )}
                    >
                      <Icon className="h-4 w-4 shrink-0" />
                      <span className="text-[11px] font-medium leading-none truncate w-full text-center">
                        {t(ct.labelKey)}
                      </span>
                    </button>
                  )
                })}
              </div>
            </div>
          )}

          <FullScreenDialogMain>
            <div className="p-4 md:p-6 space-y-4 max-w-2xl mx-auto">
              {/* Channel name */}
              <FormField
                label={t('messages.channels.name')}
                required
              >
                <Input
                  value={channelName}
                  onChange={(e) => setChannelName(e.target.value)}
                  placeholder={t('messages.channels.channelNamePlaceholder')}
                  disabled={!!editingChannel}
                  autoFocus
                />
              </FormField>

              {/* Dynamic config form */}
              {ConfigComponent && (
                <ConfigComponent
                  config={channelConfig}
                  onChange={setChannelConfig}
                />
              )}
            </div>
          </FullScreenDialogMain>

          <FullScreenDialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t('common:cancel')}
            </Button>
            <Button onClick={handleSubmit} disabled={creating || !channelName.trim()}>
              {creating
                ? t('common:loading', { defaultValue: 'Saving...' })
                : editingChannel
                  ? t('common:save')
                  : t('messages.channels.create')
              }
            </Button>
          </FullScreenDialogFooter>
        </div>
      </FullScreenDialogContent>
    </FullScreenDialog>
  )
}

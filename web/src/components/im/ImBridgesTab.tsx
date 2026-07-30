import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { TFunction } from 'i18next'
import { Send, Plus, Trash2, Copy, QrCode, Check, X } from 'lucide-react'
import { QRCodeSVG } from 'qrcode.react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button, IconButton } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { FormField } from '@/components/ui/field'
import { EmptyState, LoadingState, ListToolbar } from '@/components/shared'
import { confirm } from '@/hooks/use-confirm'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { notifySuccess, notifyError } from '@/lib/notify'
import { api, type ImBridge, type ImInvite } from '@/lib/api'
import { cn } from '@/lib/utils'
import { IM_PLATFORMS, getPlatformDef, type ImPlatformDef, type ImPlatformField } from './platforms'

type View = 'list' | 'detail' | 'select' | 'configure'

/** Display name for a platform id, resolved from the registry (falls back to capitalized id). */
function platformDisplayName(platform: string, t: TFunction): string {
  const def = getPlatformDef(platform)
  if (def) return t(def.nameKey)
  return platform.charAt(0).toUpperCase() + platform.slice(1)
}

/** Normalize a backend bridge status into { label, className } for a Badge. */
function statusBadge(status: string): { label: string; className: string } {
  const s = (status || '').toLowerCase()
  const active = s === 'connected' || s === 'active' || s === 'online' || s === 'running' || s === 'ok'
  const label = active ? 'connected' : s || 'unknown'
  const cls = active
    ? 'bg-success-light text-success border-success-light'
    : 'bg-muted-30 text-muted-foreground border-border'
  return { label: label.charAt(0).toUpperCase() + label.slice(1), className: cls }
}

export function ImBridgesTab() {
  const { t } = useTranslation(['settings', 'common'])
  const { handleError } = useErrorHandler()

  const [view, setView] = useState<View>('list')
  const [loading, setLoading] = useState(true)
  const [bridges, setBridges] = useState<ImBridge[]>([])
  const [selectedBridge, setSelectedBridge] = useState<ImBridge | null>(null)

  // Detail-view data
  const [invites, setInvites] = useState<ImInvite[]>([])
  const [allowlist, setAllowlist] = useState<string[]>([])
  const [detailLoading, setDetailLoading] = useState(false)
  const [lastInvite, setLastInvite] = useState<{ deep_link: string | null } | null>(null)

  // Add-flow state: select-platform → configure. The form field values live
  // inside <PlatformConfigForm/>; here we only track which platform was
  // picked and whether the create request is in flight.
  const [selectedPlatform, setSelectedPlatform] = useState<ImPlatformDef | null>(null)
  const [creating, setCreating] = useState(false)

  // Invite generation + clipboard
  const [generating, setGenerating] = useState(false)
  const [copiedLink, setCopiedLink] = useState(false)

  useEffect(() => {
    loadBridges()
  }, [])

  const loadBridges = async () => {
    setLoading(true)
    try {
      const res = await api.listImBridges()
      setBridges(res.bridges || [])
    } catch (error) {
      handleError(error, { operation: 'Load IM bridges', showToast: false })
      setBridges([])
    } finally {
      setLoading(false)
    }
  }

  const loadDetail = async (id: string) => {
    setDetailLoading(true)
    try {
      const [invRes, allowRes] = await Promise.all([
        api.listImInvites(id).catch(() => ({ invites: [] as ImInvite[] })),
        api.listImAllowlist(id).catch(() => ({ allowlist: [] as string[] })),
      ])
      setInvites(invRes.invites || [])
      setAllowlist(allowRes.allowlist || [])
    } finally {
      setDetailLoading(false)
    }
  }

  const openDetail = (bridge: ImBridge) => {
    setSelectedBridge(bridge)
    setLastInvite(null)
    setView('detail')
    loadDetail(bridge.id)
  }

  const handlePlatformSelect = (def: ImPlatformDef) => {
    setSelectedPlatform(def)
    setView('configure')
  }

  // Builds the create payload purely from the selected platform id + the
  // field-driven values collected by <PlatformConfigForm/>. No
  // Telegram-specific keys are referenced here — the field definitions are
  // the single source of truth.
  const handleCreate = async (values: Record<string, string>) => {
    if (!selectedPlatform) return
    setCreating(true)
    try {
      const payload = {
        platform: selectedPlatform.id,
        ...values,
      } as { platform: string; bot_token: string; api_base?: string }
      await api.createImBridge(payload)
      notifySuccess(t('settings:im.bridgeCreated'))
      setSelectedPlatform(null)
      setView('list')
      await loadBridges()
    } catch (error) {
      handleError(error, { operation: 'Create IM bridge' })
    } finally {
      setCreating(false)
    }
  }

  const handleDelete = async (bridge: ImBridge) => {
    const confirmed = await confirm({
      title: t('settings:im.deleteBridge'),
      description: t('settings:im.confirmDelete'),
      confirmText: t('common:delete', { defaultValue: 'Delete' }),
      cancelText: t('common:cancel', { defaultValue: 'Cancel' }),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      await api.deleteImBridge(bridge.id)
      notifySuccess(t('settings:im.bridgeDeleted'))
      if (selectedBridge?.id === bridge.id) {
        setSelectedBridge(null)
        setView('list')
      }
      await loadBridges()
    } catch (error) {
      handleError(error, { operation: 'Delete IM bridge' })
    }
  }

  const handleGenerateInvite = async () => {
    if (!selectedBridge) return
    setGenerating(true)
    try {
      const created = await api.createImInvite(selectedBridge.id)
      setLastInvite(created)
      notifySuccess(t('settings:im.inviteGenerated'))
      await loadDetail(selectedBridge.id)
    } catch (error) {
      handleError(error, { operation: 'Generate invite' })
    } finally {
      setGenerating(false)
    }
  }

  const handleRevoke = async (token: string) => {
    if (!selectedBridge) return
    const confirmed = await confirm({
      title: t('settings:im.revoke'),
      description: t('settings:im.confirmRevoke'),
      confirmText: t('settings:im.revoke', { defaultValue: 'Revoke' }),
      cancelText: t('common:cancel', { defaultValue: 'Cancel' }),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      await api.revokeImInvite(selectedBridge.id, token)
      notifySuccess(t('settings:im.inviteRevoked'))
      setLastInvite(null)
      await loadDetail(selectedBridge.id)
    } catch (error) {
      handleError(error, { operation: 'Revoke invite' })
    }
  }

  const handleRemoveAllowed = async (chatId: string) => {
    if (!selectedBridge) return
    const confirmed = await confirm({
      title: t('settings:im.remove'),
      description: t('settings:im.confirmRemove'),
      confirmText: t('settings:im.remove', { defaultValue: 'Remove' }),
      cancelText: t('common:cancel', { defaultValue: 'Cancel' }),
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      await api.removeImAllowed(selectedBridge.id, chatId)
      notifySuccess(t('settings:im.chatRemoved'))
      setAllowlist(prev => prev.filter(c => c !== chatId))
    } catch (error) {
      handleError(error, { operation: 'Remove allowed chat' })
    }
  }

  const handleCopyLink = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text)
      setCopiedLink(true)
      notifySuccess(t('settings:im.linkCopied'))
      setTimeout(() => setCopiedLink(false), 2000)
    } catch {
      notifyError(t('settings:im.copyFailed'))
    }
  }

  if (loading) {
    return <LoadingState variant="page" text={t('common:loading', { defaultValue: 'Loading...' })} />
  }

  // ========== SELECT-PLATFORM VIEW (add-flow step 1) ==========
  if (view === 'select') {
    const available = IM_PLATFORMS.filter(p => p.available)
    return (
      <>
        <ListToolbar
          onBack={() => setView('list')}
          backLabel={t('settings:im.back', { defaultValue: 'Back' })}
          icon={<Send className="h-5 w-5" />}
          iconBg="bg-info-light text-info"
          title={t('settings:im.selectPlatform')}
          description={t('settings:im.addBridgeDesc')}
        />
        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,260px),1fr))]">
          {available.map(def => {
            const PlatformIcon = def.icon
            return (
              <Card
                key={def.id}
                className="cursor-pointer transition-all duration-200 hover:shadow-md"
                onClick={() => handlePlatformSelect(def)}
              >
                <CardHeader className="pb-3">
                  <div className={cn('flex items-center justify-center w-12 h-12 rounded-lg', def.iconBg)}>
                    <PlatformIcon className="h-6 w-6" />
                  </div>
                  <CardTitle className="text-base mt-3">{t(def.nameKey)}</CardTitle>
                  <CardDescription className="mt-1 text-xs line-clamp-2 min-h-[2.5em]">
                    {t(def.descriptionKey)}
                  </CardDescription>
                </CardHeader>
              </Card>
            )
          })}
        </div>
        <p className="text-sm text-muted-foreground mt-4">{t('settings:im.morePlatformsComingSoon')}</p>
      </>
    )
  }

  // ========== CONFIGURE-PLATFORM VIEW (add-flow step 2) ==========
  if (view === 'configure' && selectedPlatform) {
    const PlatformIcon = selectedPlatform.icon
    return (
      <>
        <ListToolbar
          onBack={() => setView('select')}
          backLabel={t('settings:im.back', { defaultValue: 'Back' })}
          icon={<PlatformIcon className="h-5 w-5" />}
          iconBg={selectedPlatform.iconBg}
          title={t('settings:im.configurePlatform', { platform: t(selectedPlatform.nameKey) })}
          description={t('settings:im.addBridgeDesc')}
        />
        <Card>
          <CardContent className="pt-6">
            <PlatformConfigForm
              fields={selectedPlatform.fields}
              onSubmit={handleCreate}
              submitting={creating}
              submitLabel={t('settings:im.create', { defaultValue: 'Create' })}
            />
          </CardContent>
        </Card>
      </>
    )
  }

  // ========== LIST VIEW ==========
  if (view === 'list') {
    if (bridges.length === 0) {
      return (
        <EmptyState
          icon={<Send className="h-12 w-12" />}
          title={t('settings:im.noBridges')}
          description={t('settings:im.noBridgesDesc')}
          action={{
            label: t('settings:im.addBridge'),
            onClick: () => setView('select'),
            icon: <Plus className="h-4 w-4" />,
          }}
        />
      )
    }

    return (
      <>
        {/* Header / toolbar */}
        <div className="flex items-start justify-between gap-3 mb-4">
          <div className="min-w-0">
            <h2 className="text-lg font-semibold">{t('settings:im.title')}</h2>
            <p className="text-sm text-muted-foreground mt-0.5">{t('settings:im.description')}</p>
          </div>
        </div>

        <div className="grid gap-4 grid-cols-[repeat(auto-fill,minmax(max(25%_-_1rem,260px),1fr))]">
          {bridges.map(bridge => {
            const st = statusBadge(bridge.status)
            return (
              <Card
                key={bridge.id}
                className="cursor-pointer transition-all duration-200 hover:shadow-md"
                onClick={() => openDetail(bridge)}
              >
                <CardHeader className="pb-3">
                  <div className="flex items-center justify-center w-12 h-12 rounded-lg bg-info-light text-info">
                    <Send className="h-6 w-6" />
                  </div>
                  <CardTitle className="text-base mt-3">{platformDisplayName(bridge.platform, t)}</CardTitle>
                  <CardDescription className="mt-1 text-xs">
                    {t('settings:im.platformFixed')}
                  </CardDescription>
                </CardHeader>
                <CardContent className="text-sm">
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('settings:im.status')}</span>
                    <Badge className={cn('text-xs border', st.className)}>{st.label}</Badge>
                  </div>
                  <div className="flex justify-between items-center mt-3">
                    <span className="text-muted-foreground">{t('settings:im.manage')}</span>
                    <div className="flex items-center gap-1">
                      <IconButton
                        size="sm"
                        aria-label={t('settings:im.manage')}
                        onClick={(e: React.MouseEvent) => {
                          e.stopPropagation()
                          openDetail(bridge)
                        }}
                      >
                        <QrCode className="h-4 w-4" />
                      </IconButton>
                      <IconButton
                        size="sm"
                        aria-label={t('common:delete', { defaultValue: 'Delete' })}
                        className="hover:text-error hover:bg-error-light"
                        onClick={(e: React.MouseEvent) => {
                          e.stopPropagation()
                          handleDelete(bridge)
                        }}
                      >
                        <Trash2 className="h-4 w-4" />
                      </IconButton>
                    </div>
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      </>
    )
  }

  // ========== DETAIL VIEW ==========
  if (view === 'detail' && selectedBridge) {
    const st = statusBadge(selectedBridge.status)
    const deepLink = lastInvite?.deep_link ?? null
    return (
      <>
        <ListToolbar
          onBack={() => {
            setSelectedBridge(null)
            setView('list')
          }}
          backLabel={t('settings:im.back', { defaultValue: 'Back' })}
          icon={<Send className="h-5 w-5" />}
          iconBg="bg-info-light text-info"
          title={platformDisplayName(selectedBridge.platform, t)}
          description={t('settings:im.detailDesc')}
          badges={<Badge className={cn('text-xs border', st.className)}>{st.label}</Badge>}
        />

        {/* Invites section */}
        <section className="mb-6">
          <div className="flex items-center justify-between gap-3 mb-3">
            <div className="min-w-0">
              <h3 className="text-base font-semibold">{t('settings:im.invites')}</h3>
              <p className="text-sm text-muted-foreground mt-0.5">{t('settings:im.invitesDesc')}</p>
            </div>
            <Button onClick={handleGenerateInvite} disabled={generating}>
              {generating ? <QrCode className="mr-2 h-4 w-4 animate-pulse" /> : <Plus className="mr-2 h-4 w-4" />}
              {t('settings:im.generateInvite')}
            </Button>
          </div>

          {/* Prominent QR for the just-generated invite */}
          {deepLink && (
            <Card className="mb-3 border-primary-light">
              <CardContent className="py-4">
                <div className="flex flex-col sm:flex-row items-center gap-4">
                  <div
                    className="flex items-center justify-center rounded-lg p-3 shrink-0"
                    style={{ backgroundColor: '#ffffff' }}
                  >
                    <QRCodeSVG value={deepLink} size={160} bgColor="#ffffff" fgColor="#000000" level="M" />
                  </div>
                  <div className="min-w-0 flex-1 w-full">
                    <p className="text-sm font-medium mb-1">{t('settings:im.scanToConnect')}</p>
                    <div className="flex items-center gap-2">
                      <code className="flex-1 min-w-0 truncate text-xs font-mono bg-muted-30 px-2 py-1 rounded">
                        {deepLink}
                      </code>
                      <IconButton
                        size="sm"
                        aria-label={t('settings:im.copyLink', { defaultValue: 'Copy link' })}
                        onClick={() => handleCopyLink(deepLink)}
                      >
                        {copiedLink ? <Check className="h-4 w-4 text-success" /> : <Copy className="h-4 w-4" />}
                      </IconButton>
                    </div>
                  </div>
                </div>
              </CardContent>
            </Card>
          )}

          {/* Invite generated but bot not identified yet — no deep link / QR available.
              Without this the QR card silently disappears after a "generate" success,
              looking broken; tell the user why (bot token not validated / username unknown). */}
          {lastInvite && !deepLink && (
            <Card className="mb-3 border-dashed">
              <CardContent className="py-4 flex items-center gap-2 text-sm text-muted-foreground">
                <QrCode className="h-4 w-4" />
                {t('settings:im.deepLinkUnavailable', {
                  defaultValue: 'Deep link is unavailable until the bot is identified.',
                })}
              </CardContent>
            </Card>
          )}

          {detailLoading ? (
            <LoadingState variant="default" size="sm" text={t('common:loading', { defaultValue: 'Loading...' })} />
          ) : invites.length === 0 && !deepLink ? (
            <Card className="border-dashed">
              <CardContent className="py-6">
                <EmptyState
                  icon={<QrCode className="h-10 w-10" />}
                  title={t('settings:im.noInvites')}
                  description={t('settings:im.noInvitesDesc')}
                />
              </CardContent>
            </Card>
          ) : (
            <div className="space-y-2">
              {invites.map(inv => {
                const short = inv.token.length > 10 ? `${inv.token.slice(0, 8)}…` : inv.token
                return (
                  <Card key={inv.token}>
                    <CardContent className="py-3">
                      <div className="flex items-center justify-between gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2 flex-wrap">
                            <code className="text-xs font-mono">{short}</code>
                            {inv.used ? (
                              <Badge className="bg-success-light text-success border-success-light text-xs">
                                {t('settings:im.used')}
                              </Badge>
                            ) : (
                              <Badge variant="secondary" className="text-xs">{t('settings:im.unused')}</Badge>
                            )}
                            {inv.used && inv.bound_chat_id && (
                              <span className="text-xs text-muted-foreground">
                                {t('settings:im.boundTo', { chatId: inv.bound_chat_id })}
                              </span>
                            )}
                          </div>
                        </div>
                        <IconButton
                          size="sm"
                          aria-label={t('settings:im.revoke', { defaultValue: 'Revoke' })}
                          className="hover:text-error hover:bg-error-light"
                          onClick={() => handleRevoke(inv.token)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </IconButton>
                      </div>
                    </CardContent>
                  </Card>
                )
              })}
            </div>
          )}
        </section>

        {/* Allowlist section */}
        <section>
          <div className="mb-3">
            <h3 className="text-base font-semibold">{t('settings:im.allowlist')}</h3>
            <p className="text-sm text-muted-foreground mt-0.5">{t('settings:im.allowlistDesc')}</p>
          </div>

          {detailLoading ? (
            <LoadingState variant="default" size="sm" text={t('common:loading', { defaultValue: 'Loading...' })} />
          ) : allowlist.length === 0 ? (
            <Card className="border-dashed">
              <CardContent className="py-6">
                <EmptyState
                  icon={<Send className="h-10 w-10" />}
                  title={t('settings:im.noAllowlist')}
                  description={t('settings:im.noAllowlistDesc')}
                />
              </CardContent>
            </Card>
          ) : (
            <div className="space-y-2">
              {allowlist.map(chatId => (
                <Card key={chatId}>
                  <CardContent className="py-3">
                    <div className="flex items-center justify-between gap-3">
                      <code className="text-xs font-mono truncate min-w-0">{chatId}</code>
                      <IconButton
                        size="sm"
                        aria-label={t('settings:im.remove', { defaultValue: 'Remove' })}
                        className="hover:text-error hover:bg-error-light"
                        onClick={() => handleRemoveAllowed(chatId)}
                      >
                        <X className="h-4 w-4" />
                      </IconButton>
                    </div>
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </section>
      </>
    )
  }

  return null
}

// ========== Platform config form (add-flow step 2 body) ==========

interface PlatformConfigFormProps {
  fields: ImPlatformField[]
  onSubmit: (values: Record<string, string>) => Promise<void>
  submitting: boolean
  submitLabel: string
}

/**
 * Renders a config form purely from `ImPlatformField[]` definitions. No
 * platform-specific knowledge here — each field is looked up by its i18n
 * keys, so wiring a new platform is a data-only change in `platforms.ts`.
 */
function PlatformConfigForm({ fields, onSubmit, submitting, submitLabel }: PlatformConfigFormProps) {
  const { t } = useTranslation(['settings', 'common'])
  const [values, setValues] = useState<Record<string, string>>(() =>
    Object.fromEntries(fields.map(f => [f.name, ''])),
  )

  const setValue = (name: string, v: string) => setValues(prev => ({ ...prev, [name]: v }))
  const requiredMissing = fields.some(f => f.required && !values[f.name]?.trim())

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (requiredMissing || submitting) return
    await onSubmit(values)
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {fields.map(field => (
        <FormField
          key={field.name}
          label={t(field.labelKey)}
          required={field.required}
          helpText={field.helpKey ? t(field.helpKey) : undefined}
        >
          <Input
            type={field.type}
            autoComplete="off"
            placeholder={field.placeholderKey ? t(field.placeholderKey) : undefined}
            value={values[field.name] ?? ''}
            onChange={e => setValue(field.name, e.target.value)}
          />
        </FormField>
      ))}
      <div className="flex justify-end pt-2">
        <Button type="submit" disabled={requiredMissing || submitting}>
          {submitLabel}
        </Button>
      </div>
    </form>
  )
}

export default ImBridgesTab

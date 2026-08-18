/**
 * PushTargetDialog - create or edit a push target using FullScreenDialog.
 *
 * Single-page layout (like Agent Editor):
 * - All config in one scrollable page
 * - Data source picker with search, group-by, multi-select
 * - No wizard steps
 */

import { useState, useEffect, useCallback, useMemo } from 'react'
import { useStore } from '@/store'
import { useTranslation } from 'react-i18next'
import {
  Send, Server, Puzzle, Workflow, Activity,
  Search, X, Loader2, ChevronRight, ChevronDown, KeyRound, Link2,
} from 'lucide-react'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { textNano, textMini } from '@/design-system/tokens/typography'
import { useIsMobile } from '@/hooks/useMobile'
import { api } from '@/lib/api'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogFooter,
  FullScreenDialogMain,
} from '@/components/automation/dialog'
import { PushTargetType, CreatePushTargetRequest, UpdatePushTargetRequest, UnifiedDataSourceInfo, ExternalBroker } from '@/types'
import { useFormSubmit } from '@/hooks/useErrorHandler'

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const SOURCE_TYPE_ICONS: Record<string, typeof Server> = {
  device: Server,
  extension: Puzzle,
  transform: Workflow,
  system: Activity,
}

const SOURCE_TYPE_KEYS: Record<string, string> = {
  device: 'common:dataPush.sourceTypeDevice',
  extension: 'common:dataPush.sourceTypeExtension',
  transform: 'common:dataPush.sourceTypeTransform',
  system: 'common:dataPush.sourceTypeSystem',
}

const SOURCE_TYPE_BADGE: Record<string, string> = {
  device: 'bg-info-light text-info',
  extension: 'bg-accent-purple-light text-accent-purple',
  transform: 'bg-accent-orange-light text-accent-orange',
  system: 'bg-success-light text-success',
}

// ---------------------------------------------------------------------------
// Helper: derive compact source patterns from selected IDs
// ---------------------------------------------------------------------------

function derivePatterns(selectedIds: Set<string>, allSources: UnifiedDataSourceInfo[]): string[] {
  if (selectedIds.size === 0) return []

  const byPrefix = new Map<string, Set<string>>()
  const prefixAllFields = new Map<string, number>()

  for (const src of allSources) {
    const prefix = `${src.source_type}:${src.source_name}:`
    prefixAllFields.set(prefix, (prefixAllFields.get(prefix) || 0) + 1)
  }

  for (const id of selectedIds) {
    const parts = id.split(':')
    if (parts.length < 3) continue
    const prefix = `${parts[0]}:${parts[1]}:`
    if (!byPrefix.has(prefix)) byPrefix.set(prefix, new Set())
    byPrefix.get(prefix)!.add(id)
  }

  const patterns: string[] = []
  for (const [prefix, ids] of byPrefix) {
    const totalForPrefix = prefixAllFields.get(prefix) || Infinity
    if (ids.size >= totalForPrefix) {
      patterns.push(prefix)
    } else {
      for (const id of ids) patterns.push(id)
    }
  }
  return patterns
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export function PushTargetDialog() {
  const { t } = useTranslation()
  const isMobile = useIsMobile()
  const {
    pushTargetDialogOpen,
    editingPushTarget,
    createPushTarget,
    updatePushTarget,
    setPushTargetDialogOpen,
  } = useStore()

  const isEditing = !!editingPushTarget

  // Form state
  const [name, setName] = useState('')
  const [nameError, setNameError] = useState<string | null>(null)
  const [targetType, setTargetType] = useState<PushTargetType>('webhook')
  const [webhookUrl, setWebhookUrl] = useState('')
  const [webhookUrlError, setWebhookUrlError] = useState<string | null>(null)
  const [mqttBroker, setMqttBroker] = useState('')
  const [mqttTopic, setMqttTopic] = useState('')
  const [mqttPort, setMqttPort] = useState(1883)
  const [mqttUsername, setMqttUsername] = useState('')
  const [mqttPassword, setMqttPassword] = useState('')
  const [mqttQos, setMqttQos] = useState(1)
  const [mqttMode, setMqttMode] = useState<'select' | 'manual'>('select')
  const [brokers, setBrokers] = useState<ExternalBroker[]>([])
  const [selectedBrokerId, setSelectedBrokerId] = useState('')
  // Webhook auth
  const [webhookAuthType, setWebhookAuthType] = useState<'none' | 'token' | 'basic'>('none')
  const [webhookAuthToken, setWebhookAuthToken] = useState('')
  const [webhookAuthUser, setWebhookAuthUser] = useState('')
  const [webhookAuthPass, setWebhookAuthPass] = useState('')
  const [scheduleType, setScheduleType] = useState<'event_driven' | 'interval'>('event_driven')
  const [intervalSecs, setIntervalSecs] = useState(60)
  // Source picker state
  const [sources, setSources] = useState<UnifiedDataSourceInfo[]>([])
  const [sourcesLoading, setSourcesLoading] = useState(false)
  const [selectedSources, setSelectedSources] = useState<Set<string>>(new Set())
  const [sourceSearch, setSourceSearch] = useState('')
  const [showAdvanced, setShowAdvanced] = useState(false)
  const [manualPatterns, setManualPatterns] = useState('')
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set())
  // Batch config
  const [batchEnabled, setBatchEnabled] = useState(false)
  const [batchSize, setBatchSize] = useState(50)
  const [batchIntervalMs, setBatchIntervalMs] = useState(2000)
  const [batchFormat, setBatchFormat] = useState<'flat' | 'nested'>('flat')
  // Pending patterns from editing target — expanded into selectedSources once sources load
  const [pendingPatterns, setPendingPatterns] = useState<string[] | null>(null)

  const toggleGroupExpand = useCallback((key: string) => {
    setExpandedGroups(prev => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  }, [])

  const { isSubmitting, handleSubmit: wrapSubmit } = useFormSubmit({
    onSuccess: () => {
      setPushTargetDialogOpen(false)
    },
    errorOperation: isEditing ? 'Update push target' : 'Create push target',
  })

  // Reset form when dialog opens/closes
  useEffect(() => {
    if (pushTargetDialogOpen) {
      if (editingPushTarget) {
        setName(editingPushTarget.name)
        setTargetType(editingPushTarget.target_type)
        setScheduleType(editingPushTarget.schedule.type)
        setIntervalSecs(editingPushTarget.schedule.interval_secs || 60)
        if (editingPushTarget.target_type === 'webhook') {
          setWebhookUrl(editingPushTarget.config?.url || '')
          if (editingPushTarget.config?.auth_token) {
            setWebhookAuthType('token')
            setWebhookAuthToken(editingPushTarget.config.auth_token)
          } else if (editingPushTarget.config?.auth_basic) {
            setWebhookAuthType('basic')
            setWebhookAuthUser(editingPushTarget.config.auth_basic.username || '')
            setWebhookAuthPass(editingPushTarget.config.auth_basic.password || '')
          }
        } else {
          setMqttBroker(editingPushTarget.config?.broker || '')
          setMqttTopic(editingPushTarget.config?.topic || '')
          setMqttPort(editingPushTarget.config?.port || 1883)
          setMqttUsername(editingPushTarget.config?.username || '')
          setMqttPassword(editingPushTarget.config?.password || '')
          setMqttQos(editingPushTarget.config?.qos ?? 1)
        }

        const patterns = editingPushTarget.data_filter.source_patterns
        if (patterns.length > 0) {
          // Store patterns for later expansion once sources are loaded
          setPendingPatterns(patterns)
        }

        // Batch config
        const bc = editingPushTarget.batch_config
        if (bc && bc.batch_size > 1) {
          setBatchEnabled(true)
          setBatchSize(bc.batch_size)
          setBatchIntervalMs(bc.batch_interval_ms || 1000)
          setBatchFormat(bc.format === 'nested' ? 'nested' : 'flat')
        }
      } else {
        resetForm()
      }
    }
  }, [pushTargetDialogOpen, editingPushTarget])

  // Fetch data sources + external brokers when dialog opens
  useEffect(() => {
    if (pushTargetDialogOpen) {
      setSourcesLoading(true)
      api.listUnifiedDataSources({ limit: 500, skip_telemetry: 'true' })
        .then(res => setSources(res.data || []))
        .catch(() => setSources([]))
        .finally(() => setSourcesLoading(false))
      api.getBrokers()
        .then(res => setBrokers(res.brokers || []))
        .catch(() => setBrokers([]))
    }
  }, [pushTargetDialogOpen])

  // Expand pending source patterns into selectedSources once sources are loaded
  useEffect(() => {
    if (!pendingPatterns || sources.length === 0) return
    const exact: string[] = []
    const prefix: string[] = []
    for (const p of pendingPatterns) {
      if (p.endsWith(':')) {
        prefix.push(p)
      } else {
        exact.push(p)
      }
    }
    // Expand prefix patterns: match all source IDs that start with the prefix
    for (const p of prefix) {
      for (const src of sources) {
        if (src.id.startsWith(p)) {
          exact.push(src.id)
        }
      }
    }
    setSelectedSources(new Set(exact))
    setPendingPatterns(null)
  }, [pendingPatterns, sources])

  const resetForm = () => {
    setName('')
    setNameError(null)
    setWebhookUrlError(null)
    setTargetType('webhook')
    setWebhookUrl('')
    setWebhookAuthType('none')
    setWebhookAuthToken('')
    setWebhookAuthUser('')
    setWebhookAuthPass('')
    setMqttBroker('')
    setMqttTopic('')
    setMqttPort(1883)
    setMqttUsername('')
    setMqttPassword('')
    setMqttQos(1)
    setMqttMode('select')
    setSelectedBrokerId('')
    setScheduleType('event_driven')
    setIntervalSecs(60)
    setSelectedSources(new Set())
    setSourceSearch('')
    setShowAdvanced(false)
    setManualPatterns('')
    setBatchEnabled(false)
    setBatchSize(10)
    setBatchIntervalMs(1000)
    setBatchFormat('flat')
    setPendingPatterns(null)
  }

  // Group sources by type → then by source_name
  const groupedSources = useMemo(() => {
    const groups = new Map<string, UnifiedDataSourceInfo[]>()
    const search = sourceSearch.toLowerCase()
    for (const src of sources) {
      if (search) {
        const match =
          src.id.toLowerCase().includes(search) ||
          src.source_display_name.toLowerCase().includes(search) ||
          src.field_display_name.toLowerCase().includes(search) ||
          src.source_type.toLowerCase().includes(search)
        if (!match) continue
      }
      const list = groups.get(src.source_type) || []
      list.push(src)
      groups.set(src.source_type, list)
    }
    return groups
  }, [sources, sourceSearch])

  const sourceGroupsByName = useMemo(() => {
    const groups = new Map<string, UnifiedDataSourceInfo[]>()
    for (const src of sources) {
      const key = `${src.source_type}:${src.source_name}`
      const list = groups.get(key) || []
      list.push(src)
      groups.set(key, list)
    }
    return groups
  }, [sources])

  const toggleSource = useCallback((id: string) => {
    setSelectedSources(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const toggleGroup = useCallback((sourceKey: string) => {
    const groupSources = sourceGroupsByName.get(sourceKey) || []
    const groupIds = groupSources.map(s => s.id)
    setSelectedSources(prev => {
      const next = new Set(prev)
      const allSelected = groupIds.every(id => next.has(id))
      if (allSelected) {
        for (const id of groupIds) next.delete(id)
      } else {
        for (const id of groupIds) next.add(id)
      }
      return next
    })
  }, [sourceGroupsByName])

  const clearAll = useCallback(() => setSelectedSources(new Set()), [])

  // Selected source items for display
  const selectedSourcesList = useMemo(() => {
    return sources.filter(s => selectedSources.has(s.id))
  }, [sources, selectedSources])

  const isValid = name.trim() && (targetType !== 'webhook' || webhookUrl.trim())

  const handleSave = useCallback(async () => {
    if (!name.trim()) {
      setNameError(t('common:dataPush.nameRequired', 'Name is required'))
      return
    }
    if (targetType === 'webhook' && !webhookUrl.trim()) {
      setWebhookUrlError(t('common:dataPush.urlRequired', 'URL is required'))
      return
    }

    // Resolve MQTT broker from selected external broker or manual input
    const resolvedMqttBroker = mqttMode === 'select' && selectedBrokerId
      ? brokers.find(b => b.id === selectedBrokerId)
      : null

    const config: Record<string, unknown> = targetType === 'webhook'
      ? {
          url: webhookUrl,
          method: 'POST',
          timeout_secs: 30,
          ...(webhookAuthType === 'token' && webhookAuthToken.trim()
            ? { auth_token: webhookAuthToken.trim() }
            : {}),
          ...(webhookAuthType === 'basic' && webhookAuthUser.trim()
            ? { auth_basic: { username: webhookAuthUser.trim(), password: webhookAuthPass } }
            : {}),
        }
      : {
          broker: resolvedMqttBroker ? resolvedMqttBroker.broker : mqttBroker,
          port: resolvedMqttBroker ? resolvedMqttBroker.port : mqttPort,
          topic: mqttTopic,
          qos: mqttQos,
          ...(resolvedMqttBroker?.username ? { username: resolvedMqttBroker.username } : mqttUsername.trim() ? { username: mqttUsername } : {}),
          ...(resolvedMqttBroker?.password ? { password: resolvedMqttBroker.password } : mqttPassword ? { password: mqttPassword } : {}),
        }

    const schedule = scheduleType === 'event_driven'
      ? { type: 'event_driven' as const, event_types: ['device_metric', 'extension_output'] }
      : { type: 'interval' as const, interval_secs: intervalSecs }

    let sourcePatterns: string[]
    if (showAdvanced && manualPatterns.trim()) {
      sourcePatterns = manualPatterns.split(',').map(s => s.trim()).filter(Boolean)
    } else {
      sourcePatterns = derivePatterns(selectedSources, sources)
    }

    const dataFilter = {
      source_patterns: sourcePatterns,
      only_changes: false,
    }

    const batchConfig = batchEnabled ? {
      batch_size: batchSize,
      batch_interval_ms: batchIntervalMs,
      format: batchFormat,
    } : undefined

    await wrapSubmit(async () => {
      if (isEditing && editingPushTarget) {
        const update: UpdatePushTargetRequest = {
          name,
          target_type: targetType,
          config,
          schedule,
          data_filter: dataFilter,
          batch_config: batchConfig,
        }
        const ok = await updatePushTarget(editingPushTarget.id, update)
        if (!ok) throw new Error('Update failed')
      } else {
        const create: CreatePushTargetRequest = {
          name,
          target_type: targetType,
          config,
          schedule,
          data_filter: dataFilter,
          batch_config: batchConfig,
        }
        const ok = await createPushTarget(create)
        if (!ok) throw new Error('Create failed')
      }
    })()
  }, [wrapSubmit, isEditing, editingPushTarget, name, targetType, webhookUrl, webhookAuthType, webhookAuthToken, webhookAuthUser, webhookAuthPass, mqttBroker, mqttTopic, mqttPort, mqttUsername, mqttPassword, mqttQos, mqttMode, selectedBrokerId, brokers, scheduleType, intervalSecs, showAdvanced, manualPatterns, selectedSources, sources, batchEnabled, batchSize, batchIntervalMs, batchFormat, createPushTarget, updatePushTarget, t])

  return (
    <FullScreenDialog open={pushTargetDialogOpen} onOpenChange={setPushTargetDialogOpen}>
      <FullScreenDialogHeader
        icon={<Send className="h-5 w-5" />}
        iconBg="bg-accent-orange-light"
        iconColor="text-accent-orange"
        title={isEditing
          ? t('common:dataPush.editTarget', 'Edit Push Target')
          : t('common:dataPush.createTarget', 'Create Push Target')}
        onClose={() => setPushTargetDialogOpen(false)}
      />

      <FullScreenDialogContent>
        <FullScreenDialogMain className="overflow-hidden">
          <div className="h-full overflow-y-auto">
            <div className={cn("space-y-6", isMobile ? "px-4 py-6" : "px-4 py-6")}>

              {/* ── Target Type ── */}
              <div className="space-y-2">
                <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                  {t('common:dataPush.targetType', 'Target Type')}
                </Label>
                <div className={cn("gap-3", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                  {(['webhook', 'mqtt'] as PushTargetType[]).map((type) => {
                    const isActive = targetType === type
                    const Icon = type === 'webhook' ? Send : Server
                    return (
                      <button
                        key={type}
                        type="button"
                        onClick={() => setTargetType(type)}
                        className={cn(
                          "relative flex flex-col items-start gap-1.5 rounded-lg border-2 p-3 text-left transition-all",
                          isActive
                            ? "border-primary bg-muted shadow-sm"
                            : "border-border hover:border-border"
                        )}
                      >
                        <div className="flex items-center gap-2 w-full">
                          <div className={cn(
                            "h-8 w-8 rounded-lg flex items-center justify-center shrink-0",
                            isActive ? "bg-primary text-primary-foreground" : "bg-muted"
                          )}>
                            <Icon className="h-4 w-4" />
                          </div>
                          <span className="text-sm font-medium">{type.toUpperCase()}</span>
                        </div>
                        <p className={cn(textMini, "text-muted-foreground pl-10")}>
                          {type === 'webhook'
                            ? t('common:dataPush.webhookUrl', 'Send data via HTTP POST')
                            : t('common:dataPush.mqttBroker', 'Publish to MQTT topic')}
                        </p>
                      </button>
                    )
                  })}
                </div>
              </div>

              {/* ── Name ── */}
              <div className="space-y-2">
                <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                  {t('common:dataPush.name', 'Name')} <span className="text-error">*</span>
                </Label>
                <Input
                  value={name}
                  onChange={(e) => { setName(e.target.value); setNameError(null) }}
                  placeholder={t('common:dataPush.namePlaceholder', 'My Push Target')}
                  className={cn(isMobile ? "h-12 text-base" : "h-10", nameError && "border-error")}
                />
                {nameError && <p className="text-sm text-error mt-1">{nameError}</p>}
              </div>

              {/* ── Target Config ── */}
              {targetType === 'webhook' ? (
                <div className="space-y-3">
                  <div className="space-y-2">
                    <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                      {t('common:dataPush.webhookUrl', 'Webhook URL')} <span className="text-error">*</span>
                    </Label>
                    <Input
                      type="url"
                      value={webhookUrl}
                      onChange={(e) => { setWebhookUrl(e.target.value); setWebhookUrlError(null) }}
                      placeholder="https://example.com/webhook"
                      className={cn(isMobile ? "h-12 text-base" : "h-10", webhookUrlError && "border-error")}
                    />
                    {webhookUrlError && <p className="text-sm text-error mt-1">{webhookUrlError}</p>}
                  </div>

                  {/* Auth type selector */}
                  <div className="space-y-2">
                    <Label className={cn("font-medium flex items-center gap-1.5", isMobile ? "text-base" : "text-sm")}>
                      <KeyRound className="h-4 w-4 text-muted-foreground" />
                      {t('common:dataPush.webhookAuthType', 'Authentication')}
                    </Label>
                    <Select value={webhookAuthType} onValueChange={(v) => setWebhookAuthType(v as 'none' | 'token' | 'basic')}>
                      <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="none">{t('common:dataPush.webhookNoAuth', 'None')}</SelectItem>
                        <SelectItem value="token">{t('common:dataPush.webhookAuthToken', 'Bearer Token')}</SelectItem>
                        <SelectItem value="basic">{t('common:dataPush.webhookBasicAuth', 'Basic Auth')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  {/* Bearer Token */}
                  {webhookAuthType === 'token' && (
                    <Input
                      value={webhookAuthToken}
                      onChange={(e) => setWebhookAuthToken(e.target.value)}
                      placeholder={t('common:dataPush.webhookAuthTokenPlaceholder', 'Authorization token')}
                      className={isMobile ? "h-12 text-base" : "h-10"}
                    />
                  )}

                  {/* Basic Auth */}
                  {webhookAuthType === 'basic' && (
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <Input
                        value={webhookAuthUser}
                        onChange={(e) => setWebhookAuthUser(e.target.value)}
                        placeholder={t('common:dataPush.webhookUsername', 'Username')}
                        className={isMobile ? "h-12 text-base" : "h-10"}
                      />
                      <PasswordInput
                        value={webhookAuthPass}
                        onChange={(e) => setWebhookAuthPass(e.target.value)}
                        placeholder={t('common:dataPush.webhookPassword', 'Password')}
                        className={isMobile ? "h-12 text-base" : "h-10"}
                      />
                    </div>
                  )}
                </div>
              ) : (
                <div className="space-y-3">
                  {/* MQTT mode: select broker or manual */}
                  <Select value={mqttMode} onValueChange={(v) => setMqttMode(v as 'select' | 'manual')}>
                    <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="select">
                        <span className="flex items-center gap-2">
                          <Link2 className="h-4 w-4" />
                          {t('common:dataPush.mqttSelectBroker', 'Select broker')}
                        </span>
                      </SelectItem>
                      <SelectItem value="manual">
                        {t('common:dataPush.mqttManualInput', 'Manual input')}
                      </SelectItem>
                    </SelectContent>
                  </Select>

                  {mqttMode === 'select' ? (
                    <div className="space-y-2">
                      <Select value={selectedBrokerId} onValueChange={(id) => {
                        setSelectedBrokerId(id)
                        const b = brokers.find(br => br.id === id)
                        if (b) {
                          setMqttBroker(b.broker)
                          setMqttPort(b.port)
                          setMqttUsername(b.username || '')
                          setMqttPassword(b.password || '')
                        }
                      }}>
                        <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                          <SelectValue placeholder={t('common:dataPush.mqttSelectBroker')} />
                        </SelectTrigger>
                        <SelectContent>
                          {brokers.length === 0 ? (
                            <SelectItem value="_none" disabled>
                              {t('common:dataPush.sourceEmpty', 'No brokers found')}
                            </SelectItem>
                          ) : (
                            brokers.map(b => (
                              <SelectItem key={b.id} value={b.id}>
                                <span className="flex items-center gap-2">
                                  <span className="truncate">{b.name}</span>
                                  <span className={cn(textNano, "text-muted-foreground")}>{b.broker}:{b.port}</span>
                                </span>
                              </SelectItem>
                            ))
                          )}
                        </SelectContent>
                      </Select>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                        <Input
                          value={mqttBroker}
                          onChange={(e) => setMqttBroker(e.target.value)}
                          placeholder="broker.example.com"
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                        <Input
                          type="number"
                          value={mqttPort}
                          onChange={(e) => setMqttPort(Number(e.target.value))}
                          placeholder={t('common:dataPush.mqttPort', 'Port')}
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                      </div>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                        <Input
                          value={mqttUsername}
                          onChange={(e) => setMqttUsername(e.target.value)}
                          placeholder={t('common:dataPush.mqttUsername', 'Username')}
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                        <PasswordInput
                          value={mqttPassword}
                          onChange={(e) => setMqttPassword(e.target.value)}
                          placeholder={t('common:dataPush.mqttPassword', 'Password')}
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                      </div>
                    </div>
                  )}

                  {/* Topic + QoS — always shown */}
                  <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                    <div className="sm:col-span-2">
                      <Input
                        value={mqttTopic}
                        onChange={(e) => setMqttTopic(e.target.value)}
                        placeholder="neomind/data"
                        className={isMobile ? "h-12 text-base" : "h-10"}
                      />
                    </div>
                    <Select value={String(mqttQos)} onValueChange={(v) => setMqttQos(Number(v))}>
                      <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="0">QoS 0</SelectItem>
                        <SelectItem value="1">QoS 1</SelectItem>
                        <SelectItem value="2">QoS 2</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              )}

              {/* ── Schedule ── */}
              <div className="space-y-2">
                <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                  {t('common:dataPush.schedule', 'Schedule')}
                </Label>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                  <Select value={scheduleType} onValueChange={(v) => setScheduleType(v as 'event_driven' | 'interval')}>
                    <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="event_driven">{t('common:dataPush.eventDriven')}</SelectItem>
                      <SelectItem value="interval">{t('common:dataPush.intervalSchedule')}</SelectItem>
                    </SelectContent>
                  </Select>
                  {scheduleType === 'interval' && (
                    <Input
                      type="number"
                      value={intervalSecs}
                      onChange={(e) => setIntervalSecs(Number(e.target.value))}
                      min={1}
                      placeholder={t('common:dataPush.intervalSecs')}
                      className={isMobile ? "h-12 text-base" : "h-10"}
                    />
                  )}
                </div>
              </div>

              {/* ── Batch / Aggregation ── */}
              <div className="space-y-2">
                <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                  {t('common:dataPush.batchConfig', 'Batch Aggregation')}
                </Label>
                <p className={cn(textMini, "text-muted-foreground")}>
                  {t('common:dataPush.batchConfigDesc', 'Aggregate multiple events into a single payload to reduce requests')}
                </p>
                <Select value={batchEnabled ? 'batched' : 'immediate'} onValueChange={(v) => setBatchEnabled(v === 'batched')}>
                  <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="immediate">{t('common:dataPush.batchOff', 'Immediate (per event)')}</SelectItem>
                    <SelectItem value="batched">{t('common:dataPush.batchOn', 'Batched (aggregate)')}</SelectItem>
                  </SelectContent>
                </Select>
                {batchEnabled && (
                  <div className="space-y-2 mt-2">
                    <div className="space-y-1.5">
                      <Label className={cn(textMini, "font-medium text-muted-foreground")}>
                        {t('common:dataPush.batchFormat', 'Payload Format')}
                      </Label>
                      <Select value={batchFormat} onValueChange={(v) => setBatchFormat(v as 'flat' | 'nested')}>
                        <SelectTrigger className={isMobile ? "h-12 text-base" : "h-10"}>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="flat">{t('common:dataPush.batchFormatFlat', 'Flat (item list)')}</SelectItem>
                          <SelectItem value="nested">{t('common:dataPush.batchFormatNested', 'Nested (by device/source)')}</SelectItem>
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <div className="space-y-1.5">
                        <Label className={cn(textMini, "font-medium text-muted-foreground")}>
                          {t('common:dataPush.batchSize', 'Batch Size')}
                        </Label>
                        <Input
                          type="number"
                          value={batchSize}
                          onChange={(e) => setBatchSize(Number(e.target.value))}
                          min={2}
                          max={1000}
                          placeholder="50"
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                      </div>
                      <div className="space-y-1.5">
                        <Label className={cn(textMini, "font-medium text-muted-foreground")}>
                          {t('common:dataPush.batchInterval', 'Max Interval (ms)')}
                        </Label>
                        <Input
                          type="number"
                          value={batchIntervalMs}
                          onChange={(e) => setBatchIntervalMs(Number(e.target.value))}
                          min={100}
                          max={60000}
                          placeholder="2000"
                          className={isMobile ? "h-12 text-base" : "h-10"}
                        />
                      </div>
                    </div>
                    <p className={cn(textMini, "text-muted-foreground")}>
                      {t('common:dataPush.batchFieldsHint', 'Flushes when batch size or max interval is reached, whichever comes first')}
                    </p>
                  </div>
                )}
              </div>

              {/* ── Data Source Selection ── */}
              <div className="space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Label className={cn("font-medium", isMobile ? "text-base" : "text-sm")}>
                      {t('common:dataPush.sourcePicker', 'Data Source Selection')}
                    </Label>
                    {selectedSourcesList.length > 0 && (
                      <Badge variant="secondary" className={cn(textNano, "h-5 px-1.5")}>
                        {selectedSourcesList.length}
                      </Badge>
                    )}
                  </div>
                  {selectedSources.size > 0 && (
                    <Button variant="ghost" size="xs" className="text-muted-foreground" onClick={clearAll}>
                      {t('common:dataPush.deselectAllFields')}
                    </Button>
                  )}
                </div>

                {/* Search */}
                <div className="relative">
                  <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <Input
                    value={sourceSearch}
                    onChange={(e) => setSourceSearch(e.target.value)}
                    placeholder={t('common:dataPush.sourceSearch')}
                    className={cn("pl-9", isMobile ? "h-12 text-base" : "h-10")}
                  />
                </div>

                {/* Source list */}
                <div className="border rounded-lg overflow-hidden">
                  {sourcesLoading ? (
                    <div className="flex items-center justify-center py-8 text-muted-foreground">
                      <Loader2 className="h-4 w-4 animate-spin mr-2" />
                      {t('common:dataPush.sourceLoading')}
                    </div>
                  ) : groupedSources.size === 0 ? (
                    <div className="flex items-center justify-center py-8 text-muted-foreground">
                      {t('common:dataPush.sourceEmpty')}
                    </div>
                  ) : (
                    Array.from(groupedSources.entries()).map(([sourceType, items]) => {
                      const TypeIcon = SOURCE_TYPE_ICONS[sourceType] || Activity
                      const badgeClass = SOURCE_TYPE_BADGE[sourceType] || 'bg-muted text-muted-foreground'
                      const typeExpanded = expandedGroups.has(sourceType)

                      // Count selected in this type
                      const selectedInType = items.filter(s => selectedSources.has(s.id)).length

                      // Sub-group by source_name
                      const subGroups = new Map<string, UnifiedDataSourceInfo[]>()
                      for (const item of items) {
                        const key = `${item.source_type}:${item.source_name}`
                        const list = subGroups.get(key) || []
                        list.push(item)
                        subGroups.set(key, list)
                      }

                      return (
                        <div key={sourceType} className="border-b last:border-b-0">
                          {/* Type header - collapsible */}
                          <button
                            onClick={() => toggleGroupExpand(sourceType)}
                            className="w-full flex items-center gap-2 px-3 py-2.5 text-left hover:bg-muted-30 transition-colors"
                          >
                            {typeExpanded
                              ? <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
                              : <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0" />
                            }
                            <div className={cn(
                              'w-6 h-6 rounded flex items-center justify-center shrink-0',
                              badgeClass
                            )}>
                              <TypeIcon className="h-3.5 w-3.5" />
                            </div>
                            <span className="text-sm font-semibold">{t(SOURCE_TYPE_KEYS[sourceType] || sourceType)}</span>
                            <span className={cn(textMini, 'text-muted-foreground')}>
                              {selectedInType > 0
                                ? t('common:dataPush.sourceSelected', { count: `${selectedInType}/${items.length}` })
                                : `(${items.length})`
                              }
                            </span>
                          </button>

                          {/* Expanded content */}
                          {typeExpanded && (
                            <div className="pl-6">
                              {Array.from(subGroups.entries()).map(([sourceKey, sourceItems]) => {
                                const subExpanded = expandedGroups.has(sourceKey)
                                const allSelected = sourceItems.every(s => selectedSources.has(s.id))
                                const someSelected = sourceItems.some(s => selectedSources.has(s.id))
                                const displayName = sourceItems[0]?.source_display_name || sourceKey

                                return (
                                  <div key={sourceKey} className="border-t first:border-t-0 border-l">
                                    {/* Source row: checkbox + expand/collapse */}
                                    <div className="flex items-center">
                                      <button
                                        onClick={(e) => { e.stopPropagation(); toggleGroup(sourceKey) }}
                                        className={cn(
                                          'flex items-center gap-2 px-3 py-2 text-left hover:bg-muted-30 transition-colors flex-1 min-w-0',
                                          someSelected && !allSelected && 'bg-primary-light'
                                        )}
                                      >
                                        <Checkbox checked={allSelected} />
                                        <span className="text-body font-medium truncate">{displayName}</span>
                                        {someSelected && (
                                          <Badge variant="secondary" className={cn(textNano, "h-4 px-1 shrink-0")}>
                                            {sourceItems.filter(s => selectedSources.has(s.id)).length}
                                          </Badge>
                                        )}
                                      </button>
                                      <button
                                        onClick={() => toggleGroupExpand(sourceKey)}
                                        className="inline-flex items-center justify-center h-8 w-8 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors shrink-0"
                                        aria-label={subExpanded ? 'Collapse' : 'Expand'}
                                      >
                                        {subExpanded
                                          ? <ChevronDown className="h-4 w-4 text-muted-foreground" />
                                          : <ChevronRight className="h-4 w-4 text-muted-foreground" />
                                        }
                                      </button>
                                    </div>

                                    {/* Fields (expanded) */}
                                    {subExpanded && (
                                      <div className="pl-7 pr-3 pb-1">
                                        {sourceItems.map(src => (
                                          <button
                                            key={src.id}
                                            onClick={() => toggleSource(src.id)}
                                            className={cn(
                                              'w-full flex items-center gap-2 px-2 py-1.5 rounded-md text-left hover:bg-muted-30 transition-colors',
                                              selectedSources.has(src.id) && 'bg-primary-light'
                                            )}
                                          >
                                            <Checkbox checked={selectedSources.has(src.id)} />
                                            <span className="text-xs truncate">{src.field_display_name}</span>
                                            {src.data_type && (
                                              <span className={cn(
                                                'ml-auto px-1.5 py-0.5 rounded text-nano font-mono shrink-0',
                                                badgeClass
                                              )}>
                                                {src.data_type}
                                              </span>
                                            )}
                                            {src.unit && (
                                              <span className={cn(textNano, 'text-muted-foreground shrink-0')}>
                                                {src.unit}
                                              </span>
                                            )}
                                          </button>
                                        ))}
                                      </div>
                                    )}
                                  </div>
                                )
                              })}
                            </div>
                          )}
                        </div>
                      )
                    })
                  )}
                </div>

                <p className={cn(textMini, "text-muted-foreground")}>
                  {t('common:dataPush.sourcePickerDesc')}
                </p>

                {/* Selected badges */}
                {selectedSourcesList.length > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {selectedSourcesList.map(src => {
                      const SrcIcon = SOURCE_TYPE_ICONS[src.source_type] || Activity
                      return (
                        <span
                          key={src.id}
                          className="inline-flex items-center gap-1 px-2 py-1 rounded-md bg-muted text-xs font-medium"
                        >
                          <SrcIcon className="h-3 w-3" />
                          <span className="max-w-[120px] truncate">{src.source_display_name}</span>
                          <span className="text-muted-foreground">:</span>
                          <span className="max-w-[80px] truncate">{src.field_display_name}</span>
                          <button
                            onClick={() => toggleSource(src.id)}
                            className="ml-0.5 inline-flex items-center justify-center h-5 w-5 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                            aria-label={t('common:dataPush.removeSource')}
                          >
                            <X className="h-3 w-3" aria-hidden="true" />
                          </button>
                        </span>
                      )
                    })}
                  </div>
                )}

                {/* Advanced: manual pattern input */}
                <div>
                  <button
                    onClick={() => setShowAdvanced(!showAdvanced)}
                    className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
                  >
                    <ChevronRight className={cn('h-4 w-4 transition-transform', showAdvanced && 'rotate-90')} />
                    {t('common:dataPush.sourceAdvanced')}
                  </button>
                  {showAdvanced && (
                    <div className="mt-2">
                      <Input
                        value={manualPatterns}
                        onChange={(e) => setManualPatterns(e.target.value)}
                        placeholder="device:sensor1:, extension:weather:"
                        className={isMobile ? "h-12 text-base" : "h-10"}
                      />
                      <p className={cn(textMini, "text-muted-foreground mt-1")}>
                        {t('common:dataPush.sourcePatternsHint')}
                      </p>
                    </div>
                  )}
                </div>
              </div>

            </div>
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>

      {/* Footer */}
      <FullScreenDialogFooter className="flex-col items-stretch">
        <div className="flex gap-2 justify-end">
          <Button
            variant="outline"
            size={isMobile ? "default" : "sm"}
            onClick={() => setPushTargetDialogOpen(false)}
            disabled={isSubmitting}
            className={isMobile ? "min-w-[100px] h-12" : ""}
          >
            {t('common:cancel', 'Cancel')}
          </Button>
          <Button
            size={isMobile ? "default" : "sm"}
            onClick={handleSave}
            disabled={!isValid || isSubmitting}
            className={isMobile ? "min-w-[100px] h-12" : ""}
          >
            {isSubmitting ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : null}
            {isEditing ? t('common:save', 'Save') : t('common:create', 'Create')}
          </Button>
        </div>
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}

/**
 * InstanceManagerDialog - Full-screen dialog for switching + managing instances
 *
 * All instances shown as large, tappable cards. Click any card to switch.
 * Edit/Delete/Test available as secondary actions on each card.
 */

import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Badge } from '@/components/ui/badge'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from '@/components/automation/dialog/FullScreenDialog'
import {
  UnifiedFormDialog,
} from '@/components/dialog/UnifiedFormDialog'
import {
  Plus,
  Pencil,
  Trash2,
  Wifi,
  WifiOff,
  Loader2,
  Server,
  Globe,
  Check,
  ShieldCheck,
  AlertCircle,
  Info,
} from 'lucide-react'
import { confirm as confirmDialog } from '@/components/ui/use-confirm'
import type { InstanceInfo } from '@/store/slices/instanceSlice'

interface InstanceManagerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function InstanceManagerDialog({ open, onOpenChange }: InstanceManagerDialogProps) {
  const { t } = useTranslation('instances')
  const instances = useStore((s) => s.instances)
  const fetchInstances = useStore((s) => s.fetchInstances)
  const addInstance = useStore((s) => s.addInstance)
  const updateInstance = useStore((s) => s.updateInstance)
  const deleteInstance = useStore((s) => s.deleteInstance)
  const testInstance = useStore((s) => s.testInstance)
  const currentInstanceId = useStore((s) => s.currentInstanceId)
  const switchInstance = useStore((s) => s.switchInstance)
  const switchingState = useStore((s) => s.switchingState)
  const isRemote = useStore((s) => s.currentInstanceId !== 'local-default')

  const [formOpen, setFormOpen] = useState(false)
  const [editingInstance, setEditingInstance] = useState<InstanceInfo | null>(null)
  const [testingId, setTestingId] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; latency_ms?: number; error?: string }>>({})

  useEffect(() => {
    if (open) fetchInstances()
  }, [open, fetchInstances])

  // Form state
  const [formName, setFormName] = useState('')
  const [formUrl, setFormUrl] = useState('')
  const [formApiKey, setFormApiKey] = useState('')
  const [clearApiKey, setClearApiKey] = useState(false)
  const [formErrors, setFormErrors] = useState<Record<string, string>>({})
  const [apiKeyValidation, setApiKeyValidation] = useState<'idle' | 'testing' | 'valid' | 'invalid'>('idle')

  const openAddForm = (e?: React.MouseEvent) => {
    e?.stopPropagation()
    setEditingInstance(null)
    setFormName('')
    setFormUrl('')
    setFormApiKey('')
    setFormErrors({})
    setApiKeyValidation('idle')
    setClearApiKey(false)
    setFormOpen(true)
  }

  const openEditForm = (e: React.MouseEvent, instance: InstanceInfo) => {
    e.stopPropagation()
    setEditingInstance(instance)
    setFormName(instance.name)
    setFormUrl(instance.url)
    // Don't prefill masked key — show empty, user must re-enter to change
    setFormApiKey('')
    setFormErrors({})
    setApiKeyValidation('idle')
    setClearApiKey(false)
    setFormOpen(true)
  }

  const validateForm = () => {
    const errors: Record<string, string> = {}
    if (!formName.trim()) errors.name = t('validation.nameRequired')
    if (!formUrl.trim()) errors.url = t('validation.urlRequired')
    else if (!formUrl.startsWith('http://') && !formUrl.startsWith('https://')) {
      errors.url = t('validation.urlInvalid')
    }
    setFormErrors(errors)
    return Object.keys(errors).length === 0
  }

  const handleSave = async () => {
    if (!validateForm()) return

    // If API key is provided, validate it against the remote instance first
    if (formApiKey.trim()) {
      setApiKeyValidation('testing')
      try {
        const apiUrl = formUrl.replace(/\/+$/, '') + '/api'
        const res = await fetch(`${apiUrl}/auth/verify`, {
          headers: { 'X-API-Key': formApiKey.trim() },
          signal: AbortSignal.timeout(5000),
        })
        if (res.ok) {
          setApiKeyValidation('valid')
        } else {
          setApiKeyValidation('invalid')
          setFormErrors({ apiKey: t('apiKeyInvalid.description') })
          return
        }
      } catch {
        // Network error — can't verify, allow saving
        setApiKeyValidation('idle')
      }
    } else {
      setApiKeyValidation('idle')
    }

    if (editingInstance) {
      const updateData: { name: string; url: string; api_key?: string } = {
        name: formName,
        url: formUrl,
      }
      if (clearApiKey) {
        // Explicitly clear the API key
        updateData.api_key = ''
      } else if (formApiKey.trim()) {
        // New key entered — update it
        updateData.api_key = formApiKey.trim()
      }
      // else: no change, don't send api_key field
      await updateInstance(editingInstance.id, updateData)
    } else {
      await addInstance({
        name: formName,
        url: formUrl,
        api_key: formApiKey.trim() || undefined,
      })
    }
    setFormOpen(false)
  }

  const handleDelete = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    if (await confirmDialog({ title: t('delete'), description: t('deleteDesc', { defaultValue: `Are you sure you want to delete this instance? This action cannot be undone.` }), confirmText: t('delete'), variant: 'destructive' })) {
      await deleteInstance(id)
    }
  }

  const handleTest = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    setTestingId(id)
    try {
      const result = await testInstance(id)
      setTestResults((prev) => ({ ...prev, [id]: result }))
    } catch (err) {
      setTestResults((prev) => ({
        ...prev,
        [id]: { success: false, error: err instanceof Error ? err.message : 'Unknown error' },
      }))
    }
    setTestingId(null)
    fetchInstances()
  }

  const handleSwitch = async (instance: InstanceInfo) => {
    if (instance.id === currentInstanceId) return
    if (switchingState === 'switching') return

    const confirmed = await confirmDialog({
      title: t('switch.title'),
      description: t('switch.connecting', { name: instance.name }),
      confirmText: t('switchTo'),
    })
    if (!confirmed) return

    switchInstance(instance.id)
  }

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'online':
        return 'bg-success'
      case 'offline':
        return 'bg-destructive'
      default:
        return 'bg-muted-foreground'
    }
  }

  return (
    <>
      <FullScreenDialog open={open} onOpenChange={onOpenChange}>
        <FullScreenDialogHeader
          icon={<Server className="h-5 w-5" />}
          title={t('title')}
          subtitle={t('description')}
          onClose={() => onOpenChange(false)}
        />
        <FullScreenDialogContent>
          <FullScreenDialogMain className="max-w-2xl mx-auto p-6">
            {/* Remote instance hint banner */}
            {isRemote && (
              <div className="flex items-start gap-3 rounded-lg border border-info-light bg-info-light p-3 mb-4">
                <Info className="h-4 w-4 text-info mt-0.5 shrink-0" />
                <p className="text-xs text-muted-foreground leading-relaxed">
                  {t('remoteHint')}
                </p>
              </div>
            )}
            <div className="space-y-3">
              {instances.map((instance) => {
                const isCurrent = instance.id === currentInstanceId
                const isLocal = instance.is_local

                return (
                  <div
                    key={instance.id}
                    role="button"
                    tabIndex={0}
                    onClick={() => handleSwitch(instance)}
                    onKeyDown={(e) => e.key === 'Enter' && handleSwitch(instance)}
                    className={cn(
                      'relative rounded-xl border p-4 transition-all',
                      isCurrent
                        ? 'border-primary ring-2 ring-primary bg-primary-light'
                        : 'border-border hover:bg-muted-30 hover:shadow-sm cursor-pointer',
                    )}
                  >
                    <div className="flex items-center gap-4">
                      {/* Icon */}
                      <div className={cn(
                        'w-11 h-11 rounded-xl flex items-center justify-center shrink-0',
                        isCurrent ? 'bg-primary-light' : 'bg-muted',
                      )}>
                        {isLocal ? (
                          <Server className={cn('h-5 w-5', isCurrent ? 'text-primary' : 'text-muted-foreground')} />
                        ) : (
                          <Globe className={cn('h-5 w-5', isCurrent ? 'text-primary' : 'text-muted-foreground')} />
                        )}
                      </div>

                      {/* Name + URL + status */}
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className={cn('font-medium text-sm truncate', isCurrent && 'text-primary')}>
                            {instance.name}
                          </span>
                          {isCurrent && (
                            <Badge variant="outline" className="text-xs shrink-0 border-primary text-primary">
                              <Check className="h-3 w-3 mr-0.5" />
                              {t('current')}
                            </Badge>
                          )}
                          {isLocal && !isCurrent && (
                            <Badge variant="outline" className="text-xs shrink-0">{t('localBadge')}</Badge>
                          )}
                        </div>
                        <div className="flex items-center gap-2 mt-0.5">
                          <span className="text-xs text-muted-foreground truncate">{instance.url}</span>
                          <div className={cn('w-1.5 h-1.5 rounded-full shrink-0', getStatusColor(instance.last_status))} />
                        </div>
                        {/* Test result */}
                        {testResults[instance.id] && (
                          <p className={cn('text-xs mt-0.5', testResults[instance.id].success ? 'text-success' : 'text-error')}>
                            {testResults[instance.id].success
                              ? t('testSuccess', { latency: testResults[instance.id].latency_ms })
                              : t('testFailed', { error: testResults[instance.id].error })}
                          </p>
                        )}
                      </div>

                      {/* Right side: status or management actions */}
                      <div className="flex items-center gap-1.5 shrink-0">
                        {isCurrent ? (
                          <Wifi className="h-4 w-4 text-success" />
                        ) : (
                          <>
                            {/* Management actions — only available on local instance */}
                            {!isLocal && !isRemote && (
                              <>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8 sm:h-8 sm:w-8"
                                  disabled={testingId === instance.id}
                                  onClick={(e) => handleTest(e, instance.id)}
                                >
                                  {testingId === instance.id ? (
                                    <Loader2 className="h-4 w-4 animate-spin" />
                                  ) : instance.last_status === 'online' ? (
                                    <Wifi className="h-4 w-4 text-success" />
                                  ) : (
                                    <WifiOff className="h-4 w-4 text-error" />
                                  )}
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8 sm:h-8 sm:w-8"
                                  onClick={(e) => openEditForm(e, instance)}
                                >
                                  <Pencil className="h-4 w-4" />
                                </Button>
                                <Button
                                  variant="ghost"
                                  size="icon"
                                  className="h-8 w-8 text-error sm:h-8 sm:w-8"
                                  onClick={(e) => handleDelete(e, instance.id)}
                                >
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </>
                            )}
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                )
              })}

              {/* Add Instance button — only on local instance */}
              {!isRemote && (
              <button
                onClick={openAddForm}
                className="w-full rounded-xl border-2 border-dashed border-border p-4 text-sm text-muted-foreground hover:border-primary hover:text-primary transition-colors cursor-pointer"
              >
                <Plus className="h-5 w-5 mx-auto mb-1" />
                {t('add')}
              </button>
              )}
            </div>
          </FullScreenDialogMain>
        </FullScreenDialogContent>
      </FullScreenDialog>

      {/* Add/Edit Form Dialog */}
      <UnifiedFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        title={editingInstance ? t('edit') : t('add')}
        onSubmit={handleSave}
        submitLabel={editingInstance ? t('edit') : t('add')}
        className="z-[110]"
      >
        <div className="space-y-4">
          <div className="space-y-2">
            <Label>{t('name')} <span className="text-error">*</span></Label>
            <Input
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              placeholder={t('namePlaceholder')}
            />
            {formErrors.name && <p className="text-xs text-error">{formErrors.name}</p>}
          </div>
          <div className="space-y-2">
            <Label>{t('url')} <span className="text-error">*</span></Label>
            <Input
              value={formUrl}
              onChange={(e) => setFormUrl(e.target.value)}
              placeholder={t('urlPlaceholder')}
            />
            {!formErrors.url && <p className="text-xs text-muted-foreground">{t('urlHint')}</p>}
            {formErrors.url && <p className="text-xs text-error">{formErrors.url}</p>}
          </div>
          <div className="space-y-2">
            <Label className="flex items-center gap-1.5">
              {t('apiKey')}
              <span className="text-xs text-muted-foreground font-normal">({t('optional')})</span>
            </Label>
            {editingInstance && editingInstance.api_key && !clearApiKey && !formApiKey ? (
              <div className="flex items-center gap-2">
                <div className="flex-1 rounded-md border border-border px-3 py-2 text-xs text-muted-foreground bg-muted">
                  {editingInstance.api_key}
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => setClearApiKey(true)}
                  className="text-error shrink-0"
                >
                  {t('apiKeyClear', { ns: 'instances' }) || t('delete')}
                </Button>
              </div>
            ) : (
              <div className="relative">
                <Input
                  type="password"
                  value={formApiKey}
                  onChange={(e) => { setFormApiKey(e.target.value); setApiKeyValidation('idle'); setClearApiKey(false) }}
                  placeholder={editingInstance ? t('apiKeyEditPlaceholder') : t('apiKeyPlaceholder')}
                  className="pr-8"
                />
                {apiKeyValidation === 'valid' && (
                  <ShieldCheck className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-success" />
                )}
                {apiKeyValidation === 'invalid' && (
                  <AlertCircle className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 text-error" />
                )}
                {apiKeyValidation === 'testing' && (
                  <Loader2 className="absolute right-2.5 top-1/2 -translate-y-1/2 h-4 w-4 animate-spin text-muted-foreground" />
                )}
              </div>
            )}
            <p className="text-xs text-muted-foreground">{t('apiKeyHint')}</p>
          </div>
        </div>
      </UnifiedFormDialog>
    </>
  )
}

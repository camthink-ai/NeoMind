import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { Button, IconButton } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { Play, Edit, Trash2 } from 'lucide-react'
import { api } from '@/lib/api'
import { confirm } from '@/hooks/use-confirm'
import { TransformBuilder as TransformBuilderSplit } from './TransformBuilderSplit'
import { TransformTestDialog } from './TransformTestDialog'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import type { TransformAutomation, TransformScope } from '@/types'

interface DeviceTransformsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceId?: string
  deviceTypeId?: string
  deviceName?: string
  onTransformCreated?: () => void
}

export function DeviceTransformsDialog({
  open,
  onOpenChange,
  deviceId,
  deviceTypeId,
  deviceName,
  onTransformCreated,
}: DeviceTransformsDialogProps) {
  const { t } = useTranslation('automation')
  const { handleError } = useErrorHandler()

  const [transforms, setTransforms] = useState<TransformAutomation[]>([])
  const [loading, setLoading] = useState(true)
  const [builderOpen, setBuilderOpen] = useState(false)
  const [testDialogOpen, setTestDialogOpen] = useState(false)
  const [editingTransform, setEditingTransform] = useState<TransformAutomation | null>(null)
  const [testingTransformId, setTestingTransformId] = useState<string | null>(null)
  const [devices, setDevices] = useState<Array<{ id: string; name: string; device_type?: string }>>([])
  const [deviceTypes, setDeviceTypes] = useState<Array<{ device_type: string; name?: string }>>([])

  const fetchDevices = async () => {
    try {
      const result = await api.getDevices()
      setDevices((result.devices || []).map((d: any) => ({
        id: d.id,
        name: d.name,
        device_type: d.device_type,
      })))
    } catch {
      // ignore
    }
  }

  const fetchDeviceTypes = async () => {
    try {
      const result = await api.getDeviceTypes()
      setDeviceTypes((result.device_types || []).map((dt: any) => ({
        device_type: dt.device_type,
        name: dt.name,
      })))
    } catch {
      // ignore
    }
  }

  // Only fetch devices/deviceTypes when dialog opens (not on mount)
  // This prevents N×3 API calls when multiple TransformsBadge are rendered in a list
  useEffect(() => {
    if (open) {
      fetchDevices()
      fetchDeviceTypes()
    }
  }, [open])

  const fetchTransforms = async () => {
    setLoading(true)
    try {
      const result = await api.listTransforms()
      let filtered = result.transforms || []

      if (deviceId) {
        filtered = filtered.filter((tr) =>
          typeof tr.scope === 'object' && 'device' in tr.scope && tr.scope.device === deviceId
        )
      } else if (deviceTypeId) {
        filtered = filtered.filter((tr) =>
          typeof tr.scope === 'object' && 'device_type' in tr.scope && tr.scope.device_type === deviceTypeId
        )
      }

      setTransforms(filtered)
    } catch (error) {
      handleError(error, { operation: 'Fetch transforms', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    if (open) {
      fetchTransforms()
    }
  }, [open, deviceId, deviceTypeId])

  const handleToggleTransform = async (transform: TransformAutomation) => {
    try {
      await api.setAutomationStatus(transform.id, !transform.enabled)
      await fetchTransforms()
    } catch {
      // ignore
    }
  }

  const handleDeleteTransform = async (id: string) => {
    const confirmed = await confirm({
      title: t('common:delete'),
      description: t('automation:confirmDeleteTransform', { defaultValue: 'Are you sure?' }),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    try {
      await api.deleteAutomation(id)
      await fetchTransforms()
    } catch {
      // ignore
    }
  }

  const handleSaveTransform = async (data: Partial<TransformAutomation>) => {
    try {
      const definition = {
        scope: data.scope || 'global',
        js_code: data.js_code || '',
        output_prefix: data.output_prefix || '',
        complexity: data.complexity || 2,
      }

      if (editingTransform) {
        await api.updateAutomation(editingTransform.id, {
          name: data.name || '',
          description: data.description,
          enabled: data.enabled,
          definition,
        })
      } else {
        await api.createAutomation({
          name: data.name || '',
          description: data.description,
          type: 'transform',
          enabled: data.enabled ?? true,
          definition,
        })
      }
      await fetchTransforms()
      setBuilderOpen(false)
      setEditingTransform(null)
      onTransformCreated?.()
    } catch (error) {
      handleError(error, { operation: 'Save transform', showToast: true })
      throw error
    }
  }

  const handleCreateNew = () => {
    const preFilledScope: TransformScope = deviceId
      ? { device: deviceId }
      : deviceTypeId
        ? { device_type: deviceTypeId }
        : 'global'

    setEditingTransform({
      id: '',
      name: '',
      description: '',
      enabled: true,
      type: 'transform',
      scope: preFilledScope,
      output_prefix: '',
      complexity: 1,
      execution_count: 0,
      created_at: 0,
      updated_at: 0,
      last_executed: null,
    } as TransformAutomation)
    setBuilderOpen(true)
  }

  const dialogTitle = deviceId
    ? t('automation:deviceTransforms', { defaultValue: 'Transforms for Device' })
    : deviceTypeId
      ? t('automation:deviceTypeTransforms', { defaultValue: 'Transforms for Device Type' })
      : t('automation:allTransforms', { defaultValue: 'All Transforms' })

  return (
    <>
      <UnifiedFormDialog
        open={open}
        onOpenChange={onOpenChange}
        title={deviceName ? `${dialogTitle} · ${deviceName}` : dialogTitle}
        width="lg"
        loading={loading && transforms.length === 0}
        preventCloseOnSubmit={false}
        footer={
          <>
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              {t('common:close')}
            </Button>
            <Button onClick={handleCreateNew}>
              {t('automation:createTransform', { defaultValue: 'Create Transform' })}
            </Button>
          </>
        }
      >
        {transforms.length === 0 ? (
          <p className="py-8 text-center text-sm text-muted-foreground">
            {t('automation:noTransformsForDevice', { defaultValue: 'No transforms configured yet.' })}
          </p>
        ) : (
          <div className="space-y-2">
            {transforms.map((transform) => {
              const subtitle = transform.description?.trim()
              const showSubtitle = !!subtitle && subtitle !== '-'
              return (
                <div key={transform.id} className="flex items-center gap-3 rounded-lg border border-border bg-card px-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{transform.name}</div>
                    {showSubtitle && (
                      <div className="truncate text-xs text-muted-foreground">{subtitle}</div>
                    )}
                  </div>
                  <Switch
                    checked={transform.enabled}
                    onCheckedChange={() => handleToggleTransform(transform)}
                    aria-label={t('common:status', { defaultValue: 'Status' })}
                  />
                  <div className="flex shrink-0 items-center">
                    <IconButton
                      aria-label={t('automation:testTransform', { defaultValue: 'Test' })}
                      title={t('automation:testTransform', { defaultValue: 'Test' })}
                      onClick={() => {
                        setTestingTransformId(transform.id)
                        setTestDialogOpen(true)
                      }}
                    >
                      <Play className="h-4 w-4" />
                    </IconButton>
                    <IconButton
                      aria-label={t('common:edit', { defaultValue: 'Edit' })}
                      title={t('common:edit', { defaultValue: 'Edit' })}
                      onClick={() => {
                        setEditingTransform(transform)
                        setBuilderOpen(true)
                      }}
                    >
                      <Edit className="h-4 w-4" />
                    </IconButton>
                    <IconButton
                      aria-label={t('common:delete', { defaultValue: 'Delete' })}
                      title={t('common:delete', { defaultValue: 'Delete' })}
                      onClick={() => handleDeleteTransform(transform.id)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </IconButton>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </UnifiedFormDialog>

      {/* Transform Builder Dialog */}
      <TransformBuilderSplit
        open={builderOpen}
        onOpenChange={setBuilderOpen}
        transform={editingTransform}
        devices={devices}
        deviceTypes={deviceTypes}
        onSave={handleSaveTransform}
      />

      {/* Transform Test Dialog */}
      {testDialogOpen && testingTransformId && (
        <TransformTestDialog
          open={testDialogOpen}
          onOpenChange={setTestDialogOpen}
          transformId={testingTransformId}
          devices={devices}
        />
      )}
    </>
  )
}

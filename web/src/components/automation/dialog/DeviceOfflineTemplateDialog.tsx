import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { FormField } from '@/components/ui/field'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Input } from '@/components/ui/input'
import { useToast } from '@/hooks/use-toast'
import { api } from '@/lib/api'
import { useStore } from '@/store'
import type { Rule } from '@/types/rule'
import {
  DEVICE_OFFLINE_DEFAULTS,
  type DeviceOfflineTemplateOptions,
  type DurationUnit,
  type Severity,
  RULE_TEMPLATES,
} from '../ruleTemplates'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: () => void  // refresh rule list
}

export function DeviceOfflineTemplateDialog({ open, onOpenChange, onCreated }: Props) {
  const { t } = useTranslation()
  const { toast } = useToast()
  const devices = useStore((s) => s.devices)
  const [opts, setOpts] = useState<DeviceOfflineTemplateOptions>(DEVICE_OFFLINE_DEFAULTS)
  const [submitting, setSubmitting] = useState(false)

  // device_offline is the first (and currently only) template.
  const template = RULE_TEMPLATES[0]

  const handleSubmit = async () => {
    if (!opts.deviceId) {
      toast({ title: t('rules.templates.deviceOffline.selectDevice'), variant: 'destructive' })
      return
    }
    setSubmitting(true)
    try {
      const partialRule = template.build(opts)
      const payload: Omit<Rule, 'id' | 'created_at' | 'updated_at'> = {
        name: t('rules.templates.deviceOffline.defaultName', { device: opts.deviceId }),
        enabled: true,
        trigger_count: 0,
        dsl_preview: '',
        condition: partialRule.condition,
        actions: partialRule.actions ?? [],
        trigger: partialRule.trigger ?? { trigger_type: 'manual' },
        cooldown: partialRule.cooldown,
        for_duration: partialRule.for_duration,
      }
      await api.createRule(payload)
      toast({ title: t('common.success') })
      onOpenChange(false)
      onCreated()
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : undefined
      toast({
        title: t('common.error'),
        description: message,
        variant: 'destructive',
      })
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('rules.templates.deviceOffline.label')}
      onSubmit={handleSubmit}
      isSubmitting={submitting}
    >
      <FormField label={t('rules.templates.deviceOffline.device')}>
        <Select
          value={opts.deviceId}
          onValueChange={(deviceId) => setOpts((s: DeviceOfflineTemplateOptions) => ({ ...s, deviceId }))}
        >
          <SelectTrigger>
            <SelectValue placeholder={t('rules.templates.deviceOffline.selectDevice')} />
          </SelectTrigger>
          <SelectContent>
            {devices.map((d) => (
              <SelectItem key={d.id} value={d.id}>
                {d.name || d.id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </FormField>

      <FormField label={t('rules.templates.deviceOffline.duration')}>
        <div className="flex gap-2">
          <Input
            type="number"
            min={1}
            value={opts.durationValue}
            onChange={(e) => setOpts((s: DeviceOfflineTemplateOptions) => ({ ...s, durationValue: Number(e.target.value) }))}
          />
          <Select
            value={opts.durationUnit}
            onValueChange={(v) => setOpts((s: DeviceOfflineTemplateOptions) => ({ ...s, durationUnit: v as DurationUnit }))}
          >
            <SelectTrigger className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="minutes">{t('rules.templates.deviceOffline.minutes')}</SelectItem>
              <SelectItem value="hours">{t('rules.templates.deviceOffline.hours')}</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </FormField>

      <FormField label={t('rules.templates.deviceOffline.severity')}>
        <Select
          value={opts.severity}
          onValueChange={(v) => setOpts((s: DeviceOfflineTemplateOptions) => ({ ...s, severity: v as Severity }))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="info">Info</SelectItem>
            <SelectItem value="warning">Warning</SelectItem>
            <SelectItem value="critical">Critical</SelectItem>
            <SelectItem value="emergency">Emergency</SelectItem>
          </SelectContent>
        </Select>
      </FormField>
    </UnifiedFormDialog>
  )
}

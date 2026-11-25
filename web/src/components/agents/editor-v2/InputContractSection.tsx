/**
 * Input contract section — editable list of InputRequirement entries.
 * Minimal surface for daily-brief style extension reads.
 */
import { useTranslation } from 'react-i18next'
import { Plus, Trash2 } from 'lucide-react'
import { Activity } from '@/design-system/icons'
import { Button } from '@/components/ui/button'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { cardBase } from '@/design-system/tokens/size'
import { cn } from '@/lib/utils'
import type { InputContract, InputRequirement } from '@/types'

export interface InputContractSectionProps {
  value: InputContract
  onChange: (next: InputContract) => void
  className?: string
}

function sourceLabel(req: InputRequirement): string {
  const src = req.source
  switch (src.kind) {
    case 'extension_instance':
      return `extension:${src.id}${req.operation ? `/${req.operation}` : ''}`
    case 'device_metric':
      return `device:${src.device_id}:${src.field}`
    case 'whole_device':
      return `device:${src.device_id}`
    case 'upstream_ai':
      return `ai:${src.agent_id}:${src.field}`
    case 'caller_payload':
      return 'caller_payload'
  }
}

function emptyExtensionRequirement(index: number): InputRequirement {
  return {
    key: `input_${index + 1}`,
    source: { kind: 'extension_instance', id: '' },
    operation: '',
    timeout_secs: 10,
    result_type: 'structured_json',
    freshness: { kind: 'any' },
    on_missing: 'report_stale',
  }
}

export function InputContractSection({ value, onChange, className }: InputContractSectionProps) {
  const { t } = useTranslation('agents')
  const requirements = value.requirements ?? []

  const updateAt = (index: number, patch: Partial<InputRequirement>) => {
    const next = requirements.map((r, i) => (i === index ? { ...r, ...patch } : r))
    onChange({ requirements: next })
  }

  const updateExtensionId = (index: number, id: string) => {
    const req = requirements[index]
    if (!req || req.source.kind !== 'extension_instance') return
    updateAt(index, { source: { kind: 'extension_instance', id } })
  }

  const removeAt = (index: number) => {
    onChange({ requirements: requirements.filter((_, i) => i !== index) })
  }

  const addRequirement = () => {
    onChange({
      requirements: [...requirements, emptyExtensionRequirement(requirements.length)],
    })
  }

  return (
    <section className={cn(cardBase, 'p-4 space-y-4', className)} data-section="input">
      <header className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <Activity className="h-4 w-4 text-muted-foreground" />
          <h3 className="text-sm font-medium text-foreground">
            {t('editorV2.input.title')}
          </h3>
          <span className="text-xs text-muted-foreground">
            {t('editorV2.input.count', { count: requirements.length })}
          </span>
        </div>
        <Button type="button" variant="outline" size="sm" onClick={addRequirement}>
          <Plus className="h-3.5 w-3.5 mr-1.5" />
          {t('editorV2.input.add')}
        </Button>
      </header>

      {requirements.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t('editorV2.input.empty')}</p>
      ) : (
        <ul className="space-y-3">
          {requirements.map((req, index) => (
            <li
              key={`${req.key}-${index}`}
              className="rounded-lg border border-border bg-muted-20 p-3 space-y-3"
            >
              <div className="flex items-start justify-between gap-2">
                <p className="text-xs text-muted-foreground font-mono truncate">
                  {sourceLabel(req)}
                </p>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-8 w-8 p-0 text-muted-foreground hover:text-error"
                  onClick={() => removeAt(index)}
                  aria-label={t('editorV2.input.remove')}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
              <div className="grid gap-3 sm:grid-cols-2">
                <FormField label={t('editorV2.input.key')}>
                  <Input
                    value={req.key}
                    onChange={(e) => updateAt(index, { key: e.target.value })}
                    className="font-mono text-sm"
                  />
                </FormField>
                {req.source.kind === 'extension_instance' ? (
                  <FormField label={t('editorV2.input.extensionId')}>
                    <Input
                      value={req.source.id}
                      onChange={(e) => updateExtensionId(index, e.target.value)}
                      placeholder="weather_cn"
                      className="font-mono text-sm"
                    />
                  </FormField>
                ) : (
                  <FormField label={t('editorV2.input.source')}>
                    <Input value={sourceLabel(req)} disabled className="font-mono text-sm" />
                  </FormField>
                )}
                {req.source.kind === 'extension_instance' && (
                  <FormField label={t('editorV2.input.operation')}>
                    <Input
                      value={req.operation ?? ''}
                      onChange={(e) => updateAt(index, { operation: e.target.value })}
                      placeholder="current"
                      className="font-mono text-sm"
                    />
                  </FormField>
                )}
              </div>
              {req.auth_scope && (
                <p className="text-xs text-muted-foreground">
                  {t('editorV2.input.auth')}: {req.auth_scope}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

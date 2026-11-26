/**
 * Input contract section — editable list of InputRequirement entries.
 * Minimal surface for daily-brief style extension reads.
 */
import { useTranslation } from 'react-i18next'
import { Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import type { InputContract, InputRequirement } from '@/types'
import { EditorSection } from './EditorSection'

export interface InputContractSectionProps {
  value: InputContract
  onChange: (next: InputContract) => void
  className?: string
}

function sourceLabel(req: InputRequirement): string {
  const src = req?.source
  if (!src || typeof src !== 'object') return req?.key || '—'
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
  const requirements = value?.requirements ?? []

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
    <EditorSection
      title={t('editorV2.input.title')}
      meta={t('agents:editorV2.input.count', { count: requirements.length })}
      className={cn(className)}
      data-section="input"
      actions={
        <Button type="button" variant="outline" size="sm" onClick={addRequirement}>
          <Plus className="h-3.5 w-3.5 mr-1.5" />
          {t('editorV2.input.add')}
        </Button>
      }
    >
      {requirements.length > 0 && (
        <ul className="space-y-3">
          {requirements.map((req, index) => (
            <li
              key={`${req.key}-${index}`}
              className="space-y-3 rounded-md border border-border bg-muted-20 p-3"
            >
              <div className="flex items-start justify-between gap-2">
                <p className="truncate font-mono text-xs text-muted-foreground">
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
            </li>
          ))}
        </ul>
      )}
    </EditorSection>
  )
}

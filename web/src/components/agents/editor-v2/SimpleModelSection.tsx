/**
 * Concise LLM backend picker → draft.llm_backend_id.
 * Empty / "use active" means fall back to the global active backend.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { FormField } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import type { LlmBackendInstance } from '@/types'

const ACTIVE_VALUE = '__active__'

export interface SimpleModelSectionProps {
  value?: string
  onChange: (llmBackendId: string | undefined) => void
  className?: string
}

export function SimpleModelSection({ value, onChange, className }: SimpleModelSectionProps) {
  const { t } = useTranslation('agents')
  const [backends, setBackends] = useState<LlmBackendInstance[]>([])
  const [activeId, setActiveId] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    void api
      .listLlmBackends({ include_vlm: true })
      .then((resp) => {
        if (cancelled) return
        setBackends(resp.backends ?? [])
        setActiveId(resp.active_id ?? null)
      })
      .catch(() => {
        if (!cancelled) {
          setBackends([])
          setActiveId(null)
        }
      })
    return () => {
      cancelled = true
    }
  }, [])

  const selectValue = value?.trim() ? value : ACTIVE_VALUE

  return (
    <div className={cn(className)} data-section="model-simple">
      <FormField label={t('editorV2.model.label')}>
        <Select
          value={selectValue}
          onValueChange={(v) => onChange(v === ACTIVE_VALUE ? undefined : v)}
        >
          <SelectTrigger className="h-10">
            <SelectValue placeholder={t('editorV2.model.useActive')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ACTIVE_VALUE}>
              {activeId
                ? t('editorV2.model.useActiveNamed', {
                    name: backends.find((b) => b.id === activeId)?.name ?? activeId,
                  })
                : t('editorV2.model.useActive')}
            </SelectItem>
            {backends.map((b) => (
              <SelectItem key={b.id} value={b.id}>
                {b.name}
                {b.model ? ` · ${b.model}` : ''}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </FormField>
    </div>
  )
}

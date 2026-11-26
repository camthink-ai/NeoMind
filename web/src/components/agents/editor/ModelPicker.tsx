/**
 * LLM Model picker for the BuilderShell left config rail.
 * Shows global default + every backend with model name and capability pills
 * (Vision / Tools / Thinking).
 */
import { useTranslation } from 'react-i18next'
import { Field, FieldLabel } from '@/components/ui/field'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { textNano } from '@/design-system/tokens/typography'
import type { LlmBackendInstance } from '@/types'

const ACTIVE_VALUE = '__active__'

export interface ModelPickerProps {
  value?: string
  onChange: (llmBackendId: string | undefined) => void
  backends: LlmBackendInstance[]
}

export function ModelPicker({ value, onChange, backends }: ModelPickerProps) {
  const { t } = useTranslation('agents')
  const selectValue = value?.trim() ? value : ACTIVE_VALUE
  const activeBackend = backends.find((b) => b.is_active)

  return (
    <Field data-section="model-picker">
      <FieldLabel>{t('editorV2.model.label')}</FieldLabel>
      <Select
        value={selectValue}
        onValueChange={(v) => onChange(v === ACTIVE_VALUE ? undefined : v)}
      >
        <SelectTrigger>
          <SelectValue placeholder={t('editorV2.model.useActive')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={ACTIVE_VALUE}>
            {activeBackend
              ? t('agents:editorV2.model.useActiveNamed', { name: activeBackend.name })
              : t('editorV2.model.useActive')}
          </SelectItem>
          {backends.map((b) => (
            <SelectItem key={b.id} value={b.id} textValue={b.name}>
              <span className="flex flex-1 min-w-0 items-center gap-1.5">
                <span className="truncate font-medium">{b.name}</span>
                {b.model && (
                  <span className="truncate text-xs text-muted-foreground">{b.model}</span>
                )}
                <span className="ml-auto flex shrink-0 items-center gap-1">
                  {b.capabilities?.supports_multimodal && (
                    <span className={cn('rounded bg-muted px-1.5 py-0.5 text-muted-foreground', textNano)}>
                      {t('editorV2.capability.vision')}
                    </span>
                  )}
                  {b.capabilities?.supports_tools && (
                    <span className={cn('rounded bg-muted px-1.5 py-0.5 text-muted-foreground', textNano)}>
                      {t('editorV2.capability.tools')}
                    </span>
                  )}
                  {b.capabilities?.supports_thinking && (
                    <span className={cn('rounded bg-muted px-1.5 py-0.5 text-muted-foreground', textNano)}>
                      {t('editorV2.capability.thinking')}
                    </span>
                  )}
                </span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  )
}

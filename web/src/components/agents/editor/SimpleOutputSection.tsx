/**
 * Optional structured output fields → draft.output.fields.
 * Empty list = free-form conclusion; non-empty → backend structured_result.
 */
import { useTranslation } from 'react-i18next'
import { Plus, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { cn } from '@/lib/utils'
import type { OutputContract, OutputFieldV2 } from '@/types'

export interface SimpleOutputSectionProps {
  value?: OutputContract
  onChange: (next: OutputContract) => void
  className?: string
}

function normalize(value?: OutputContract): OutputContract {
  return {
    fields: [...(value?.fields ?? [])],
    publish: value?.publish ?? false,
  }
}

export function SimpleOutputSection({ value, onChange, className }: SimpleOutputSectionProps) {
  const { t } = useTranslation('agents')
  const contract = normalize(value)
  const enabled = contract.fields.length > 0

  const setEnabled = (on: boolean) => {
    if (on) {
      onChange({
        fields: contract.fields.length
          ? contract.fields
          : [{ name: 'result', description: '', depends_on: [] }],
        publish: false,
      })
      return
    }
    onChange({ fields: [], publish: false })
  }

  const updateField = (index: number, patch: Partial<OutputFieldV2>) => {
    const fields = contract.fields.map((f, i) => (i === index ? { ...f, ...patch } : f))
    onChange({ ...contract, fields })
  }

  const removeField = (index: number) => {
    const fields = contract.fields.filter((_, i) => i !== index)
    onChange({ ...contract, fields })
  }

  const addField = () => {
    onChange({
      ...contract,
      fields: [...contract.fields, { name: '', description: '', depends_on: [] }],
    })
  }

  return (
    <div className={cn('space-y-3', className)} data-section="output-simple">
      <div className="flex items-center justify-between gap-3 rounded-md border border-border px-3 py-2.5">
        <Label htmlFor="simple-output-enabled" className="text-sm font-medium">
          {t('editorV2.output.enabled')}
        </Label>
        <Switch
          id="simple-output-enabled"
          checked={enabled}
          onCheckedChange={setEnabled}
        />
      </div>

      {enabled && (
        <div className="space-y-2">
          <ul className="space-y-2">
            {contract.fields.map((field, index) => (
              <li key={index} className="flex items-center gap-2">
                <Input
                  value={field.name}
                  onChange={(e) => updateField(index, { name: e.target.value })}
                  placeholder={t('editorV2.output.namePlaceholder')}
                  className="font-mono text-sm"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-9 w-9 shrink-0 text-muted-foreground hover:text-error"
                  onClick={() => removeField(index)}
                  aria-label={t('editorV2.output.remove')}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </li>
            ))}
          </ul>
          <Button type="button" variant="outline" size="sm" onClick={addField}>
            <Plus className="mr-1.5 h-3.5 w-3.5" />
            {t('editorV2.output.add')}
          </Button>
        </div>
      )}
    </div>
  )
}

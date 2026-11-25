/**
 * Tool policy section — disabled / unrestricted / scoped allow-list.
 */
import { useTranslation } from 'react-i18next'
import { Wrench } from 'lucide-react'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cardBase } from '@/design-system/tokens/size'
import { cn } from '@/lib/utils'
import type { ToolPolicy } from '@/types'

export interface ToolPolicySectionProps {
  value: ToolPolicy
  onChange: (next: ToolPolicy) => void
  className?: string
}

type PolicyMode = 'disabled' | 'unrestricted' | 'scoped'

function modeOf(policy: ToolPolicy): PolicyMode {
  if (policy === 'disabled') return 'disabled'
  if (policy === 'unrestricted') return 'unrestricted'
  return 'scoped'
}

function toolsText(policy: ToolPolicy): string {
  if (typeof policy === 'object' && 'scoped' in policy) {
    return (policy.scoped.allowed_tools ?? []).join(', ')
  }
  return ''
}

export function ToolPolicySection({ value, onChange, className }: ToolPolicySectionProps) {
  const { t } = useTranslation('agents')
  const mode = modeOf(value)

  const setMode = (next: PolicyMode) => {
    if (next === 'disabled') onChange('disabled')
    else if (next === 'unrestricted') onChange('unrestricted')
    else {
      const existing = typeof value === 'object' && 'scoped' in value ? value.scoped.allowed_tools : []
      onChange({ scoped: { allowed_tools: [...(existing ?? [])], allowed_commands: [] } })
    }
  }

  return (
    <section className={cn(cardBase, 'p-4 space-y-4', className)} data-section="tool-policy">
      <header className="flex items-center gap-2">
        <Wrench className="h-4 w-4 text-muted-foreground" />
        <h3 className="text-sm font-medium text-foreground">
          {t('editorV2.tools.title')}
        </h3>
      </header>

      <FormField label={t('editorV2.tools.mode')} helpText={t('editorV2.tools.hint')}>
        <Select value={mode} onValueChange={(v) => setMode(v as PolicyMode)}>
          <SelectTrigger className="h-10">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="disabled">{t('editorV2.tools.disabled')}</SelectItem>
            <SelectItem value="unrestricted">{t('editorV2.tools.unrestricted')}</SelectItem>
            <SelectItem value="scoped">{t('editorV2.tools.scoped')}</SelectItem>
          </SelectContent>
        </Select>
      </FormField>

      {mode === 'scoped' && (
        <FormField
          label={t('editorV2.tools.allowedTools')}
          helpText={t('editorV2.tools.allowedToolsHint')}
        >
          <Input
            value={toolsText(value)}
            onChange={(e) => {
              const allowed_tools = e.target.value
                .split(',')
                .map((s) => s.trim())
                .filter(Boolean)
              onChange({ scoped: { allowed_tools, allowed_commands: [] } })
            }}
            placeholder="shell, memory"
            className="font-mono text-sm"
          />
        </FormField>
      )}
    </section>
  )
}

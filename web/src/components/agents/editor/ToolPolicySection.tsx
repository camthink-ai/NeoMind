/**
 * Tool policy section — disabled / unrestricted / scoped allow-list.
 */
import { useTranslation } from 'react-i18next'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { cn } from '@/lib/utils'
import type { ToolPolicy } from '@/types'
import { normalizeToolPolicy } from './defaults'
import { EditorSection } from './EditorSection'

export interface ToolPolicySectionProps {
  value?: ToolPolicy | null
  onChange: (next: ToolPolicy) => void
  className?: string
}

type PolicyMode = 'disabled' | 'unrestricted' | 'scoped'

function modeOf(policy?: ToolPolicy | null): PolicyMode {
  const safe = normalizeToolPolicy(policy)
  if (safe === 'disabled') return 'disabled'
  if (safe === 'unrestricted') return 'unrestricted'
  return 'scoped'
}

function toolsText(policy?: ToolPolicy | null): string {
  const safe = normalizeToolPolicy(policy)
  if (typeof safe === 'object' && 'scoped' in safe) {
    return (safe.scoped.allowed_tools ?? []).join(', ')
  }
  return ''
}

export function ToolPolicySection({ value, onChange, className }: ToolPolicySectionProps) {
  const { t } = useTranslation('agents')
  const safe = normalizeToolPolicy(value)
  const mode = modeOf(safe)

  const setMode = (next: PolicyMode) => {
    if (next === 'disabled') onChange('disabled')
    else if (next === 'unrestricted') onChange('unrestricted')
    else {
      const existing = typeof safe === 'object' && 'scoped' in safe ? safe.scoped.allowed_tools : []
      onChange({ scoped: { allowed_tools: [...(existing ?? [])], allowed_commands: [] } })
    }
  }

  return (
    <EditorSection
      title={t('editorV2.tools.title')}
      className={cn(className)}
      data-section="tool-policy"
    >
      <FormField label={t('editorV2.tools.mode')}>
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
        <FormField label={t('editorV2.tools.allowedTools')}>
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
    </EditorSection>
  )
}

// The full template list. The rail carries only the few most agents start
// from; nine two-line cards would push the name and model fields past the fold
// of the column that holds them.

import { useTranslation } from 'react-i18next'
import { LayoutGrid } from 'lucide-react'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogMain,
} from '@/components/automation/dialog/FullScreenDialog'
import { cn } from '@/lib/utils'
import { interactiveCardHover } from '@/design-system/tokens/size'
import { AGENT_PRESETS, type AgentPreset } from './presets'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSelect: (preset: AgentPreset) => void
}

export function PresetPickerDialog({ open, onOpenChange, onSelect }: Props) {
  const { t } = useTranslation('agents')

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<LayoutGrid className="h-full w-full" />}
        iconBg="bg-primary-light"
        iconColor="text-primary"
        title={t('creator.preset.allTitle')}
        subtitle={t('creator.preset.allSubtitle')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogMain>
        <div className="mx-auto w-full max-w-4xl p-4 md:p-8">
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {AGENT_PRESETS.map((preset) => {
              const Icon = preset.icon
              return (
                <button
                  key={preset.key}
                  type="button"
                  onClick={() => onSelect(preset)}
                  className={cn(
                    'group flex flex-col items-start gap-2 rounded-xl border border-border bg-card p-4 text-left shadow-sm',
                    'card-sheen',
                    interactiveCardHover,
                    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
                  )}
                >
                  <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted-30 text-muted-foreground transition-colors group-hover:text-foreground">
                    <Icon className="h-4 w-4" />
                  </span>
                  <span className="text-sm font-medium">
                    {t(`creator.preset.${preset.key}.name`)}
                  </span>
                  <span className="text-xs leading-relaxed text-muted-foreground">
                    {t(`creator.preset.${preset.key}.desc`)}
                  </span>
                </button>
              )
            })}
          </div>
        </div>
      </FullScreenDialogMain>
    </FullScreenDialog>
  )
}

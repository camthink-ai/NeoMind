import { useTranslation } from 'react-i18next'
import { Zap, ChevronRight } from 'lucide-react'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogMain,
} from '@/components/automation/dialog/FullScreenDialog'
import { RULE_TEMPLATES } from '../ruleTemplates'
import { cn } from '@/lib/utils'
import { interactiveCardHover } from '@/design-system/tokens/size'

interface Props {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSelectTemplate: (templateId: string) => void
}

const ACCENT_STYLES: Record<string, { icon: string }> = {
  primary: { icon: 'bg-primary-light text-primary' },
  success: { icon: 'bg-success-light text-success' },
  warning: { icon: 'bg-warning-light text-warning' },
  error: { icon: 'bg-error-light text-error' },
  info: { icon: 'bg-info-light text-info' },
}

export function RuleTemplatePicker({ open, onOpenChange, onSelectTemplate }: Props) {
  const { t } = useTranslation()
  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<Zap className="w-full h-full" />}
        iconBg="bg-primary-light"
        iconColor="text-primary"
        title={t('rules.templates.title')}
        subtitle={t('rules.templates.subtitle')}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogMain>
        <div className="mx-auto w-full max-w-4xl p-4 md:p-8">
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            {RULE_TEMPLATES.map((tpl) => {
              const Icon = tpl.icon
              const accent = ACCENT_STYLES[tpl.accent ?? 'primary']
              return (
                <button
                  key={tpl.id}
                  type="button"
                  onClick={() => onSelectTemplate(tpl.id)}
                  className={cn(
                    'group text-left bg-card rounded-xl border shadow-sm p-5',
                    'card-sheen',
                    interactiveCardHover,
                    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2',
                  )}
                >
                  <div className="flex flex-col gap-3">
                    <div className={cn(
                      'flex h-12 w-12 items-center justify-center rounded-xl transition-transform group-hover:scale-105',
                      accent.icon,
                    )}>
                      <Icon className="h-6 w-6" />
                    </div>
                    <div className="flex items-start justify-between gap-2">
                      <h3 className="font-semibold text-base leading-tight">{t(tpl.labelKey)}</h3>
                      <ChevronRight className="h-4 w-4 text-muted-foreground shrink-0 mt-0.5 transition-transform group-hover:translate-x-0.5" />
                    </div>
                    <p className="text-sm text-muted-foreground leading-relaxed">
                      {t(tpl.descriptionKey)}
                    </p>
                  </div>
                </button>
              )
            })}
          </div>
        </div>
      </FullScreenDialogMain>
    </FullScreenDialog>
  )
}

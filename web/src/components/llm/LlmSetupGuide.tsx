/**
 * LlmSetupGuide — shared guided empty-state for "no LLM backend yet".
 *
 * One story on every surface: the built-in model is the fastest path
 * (one click, offline, no API key), bring-your-own-backend is the second.
 * Used full-screen (chat) and as a page banner (agents). The explanatory
 * copy folds in the onboarding guide's LLM section — why an AI brain
 * matters — so the empty state teaches instead of just blocking.
 */

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { Cpu, Server, ChevronRight, Download } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { BuiltinModelWizard } from '@/components/llm/BuiltinModelWizard'

interface LlmSetupGuideProps {
  /** Full-screen centered card (chat empty state) */
  variant?: 'full' | 'banner'
}

export function LlmSetupGuide({ variant = 'full' }: LlmSetupGuideProps) {
  const { t } = useTranslation(['common'])
  const openSettings = useStore((s) => s.openSettings)
  const [wizardOpen, setWizardOpen] = useState(false)

  const wizard = (
    <BuiltinModelWizard
      open={wizardOpen}
      onOpenChange={setWizardOpen}
      onActivated={() => setWizardOpen(false)}
    />
  )

  const actions = (
    <>
      <button
        onClick={() => setWizardOpen(true)}
        className="w-full flex items-center gap-3 p-3 rounded-lg border border-border hover:bg-muted-50 transition-colors text-left group"
      >
        <div className="flex size-9 items-center justify-center rounded-lg bg-primary-light text-primary shrink-0">
          <Download className="size-4" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium">{t('common:llmGuide.builtinTitle')}</div>
          <div className="text-xs text-muted-foreground">{t('common:llmGuide.builtinDesc')}</div>
        </div>
        <ChevronRight className="size-4 text-muted-foreground group-hover:text-foreground transition-colors shrink-0" />
      </button>
      <button
        onClick={() => openSettings()}
        className="w-full flex items-center gap-3 p-3 rounded-lg border border-border hover:bg-muted-50 transition-colors text-left group"
      >
        <div className="flex size-9 items-center justify-center rounded-lg bg-muted text-primary shrink-0">
          <Server className="size-4" />
        </div>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium">{t('common:llmGuide.ownTitle')}</div>
          <div className="text-xs text-muted-foreground">{t('common:llmGuide.ownDesc')}</div>
        </div>
        <ChevronRight className="size-4 text-muted-foreground group-hover:text-foreground transition-colors shrink-0" />
      </button>
    </>
  )

  if (variant === 'banner') {
    return (
      <div className="mx-4 sm:mx-6 mt-4 rounded-xl border border-primary bg-primary-light p-4">
        <div className="flex flex-col items-start gap-4 sm:flex-row sm:items-center">
          <div className="flex size-11 items-center justify-center rounded-xl bg-primary-light text-primary shrink-0">
            <Cpu className="size-5" />
          </div>
          <div className="min-w-0 flex-1">
            <h3 className="text-sm font-semibold">{t('common:llmGuide.bannerTitle')}</h3>
            <p className="mt-0.5 text-xs text-muted-foreground leading-relaxed">
              {t('common:llmGuide.bannerDesc')}
            </p>
          </div>
          <div className="flex flex-wrap items-center gap-2 shrink-0">
            <Button size="sm" onClick={() => setWizardOpen(true)}>
              <Download className="mr-1.5 h-3.5 w-3.5" />
              {t('common:llmGuide.builtinShort')}
            </Button>
            <Button size="sm" variant="secondary" onClick={() => openSettings()}>
              <Server className="mr-1.5 h-3.5 w-3.5" />
              {t('common:llmGuide.ownShort')}
            </Button>
          </div>
        </div>
        {wizard}
      </div>
    )
  }

  return (
    <div className={cn('flex items-center justify-center', 'h-full', 'bg-background')}>
      <div className="text-center max-w-md px-6">
        <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-xl bg-primary-light text-primary">
          <Cpu className="size-7" />
        </div>
        <h2 className="mb-2 text-lg font-semibold tracking-tight">{t('common:llmGuide.title')}</h2>
        <p className="text-sm text-muted-foreground mb-6 leading-relaxed">
          {t('common:llmGuide.desc')}
        </p>
        <div className="text-left space-y-2">{actions}</div>
        <p className="mt-5 text-xs text-muted-foreground">{t('common:llmGuide.footnote')}</p>
      </div>
      {wizard}
    </div>
  )
}

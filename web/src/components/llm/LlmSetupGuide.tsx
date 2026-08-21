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
            <Button size="sm" variant="secondary" onClick={() => openSettings('llm')}>
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
      <div className="text-center max-w-lg px-6">
        <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-xl bg-primary-light text-primary">
          <Cpu className="size-7" />
        </div>
        <h2 className="mb-2 text-lg font-semibold tracking-tight">{t('common:llmGuide.title')}</h2>
        <p className="text-sm text-muted-foreground mb-6 leading-relaxed">
          {t('common:llmGuide.desc')}
        </p>
        {/* Two choice cards side by side — recommended path visually distinct */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-left">
          <button
            onClick={() => setWizardOpen(true)}
            className="group relative rounded-xl border border-primary bg-primary-light p-4 transition-all hover:shadow-md text-left"
          >
            <span className="absolute right-3 top-3 rounded-full bg-primary text-primary-foreground px-2 py-0.5 text-[10px] font-medium">
              {t('common:llmGuide.recommended')}
            </span>
            <div className="flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
              <Download className="size-5" />
            </div>
            <div className="mt-3 text-sm font-semibold">{t('common:llmGuide.builtinTitleShort')}</div>
            <p className="mt-1 text-xs text-muted-foreground leading-relaxed">
              {t('common:llmGuide.builtinDesc')}
            </p>
            <div className="mt-3 flex items-center gap-1 text-xs font-medium text-primary">
              {t('common:llmGuide.builtinCta')}
              <ChevronRight className="size-3 transition-transform group-hover:translate-x-0.5" />
            </div>
          </button>
          <button
            onClick={() => openSettings('llm')}
            className="group rounded-xl border border-border p-4 transition-all hover:border-primary hover:shadow-md text-left"
          >
            <div className="flex size-10 items-center justify-center rounded-lg bg-muted text-primary">
              <Server className="size-5" />
            </div>
            <div className="mt-3 text-sm font-semibold">{t('common:llmGuide.ownTitleShort')}</div>
            <p className="mt-1 text-xs text-muted-foreground leading-relaxed">
              {t('common:llmGuide.ownDesc')}
            </p>
            <div className="mt-3 flex items-center gap-1 text-xs font-medium text-primary">
              {t('common:llmGuide.ownCta')}
              <ChevronRight className="size-3 transition-transform group-hover:translate-x-0.5" />
            </div>
          </button>
        </div>
        <p className="mt-5 text-xs text-muted-foreground">{t('common:llmGuide.footnote')}</p>
      </div>
      {wizard}
    </div>
  )
}

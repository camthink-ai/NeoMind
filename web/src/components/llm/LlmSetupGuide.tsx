/**
 * LlmSetupGuide — shared guided empty-state for "no LLM backend yet".
 *
 * One story on every surface: the built-in model is the fastest path
 * (one click, offline, no API key), bring-your-own-backend is the second.
 * Used full-screen (chat empty state). The explanatory
 * copy folds in the onboarding guide's LLM section — why an AI brain
 * matters — so the empty state teaches instead of just blocking.
 */

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { Cpu, Server, ChevronRight, Download } from 'lucide-react'
import { BuiltinModelWizard } from '@/components/llm/BuiltinModelWizard'

export function LlmSetupGuide() {
  const { t } = useTranslation(['common'])
  const openSettings = useStore((s) => s.openSettings)
  const loadBackends = useStore((s) => s.loadBackends)
  const [wizardOpen, setWizardOpen] = useState(false)

  const wizard = (
    <BuiltinModelWizard
      open={wizardOpen}
      onOpenChange={setWizardOpen}
      onActivated={() => { setWizardOpen(false); loadBackends() }}
    />
  )

  return (
    <div className={cn('flex items-center justify-center', 'h-full', 'bg-background')}>
      <div className="text-center max-w-2xl px-6">
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
            className="group relative flex flex-col rounded-xl border border-primary bg-primary-light p-4 transition-all hover:shadow-md text-left"
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
            <div className="mt-auto pt-3 flex items-center gap-1 text-xs font-medium text-primary">
              {t('common:llmGuide.builtinCta')}
              <ChevronRight className="size-3 transition-transform group-hover:translate-x-0.5" />
            </div>
          </button>
          <button
            onClick={() => openSettings('llm')}
            className="group flex flex-col rounded-xl border border-border p-4 transition-all hover:border-primary hover:shadow-md text-left"
          >
            <div className="flex size-10 items-center justify-center rounded-lg bg-muted text-primary">
              <Server className="size-5" />
            </div>
            <div className="mt-3 text-sm font-semibold">{t('common:llmGuide.ownTitleShort')}</div>
            <p className="mt-1 text-xs text-muted-foreground leading-relaxed">
              {t('common:llmGuide.ownDesc')}
            </p>
            <div className="mt-auto pt-3 flex items-center gap-1 text-xs font-medium text-primary">
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

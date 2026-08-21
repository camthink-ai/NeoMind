/**
 * BuiltinDownloadIndicator — persistent top-right affordance while the
 * builtin model is downloading.
 *
 * The download continues server-side even if the wizard is closed; this
 * compact pill (next to the theme/language/alerts cluster) shows live
 * progress and reopens the wizard on click, so a dismissed dialog never
 * strands the user without a way back to the progress.
 */

import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Download } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { BuiltinModelWizard } from '@/components/llm/BuiltinModelWizard'
import { api } from '@/lib/api'
import type { BuiltinLlmStatus } from '@/types'

const POLL_MS = 3000

export function BuiltinDownloadIndicator() {
  const { t } = useTranslation('common')
  const [status, setStatus] = useState<BuiltinLlmStatus | null>(null)
  const [wizardOpen, setWizardOpen] = useState(false)

  useEffect(() => {
    let cancelled = false
    const poll = async () => {
      try {
        const s = await api.getBuiltinLlmStatus()
        if (!cancelled) setStatus(s)
      } catch {
        // Transient — next tick retries.
      }
    }
    poll()
    const timer = window.setInterval(poll, POLL_MS)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [])

  const downloading = status?.server_state === 'downloading'
  if (!downloading) return null

  const percent =
    status?.total_bytes && status?.total_bytes > 0 && status.downloaded_bytes != null
      ? Math.min(100, Math.round((status.downloaded_bytes / status.total_bytes) * 100))
      : null

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="gap-1.5 text-muted-foreground hover:text-foreground no-press-scale"
        onClick={() => setWizardOpen(true)}
        aria-label={t('common:llmGuide.downloadingTitle')}
      >
        <Download className="h-3.5 w-3.5 animate-pulse" />
        <span className="tabular-nums">
          {percent != null ? `${percent}%` : t('common:llmGuide.downloading')}
        </span>
      </Button>
      <BuiltinModelWizard
        open={wizardOpen}
        onOpenChange={setWizardOpen}
        onActivated={() => setWizardOpen(false)}
      />
    </>
  )
}

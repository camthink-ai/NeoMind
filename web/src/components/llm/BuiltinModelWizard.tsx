/**
 * BuiltinModelWizard — first-run guide for the bundled LFM2.5-2.6B model.
 *
 * A full-screen dialog (per DESIGN_SPEC: FullScreenDialog, never raw Dialog)
 * that walks a user through downloading + activating the built-in model:
 *
 *   idle        → model info + 「开始下载」
 *   downloading → progress bar (sourced from status polling today)
 *   activating  → brief spinner while POST /api/builtin-llm/activate runs
 *   ready       → 「已就绪」(+ restart engine when the bundled server is down)
 *   error       → error message + retry
 *
 * # Progress source
 * Task 15 replaces this with a WS `ModelDownloadProgress` subscription. Until
 * then progress is polled from `GET /api/builtin-llm/status`, which reports
 * `downloaded_bytes`/`total_bytes` while `server_state === 'downloading'`.
 * The view reads ONLY the small `progress` state below — Task 15 can swap the
 * source without touching the render.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
  FullScreenDialogFooter,
} from '@/components/automation/dialog'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import {
  AlertCircle,
  CheckCircle2,
  Cpu,
  Download,
  HardDrive,
  Loader2,
  Power,
  RotateCcw,
  WifiOff,
  Zap,
} from 'lucide-react'
import { api } from '@/lib/api'
import type { BuiltinLlmStatus } from '@/types'

const STATUS_POLL_MS = 2000

type WizardPhase = 'loading' | 'idle' | 'downloading' | 'activating' | 'ready' | 'error'

type RetryAction = 'download' | 'activate' | 'restart'

/** Small, isolated progress state — the single source the view reads. */
interface DownloadProgressState {
  percent: number | null
  downloadedBytes: number
  totalBytes: number | null
}

interface BuiltinModelWizardProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Whether an active backend exists that is NOT the builtin — suppresses auto-activate. */
  hasActiveBackend?: boolean
  /** Whether the builtin is currently the active backend. */
  isBuiltinActive?: boolean
  /** Called after the builtin backend is successfully activated (parent refreshes). */
  onActivated?: () => void
}

function extractErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function calcPercent(downloaded: number | null, total: number | null): number | null {
  if (total && total > 0 && downloaded != null) {
    return Math.min(100, Math.round((downloaded / total) * 100))
  }
  return null
}

function InfoTile({ icon, label, value }: { icon: React.ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-lg border border-border bg-card p-3">
      <div className="flex items-center gap-1.5 text-muted-foreground">
        {icon}
        <span className="text-xs">{label}</span>
      </div>
      <p className="mt-1.5 text-sm font-medium text-foreground leading-snug">{value}</p>
    </div>
  )
}

export function BuiltinModelWizard({
  open,
  onOpenChange,
  hasActiveBackend = false,
  isBuiltinActive = false,
  onActivated,
}: BuiltinModelWizardProps) {
  const { t } = useTranslation(['plugins', 'common'])

  const [phase, setPhase] = useState<WizardPhase>('loading')
  const [status, setStatus] = useState<BuiltinLlmStatus | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [retryAction, setRetryAction] = useState<RetryAction>('download')
  const [progress, setProgress] = useState<DownloadProgressState>({
    percent: null,
    downloadedBytes: 0,
    totalBytes: null,
  })

  // Guards against double-activation within one open session.
  const activatedRef = useRef(false)
  const sawDownloadingRef = useRef(false)

  const failWith = (action: RetryAction, error: unknown) => {
    setRetryAction(action)
    setErrorMsg(extractErrorMessage(error))
    setPhase('error')
  }

  const handleActivate = useCallback(async () => {
    if (activatedRef.current) return
    activatedRef.current = true
    setPhase('activating')
    setErrorMsg(null)
    try {
      await api.activateBuiltinLlm()
      setPhase('ready')
      onActivated?.()
    } catch (error) {
      // Allow retry on failure.
      activatedRef.current = false
      failWith('activate', error)
    }
  }, [onActivated])

  // Reset per-open-session state. If the model is already installed when the
  // wizard reopens, jump straight to ready instead of re-downloading.
  useEffect(() => {
    if (open) {
      activatedRef.current = false
      sawDownloadingRef.current = false
      setErrorMsg(null)
      setPhase(status?.installed ? 'ready' : 'loading')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // Poll status while open. Task 15 replaces this with a WS subscription.
  useEffect(() => {
    if (!open) return
    let cancelled = false
    const poll = async () => {
      try {
        const s = await api.getBuiltinLlmStatus()
        if (!cancelled) setStatus(s)
      } catch {
        // Transient poll failure: keep the last good status rather than
        // flipping the wizard into error (a download may be mid-flight).
      }
    }
    poll()
    const timer = window.setInterval(poll, STATUS_POLL_MS)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [open])

  // Derive phase + progress from the polled status.
  useEffect(() => {
    if (!open || !status) return
    const s = status

    if (s.server_state === 'downloading') {
      sawDownloadingRef.current = true
      setProgress({
        percent: calcPercent(s.downloaded_bytes, s.total_bytes),
        downloadedBytes: s.downloaded_bytes ?? 0,
        totalBytes: s.total_bytes,
      })
      setPhase('downloading')
      return
    }

    if (s.installed) {
      setProgress({
        percent: 100,
        downloadedBytes: s.downloaded_bytes ?? 0,
        totalBytes: s.total_bytes,
      })
      // 下载完成自动激活 — but only when nothing else is active ("有后端不抢")
      // and only when the bundled server is actually running (activating a
      // stopped server would point chat at a dead endpoint).
      if (sawDownloadingRef.current && !activatedRef.current && s.server_state === 'running' && !hasActiveBackend) {
        void handleActivate()
        return
      }
      setPhase('ready')
      return
    }

    setPhase('idle')
  }, [status, open, hasActiveBackend, handleActivate])

  const handleStartDownload = async () => {
    setErrorMsg(null)
    try {
      await api.downloadBuiltinLlm()
      // Optimistically switch to the downloading phase; the poll confirms the
      // server state and supplies real progress numbers (even if the server
      // reports an already-running download, progress will follow).
      sawDownloadingRef.current = true
      setPhase('downloading')
    } catch (error) {
      failWith('download', error)
    }
  }

  const handleRestartEngine = async () => {
    setErrorMsg(null)
    try {
      await api.restartBuiltinLlm()
      // The poll observes running → auto-activate (when allowed) or ready.
    } catch (error) {
      failWith('restart', error)
    }
  }

  const handleRetry = () => {
    if (retryAction === 'activate') void handleActivate()
    else if (retryAction === 'restart') void handleRestartEngine()
    else void handleStartDownload()
  }

  const close = () => onOpenChange(false)

  const readyActivated = activatedRef.current || isBuiltinActive
  const engineStopped = status?.server_state === 'stopped'

  return (
    // Nested inside SettingsDialog's z-[100] overlay → z-[110] (DESIGN_SPEC
    // "Nested Dialog inside FullScreenDialog MUST use z-[110]").
    <FullScreenDialog open={open} onOpenChange={onOpenChange} zIndex={110}>
      <FullScreenDialogHeader
        title={t('plugins:llm.builtinWizardTitle')}
        onClose={close}
      />
      <FullScreenDialogContent>
        <FullScreenDialogMain className="p-4 md:p-6">
          <div className="mx-auto w-full max-w-lg space-y-6">
            {/* Model identity */}
            <div className="flex items-start gap-3">
              <div className="flex items-center justify-center h-12 w-12 rounded-xl shrink-0 bg-warning-light text-warning">
                <Cpu className="h-6 w-6" />
              </div>
              <div className="min-w-0 flex-1">
                <h2 className="text-lg font-semibold text-foreground">
                  {t('plugins:llm.builtinTitle')}
                </h2>
                <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                  {t('plugins:llm.builtinWizardIntro')}
                </p>
              </div>
            </div>

            {/* Model info: size / speed / offline */}
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <InfoTile
                icon={<HardDrive className="h-4 w-4" />}
                label={t('plugins:llm.builtinWizardSizeLabel')}
                value={t('plugins:llm.builtinWizardSizeValue')}
              />
              <InfoTile
                icon={<Zap className="h-4 w-4" />}
                label={t('plugins:llm.builtinWizardSpeedLabel')}
                value={t('plugins:llm.builtinWizardSpeedValue')}
              />
              <InfoTile
                icon={<WifiOff className="h-4 w-4" />}
                label={t('plugins:llm.builtinWizardOfflineLabel')}
                value={t('plugins:llm.builtinWizardOfflineValue')}
              />
            </div>

            {/* Phase body */}
            {phase === 'loading' && (
              <div className="flex items-center justify-center gap-2 py-8 text-muted-foreground">
                <Loader2 className="h-5 w-5 animate-spin" />
                <span className="text-sm">{t('common:loading')}</span>
              </div>
            )}

            {phase === 'idle' && (
              <div className="space-y-4 rounded-lg border border-border bg-card p-5">
                <p className="text-sm text-muted-foreground leading-relaxed">
                  {t('plugins:llm.builtinWizardIdleHint')}
                </p>
                <Button size="lg" className="w-full" onClick={handleStartDownload}>
                  <Download className="mr-2 h-4 w-4" />
                  {t('plugins:llm.builtinWizardStart')}
                </Button>
              </div>
            )}

            {phase === 'downloading' && (
              <div className="space-y-3 rounded-lg border border-border bg-card p-5">
                <div className="flex items-center justify-between gap-3">
                  <span className="text-sm font-medium text-foreground">
                    {progress.percent != null
                      ? t('plugins:llm.builtinDownloading', { percent: progress.percent })
                      : t('plugins:llm.builtinDownloadingNoProgress')}
                  </span>
                  <Download className="h-4 w-4 shrink-0 text-muted-foreground" />
                </div>
                <Progress value={progress.percent ?? 0} />
                <p className="text-xs text-muted-foreground">
                  {t('plugins:llm.builtinWizardDownloadingHint')}
                </p>
              </div>
            )}

            {phase === 'activating' && (
              <div className="flex items-center justify-center gap-2 py-8 text-muted-foreground">
                <Loader2 className="h-5 w-5 animate-spin" />
                <span className="text-sm">{t('plugins:llm.builtinWizardActivating')}</span>
              </div>
            )}

            {phase === 'ready' && (
              <div className="space-y-4 rounded-lg border border-border bg-card p-5">
                <div className="flex items-start gap-3">
                  <CheckCircle2 className="h-6 w-6 shrink-0 text-success" />
                  <div className="min-w-0 flex-1">
                    <h3 className="text-base font-semibold text-foreground">
                      {t('plugins:llm.builtinWizardReady')}
                    </h3>
                    <p className="mt-1 text-sm text-muted-foreground leading-relaxed">
                      {readyActivated
                        ? t('plugins:llm.builtinWizardReadyDesc')
                        : t('plugins:llm.builtinWizardReadyNotActive')}
                    </p>
                  </div>
                </div>
                {engineStopped && (
                  <div className="flex items-center justify-between gap-3 rounded-md bg-warning-light px-3 py-2 text-warning">
                    <span className="text-xs font-medium">
                      {t('plugins:llm.builtinWizardEngineStopped')}
                    </span>
                    <Button size="sm" variant="outline" onClick={handleRestartEngine}>
                      <RotateCcw className="mr-1.5 h-3.5 w-3.5" />
                      {t('plugins:llm.builtinWizardRestartEngine')}
                    </Button>
                  </div>
                )}
                {!readyActivated && !engineStopped && (
                  <Button size="lg" className="w-full" onClick={handleActivate}>
                    <Power className="mr-2 h-4 w-4" />
                    {t('plugins:llm.builtinWizardActivate')}
                  </Button>
                )}
              </div>
            )}

            {phase === 'error' && (
              <div className="space-y-4 rounded-lg border border-error-light bg-error-light px-4 py-3">
                <div className="flex items-start gap-3">
                  <AlertCircle className="h-5 w-5 shrink-0 text-error" />
                  <div className="min-w-0 flex-1">
                    <h3 className="text-sm font-semibold text-error">
                      {t('plugins:llm.builtinWizardErrorTitle')}
                    </h3>
                    {errorMsg && (
                      <p className="mt-1 text-xs text-muted-foreground break-words">
                        {t('plugins:llm.builtinWizardErrorDesc', { message: errorMsg })}
                      </p>
                    )}
                  </div>
                </div>
                <Button size="lg" className="w-full" variant="secondary" onClick={handleRetry}>
                  <RotateCcw className="mr-2 h-4 w-4" />
                  {t('common:retry')}
                </Button>
              </div>
            )}
          </div>
        </FullScreenDialogMain>
      </FullScreenDialogContent>

      <FullScreenDialogFooter>
        {phase === 'ready' ? (
          <Button onClick={close}>{t('common:done')}</Button>
        ) : (
          <Button variant="ghost" onClick={close}>
            {t('common:skip')}
          </Button>
        )}
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}

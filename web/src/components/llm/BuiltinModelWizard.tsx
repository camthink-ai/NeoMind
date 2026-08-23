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
 * Live `ModelDownloadProgress` events over the WS/SSE event stream feed the
 * small `progress` state below (Task 15). That event type is in NO
 * `is_*_event()` category, so it is received on the unfiltered 'all' stream
 * rather than the 'llm' category. `GET /api/builtin-llm/status` polling remains
 * as a fallback/resume source (e.g. the socket was disconnected when the
 * download started). The view reads ONLY the small `progress` state below, so
 * the render is independent of which source supplied it.
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
import { cn } from '@/lib/utils'
import { Progress } from '@/components/ui/progress'
import {
  AlertCircle,
  BrainCircuit,
  Info,
  CheckCircle2,
  Cpu,
  Download,
  HardDrive,
  Loader2,
  Power,
  RotateCcw,
  WifiOff,
  Zap,
  AlertTriangle,
} from 'lucide-react'
import { api, isTauriEnv } from '@/lib/api'
import { useEvents } from '@/hooks/useEvents'
import type { ModelDownloadProgressEvent } from '@/lib/events'
import type { BuiltinLlmStatus, BuiltinModelDef } from '@/types'

/**
 * Platform-specific runtime guidance for the download wizard.
 *
 * The runtime download only happens where no bundled llama-server exists
 * (bare server installs / dev) — the Tauri desktop ships one, so hints are
 * desktop-hidden. Browser access == server deployment, exactly the audience
 * that needs the CUDA/Jetson pointers.
 */
type PlatformHintKey = 'platformHintLinux' | 'platformHintLinuxArm' | 'platformHintWindows' | null

function detectPlatformHint(): PlatformHintKey {
  if (typeof navigator === 'undefined' || isTauriEnv()) return null
  const platform = navigator.platform || ''
  const ua = navigator.userAgent
  if (/Win/i.test(platform) || /Windows/.test(ua)) {
    // Windows CUDA hint is x64-only (win-cuda asset); ARM64 Windows runs CPU.
    return /arm64|aarch64/i.test(platform) ? null : 'platformHintWindows'
  }
  if (/Mac/.test(platform) || /Macintosh/.test(ua)) return null // Metal just works
  if (/Linux/.test(ua) || /Linux/i.test(platform)) {
    return /arm64|aarch64/i.test(platform + ' ' + ua)
      ? 'platformHintLinuxArm'
      : 'platformHintLinux'
  }
  return null
}

const STATUS_POLL_MS = 2000

type WizardPhase = 'loading' | 'idle' | 'downloading' | 'starting' | 'activating' | 'ready' | 'error'

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
  /** Open directly at the model picker (切换模型 entry) instead of the
   *  ready screen — the card's switch button uses this. */
  startInPicker?: boolean
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
  startInPicker = false,
}: BuiltinModelWizardProps) {
  const { t } = useTranslation(['plugins', 'common'])

  const [phase, setPhase] = useState<WizardPhase>('loading')
  const [status, setStatus] = useState<BuiltinLlmStatus | null>(null)
  const [models, setModels] = useState<BuiltinModelDef[]>([])
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null)
  // Model the ready-phase tiles describe: the installed one, else selection.
  const shownModel = models.find((m) => m.id === (status?.model_id ?? selectedModelId))
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
  // Set when a download failure is surfaced (WS error event or server_state
  // 'error'). Prevents the status poll from silently resetting the wizard to
  // idle after a failed download leaves the manifest unwritten (not_configured).
  const downloadFailedRef = useRef(false)
  // Timestamp of the last live WS progress event — the poll fills progress gaps
  // only when this is stale (Task 15).
  const wsProgressAtRef = useRef(0)
  // When the download completed (WS 'complete' / 100% poll) — bounds the
  // 'starting' phase so a permanently failed spawn can't spin forever.
  const completedAtRef = useRef(0)
  // True while the user explicitly chose 切换模型 and is browsing the model
  // picker. The status poll re-runs the phase-derivation effect every 2s and
  // would otherwise yank an installed model's user straight back to 'ready'
  // before they can pick another one.
  const browsingPickerRef = useRef(false)

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
  // wizard reopens, jump straight to ready instead of re-downloading —
  // unless the switch-model entry asked for the picker directly.
  useEffect(() => {
    if (open) {
      activatedRef.current = false
      sawDownloadingRef.current = false
      downloadFailedRef.current = false
      browsingPickerRef.current = startInPicker
      wsProgressAtRef.current = 0
      setErrorMsg(null)
      setPhase(startInPicker ? 'idle' : status?.installed ? 'ready' : 'loading')
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  // Live download progress over WS/SSE (Task 15). ModelDownloadProgress is in
  // NO is_*_event() category, so the 'llm' category filter would never deliver
  // it — subscribe on the unfiltered 'all' stream and client-filter to this
  // type (the 'all' connection is shared, so this adds no extra socket). The
  // status poll below remains a fallback/resume source.
  useEvents({
    category: 'all',
    eventTypes: ['ModelDownloadProgress'],
    enabled: open,
    onEvent: (event) => {
      if (event.type !== 'ModelDownloadProgress') return
      const d = (event as ModelDownloadProgressEvent).data
      // total/error are Option on the backend: total serializes as null when
      // unknown and error is null/absent on success — tolerate both.
      if (d.status === 'downloading') {
        sawDownloadingRef.current = true
        wsProgressAtRef.current = Date.now()
        setProgress({
          percent: calcPercent(d.downloaded, d.total),
          downloadedBytes: d.downloaded ?? 0,
          totalBytes: d.total ?? null,
        })
        setPhase('downloading')
      } else if (d.status === 'complete') {
        completedAtRef.current = Date.now()
        // Show 100% immediately; the poll observes installed + running and
        // drives the ready / auto-activate transition.
        setProgress({
          percent: 100,
          downloadedBytes: d.downloaded ?? 0,
          totalBytes: d.total ?? null,
        })
      } else if (d.status === 'error') {
        // Mid-download failure: surface it NOW. The poll reports not_configured
        // (manifest unwritten), so without this branch the wizard would sit in
        // downloading with frozen progress then silently reset to idle.
        downloadFailedRef.current = true
        setRetryAction('download')
        setErrorMsg(d.error ?? t('plugins:llm.builtinDownloadFailed'))
        setPhase('error')
      }
    },
  })

  // Poll status while open — fallback/resume source for progress (e.g. the
  // socket was disconnected when the download started) and the authority for
  // phase transitions (installed/running/error).
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
    api
      .getBuiltinModels()
      .then((r) => {
        setModels(r.models)
        // Pre-select: already-installed → recommended → default id → first.
        const preferred =
          r.models.find((m) => m.installed) ??
          r.models.find((m) => m.recommended) ??
          r.models.find((m) => m.id === r.default_model_id) ??
          r.models[0]
        if (preferred) setSelectedModelId(preferred.id)
      })
      .catch(() => {
        // Transient — the picker just stays empty and the poll retries status.
      })
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
      // WS delivers live progress; only fall back to the poll when WS has been
      // silent (e.g. it wasn't connected when the download started) so a fresh
      // WS value is not clobbered by a slightly older poll snapshot.
      if (Date.now() - wsProgressAtRef.current > STATUS_POLL_MS) {
        setProgress({
          percent: calcPercent(s.downloaded_bytes, s.total_bytes),
          downloadedBytes: s.downloaded_bytes ?? 0,
          totalBytes: s.total_bytes,
        })
      }
      setPhase('downloading')
      return
    }

    if (s.server_state === 'error') {
      // Manifest read failed / server reports an error state. The poll has no
      // error message field, so surface a generic message (a specific one from
      // the live WS error event, if any, is preserved via downloadFailedRef).
      downloadFailedRef.current = true
      setRetryAction('download')
      setErrorMsg((e) => e ?? t('plugins:llm.builtinDownloadFailed'))
      setPhase('error')
      return
    }

    if (s.installed) {
      // User is browsing the picker (switch model) — leave them there; a
      // poll snapshot of the already-installed model must not bounce the
      // UI back to 'ready'. The guard clears on download start / reopen.
      if (browsingPickerRef.current) return
      if (completedAtRef.current === 0) completedAtRef.current = Date.now()
      setProgress({
        percent: 100,
        downloadedBytes: s.downloaded_bytes ?? 0,
        totalBytes: s.total_bytes,
      })
      // Model is on disk but the bundled server isn't up yet — the post-
      // download spawn (incl. a first-time llama-server runtime download) is
      // still in flight. Show "starting" for at most 90s; a spawn that failed
      // permanently leaves status at 'stopped' forever, and an endless
      // spinner with no error is worse than landing on ready (which carries
      // the restart-engine button for exactly this case).
      if (
        sawDownloadingRef.current &&
        !activatedRef.current &&
        s.server_state !== 'running' &&
        Date.now() - completedAtRef.current < 90_000
      ) {
        setPhase('starting')
        return
      }
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

    // not_configured / stopped: a failed download leaves the manifest unwritten,
    // so the poll reports not_configured — never clobber an error the WS already
    // surfaced back into idle.
    if (!downloadFailedRef.current) {
      setPhase('idle')
    }
  }, [status, open, hasActiveBackend, handleActivate])

  const handleStartDownload = async (modelId?: string) => {
    setErrorMsg(null)
    downloadFailedRef.current = false
    browsingPickerRef.current = false
    try {
      await api.downloadBuiltinLlm(modelId)
      // Optimistically switch to the downloading phase; live WS events supply
      // real progress numbers (and the poll confirms/resumes if the socket is
      // not delivering).
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
          <div className="mx-auto w-full max-w-3xl space-y-6">
            {/* Model identity */}
            <div className="flex items-start gap-3">
              <div className="flex items-center justify-center h-12 w-12 rounded-xl shrink-0 bg-accent-indigo-light text-accent-indigo">
                <BrainCircuit className="h-6 w-6" />
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

            {/* Model info: size / context / offline — values come from the
                installed/selected model def, not static copy (hardware speed
                claims were dropped: edge tok/s vary 20x, a number misleads). */}
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
              <InfoTile
                icon={<HardDrive className="h-4 w-4" />}
                label={t('plugins:llm.builtinWizardSizeLabel')}
                value={
                  shownModel
                    ? `${(shownModel.size_bytes / 1e9).toFixed(1)}GB · ${shownModel.quant}`
                    : t('plugins:llm.builtinWizardSizeValue')
                }
              />
              <InfoTile
                icon={<Zap className="h-4 w-4" />}
                label={t('plugins:llm.builtinWizardCtxLabel')}
                value={
                  shownModel
                    ? `${Math.round(shownModel.default_ctx / 1024)}K context`
                    : ''
                }
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
              <div className="space-y-3 rounded-lg border border-border bg-card p-5">
                <p className="text-sm text-muted-foreground leading-relaxed">
                  {t('plugins:llm.builtinWizardIdleHint')}
                </p>
                {(() => {
                  // Static per session — compute once, no reactive need.
                  const hint = detectPlatformHint()
                  return hint ? (
                    <div className="flex items-start gap-1.5 rounded-md bg-muted-30 px-2.5 py-2">
                      <Info className="h-3.5 w-3.5 shrink-0 mt-0.5 text-muted-foreground" />
                      <span className="text-[11px] leading-snug text-muted-foreground">
                        {t(`plugins:llm.${hint}`)}
                      </span>
                    </div>
                  ) : null
                })()}
                {/* Model picker — one builtin model at a time; pick then
                    download (installed model is marked and pre-selected). */}
                <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
                  {models.length === 0 && (
                    <div className="col-span-full text-xs text-muted-foreground text-center py-2">
                      {t('plugins:llm.builtinWizardNoModels')}
                    </div>
                  )}
                  {models.map((m) => {
                    const selected = selectedModelId === m.id
                    // Display copy is i18n'd on the frontend (fall back to the
                    // backend's notes for non-i18n API consumers).
                    const modelName = t(`plugins:llm.models.${m.id}.name`, { defaultValue: m.name })
                    const modelNotes = t(`plugins:llm.models.${m.id}.notes`, { defaultValue: m.notes })
                    return (
                      <button
                        key={m.id}
                        onClick={() => setSelectedModelId(m.id)}
                        className={cn(
                          'flex flex-col items-start gap-2 rounded-lg border p-3 text-left transition-colors',
                          selected
                            ? 'border-primary bg-primary-light'
                            : 'border-border hover:border-primary'
                        )}
                      >
                        <div className="flex items-center gap-2 min-w-0">
                          <span className="text-sm font-medium truncate">{modelName}</span>
                          {m.recommended && (
                            <span className="shrink-0 rounded-full bg-primary px-1.5 py-0.5 text-[10px] font-medium text-primary-foreground">
                              {t('common:llmGuide.recommended')}
                            </span>
                          )}
                        </div>
                        <p className="text-xs text-muted-foreground leading-relaxed">
                          {modelNotes}
                        </p>
                        <div className="mt-auto flex items-center gap-2">
                          <span className="text-xs text-muted-foreground">
                            {(m.size_bytes / 1e9).toFixed(1)} GB · {m.quant}
                          </span>
                          {m.installed && (
                            <span className="rounded-full bg-success-light px-1.5 py-0.5 text-[10px] font-medium text-success">
                              {t('plugins:llm.installed')}
                            </span>
                          )}
                        </div>
                        {m.memory_ok === false && (
                          <div className="flex items-start gap-1.5 rounded-md bg-warning-light px-2 py-1.5 text-warning">
                            <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
                            <span className="text-[11px] leading-snug">
                              {t('plugins:llm.builtinMemLow', {
                                min: (m.min_ram_mb / 1024).toFixed(0),
                              })}
                            </span>
                          </div>
                        )}
                      </button>
                    )
                  })}
                </div>
                <Button
                  size="lg"
                  className="w-full"
                  disabled={!selectedModelId}
                  onClick={() => handleStartDownload(selectedModelId ?? undefined)}
                >
                  <Download className="mr-2 h-4 w-4" />
                  {status?.installed
                    ? t('plugins:llm.switchModelCta')
                    : t('plugins:llm.builtinWizardStart')}
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

            {phase === 'starting' && (
              <div className="flex items-center justify-center gap-2 py-8 text-muted-foreground">
                <Loader2 className="h-5 w-5 animate-spin" />
                <span className="text-sm">{t('plugins:llm.builtinWizardStarting')}</span>
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
          <>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                browsingPickerRef.current = true
                setPhase('idle')
              }}
              className="gap-1.5"
            >
              <Download className="h-3.5 w-3.5" />
              {t('plugins:llm.switchModel')}
            </Button>
            <Button onClick={close}>{t('common:done')}</Button>
          </>
        ) : (
          <Button variant="ghost" onClick={close}>
            {t('common:skip')}
          </Button>
        )}
      </FullScreenDialogFooter>
    </FullScreenDialog>
  )
}

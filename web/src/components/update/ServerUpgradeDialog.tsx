/**
 * ServerUpgradeDialog Component
 *
 * Web-triggered server self-upgrade for browser (non-Tauri) access to a
 * server deployment — the counterpart of the Tauri-desktop UpdateDialog,
 * which uses the desktop OTA channel instead.
 *
 * On open it fetches a release check (`/api/system/upgrade/check`); when a
 * new version is available (and the deployment supports it), confirm kicks
 * off `POST /api/system/upgrade`. Progress comes from two sources, mirroring
 * the builtin-model wizard: `SystemUpgradeProgress` WS events (unfiltered
 * 'all' stream — the event belongs to no backend category) plus a 2s status
 * poll fallback, which is the only source during the restart window when
 * the WS is down. When the server answers again with the target version,
 * the page reloads (index.html is served no-cache, so the reload picks up
 * the new frontend too).
 */

import { useState, useEffect, useCallback, useRef, lazy, Suspense } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { AlertCircle, Check, Download, Loader2, Server } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { useEvents } from '@/hooks/useEvents'
import { api, type ServerUpgradeCheck, type ServerUpgradePhase } from '@/lib/api'
import type { SystemUpgradeProgressEvent } from '@/lib/events'
import { notifySuccess } from '@/lib/notify'
import type { Components } from 'react-markdown'
import { cn } from '@/lib/utils'
import { releaseNotesProse } from './releaseNotesProse'

// Lazy: keeps the markdown/highlight vendor chunk out of the initial graph
// (same chunk UpdateDialog shares).
const ReleaseNotes = lazy(() =>
  import('./ReleaseNotes').then((m) => ({ default: m.ReleaseNotes }))
)

const releaseNotesComponents: Components = {
  a: ({ className, children, href, ...props }) => (
    <a
      className={cn('text-primary underline underline-offset-2 hover:opacity-80', className)}
      href={href as string}
      target="_blank"
      rel="noopener noreferrer"
      {...(props as any)}
    >
      {children}
    </a>
  ),
}

/** Poll the in-flight upgrade status + wait for the restarted backend. */
const STATUS_POLL_MS = 2000
/** Give the restarted server this long to come back before declaring failure. */
const RESTART_WAIT_TIMEOUT_MS = 5 * 60 * 1000
/** Pause after "complete" before reloading, so the user sees the success card. */
const RELOAD_DELAY_MS = 1500

type Stage = 'checking' | 'idle' | 'upgrading' | 'restarting' | 'done' | 'error'

export interface ServerUpgradeDialogProps {
  open: boolean
  onClose: () => void
}

export function ServerUpgradeDialog({ open, onClose }: ServerUpgradeDialogProps) {
  const { t } = useTranslation(['common', 'settings'])

  const [stage, setStage] = useState<Stage>('checking')
  const [check, setCheck] = useState<ServerUpgradeCheck | null>(null)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [phase, setPhase] = useState<ServerUpgradePhase>('idle')
  const [downloaded, setDownloaded] = useState(0)
  const [total, setTotal] = useState(0)

  // Guard against double-start and double-reload across poll/event paths.
  const startedRef = useRef(false)
  const reloadRef = useRef(false)

  // ---- Check on open ----------------------------------------------------
  useEffect(() => {
    if (!open) return
    startedRef.current = false
    reloadRef.current = false
    setStage('checking')
    setCheck(null)
    setErrorMsg(null)
    setPhase('idle')
    setDownloaded(0)
    setTotal(0)

    api
      .checkServerUpgrade(true)
      .then((result) => {
        setCheck(result)
        setStage('idle')
      })
      .catch((e) => {
        setErrorMsg(e instanceof Error ? e.message : String(e))
        setStage('error')
      })
  }, [open])

  // ---- Progress via WS events (unfiltered 'all' stream) -----------------
  useEvents({
    category: 'all',
    eventTypes: ['SystemUpgradeProgress'],
    enabled: open,
    onEvent: (event) => {
      if (event.type !== 'SystemUpgradeProgress') return
      const d = (event as SystemUpgradeProgressEvent).data
      setPhase(d.phase)
      if (typeof d.downloaded === 'number') setDownloaded(d.downloaded)
      if (typeof d.total === 'number' && d.total > 0) setTotal(d.total)
      if (d.phase === 'restarting') setStage((s) => (s === 'upgrading' ? 'restarting' : s))
      if (d.phase === 'error') {
        setErrorMsg(d.error ?? t('settings:serverUpgradeFailed'))
        setStage('error')
      }
    },
  })

  // ---- Status polling fallback (also the only channel during restart) ---
  useEffect(() => {
    if (!open || (stage !== 'upgrading' && stage !== 'restarting')) return

    const id = setInterval(async () => {
      try {
        const status = await api.getServerUpgradeStatus()
        if (status.phase === 'error') {
          setErrorMsg(status.error ?? t('settings:serverUpgradeFailed'))
          setStage('error')
          return
        }
        if (stage === 'upgrading') {
          setPhase(status.phase)
          setDownloaded(status.downloaded)
          if (status.total > 0) setTotal(status.total)
          if (status.phase === 'restarting') setStage('restarting')
        }
      } catch {
        // Backend unreachable — expected while it restarts; the
        // 'restarting' branch below handles recovery.
      }
    }, STATUS_POLL_MS)
    return () => clearInterval(id)
  }, [open, stage, t])

  // ---- Wait for the restarted backend, then reload -----------------------
  useEffect(() => {
    if (!open || stage !== 'restarting' || !check?.latest_version) return

    const target = check.latest_version
    const startedAt = Date.now()
    const id = setInterval(async () => {
      if (Date.now() - startedAt > RESTART_WAIT_TIMEOUT_MS) {
        setErrorMsg(
          `${t('settings:serverRestartingDesc')} (${t('settings:serverUpgradeFailed')})`
        )
        setStage('error')
        return
      }
      try {
        const stats = await api.getSystemStats()
        // Version may briefly still be the old one mid-restart window;
        // only proceed when the new binary is actually serving.
        if (stats.version === target || stats.version === `v${target}`) {
          clearInterval(id)
          if (reloadRef.current) return
          reloadRef.current = true
          // Same marker the Tauri flow uses — the next check (auto or
          // manual) toasts "update applied" instead of re-offering it.
          localStorage.setItem('neomind_installed_version', target)
          notifySuccess(t('settings:updateApplied'), t('settings:serverUpgradeCompleteTitle'))
          setStage('done')
          setTimeout(() => window.location.reload(), RELOAD_DELAY_MS)
        }
      } catch {
        // Still down — keep waiting until the timeout above.
      }
    }, STATUS_POLL_MS)
    return () => clearInterval(id)
  }, [open, stage, check, t])

  // ---- Actions -----------------------------------------------------------
  const handleUpgrade = useCallback(async () => {
    if (startedRef.current) return
    startedRef.current = true
    setStage('upgrading')
    setPhase('checking')
    setErrorMsg(null)
    try {
      await api.startServerUpgrade(check?.latest_version ?? undefined)
    } catch (e) {
      startedRef.current = false
      setErrorMsg(e instanceof Error ? e.message : String(e))
      setStage('error')
    }
  }, [check])

  // ---- Derived UI state ---------------------------------------------------
  const canClose = stage === 'checking' || stage === 'idle' || stage === 'error' || stage === 'done'
  const busy = stage === 'upgrading' || stage === 'restarting'

  const formatBytes = (bytes: number) => {
    if (!bytes) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1)
    return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i]
  }

  const getProgressPercent = () => {
    if (!total) return stage === 'restarting' || stage === 'done' ? 100 : 0
    return Math.min(100, Math.round((downloaded / total) * 100))
  }

  const phaseMessage = () => {
    switch (phase) {
      case 'checking':
        return t('settings:checkingForUpdates')
      case 'downloading':
        return t('settings:downloadingUpdate')
      case 'verifying':
        return t('settings:serverUpgradeVerifying')
      case 'staged':
      case 'applying':
        return t('settings:serverUpgradeStaged')
      default:
        return t('settings:updating')
    }
  }

  const getTitle = () => {
    switch (stage) {
      case 'checking':
        return t('settings:checkingForUpdates')
      case 'upgrading':
        return t('settings:serverUpgrading')
      case 'restarting':
        return t('settings:serverUpgrading')
      case 'done':
        return t('settings:serverUpgradeCompleteTitle')
      case 'error':
        return t('settings:serverUpgradeFailed')
      default:
        return check?.available
          ? t('settings:newVersionAvailable')
          : t('settings:serverUpgrade')
    }
  }

  const dialogIcon = (() => {
    switch (stage) {
      case 'done':
        return <Check className="h-4 w-4 text-success" strokeWidth={2} />
      case 'error':
        return <AlertCircle className="h-4 w-4 text-error" strokeWidth={1.75} />
      case 'checking':
      case 'upgrading':
      case 'restarting':
        return <Loader2 className="h-4 w-4 text-foreground animate-spin" />
      default:
        return <Server className="h-4 w-4 text-foreground" strokeWidth={1.75} />
    }
  })()

  const upgradable = !!check?.available && check.supported
  const showHint = !!check && !check.supported
  const hasReleaseNotes = upgradable && stage === 'idle' && !!check?.release_notes
  // Checking and the final reload need no actions: the header already states
  // the state, so an extra disabled button (plus its divider) was pure noise.
  const hideFooter = stage === 'checking' || stage === 'done'

  const footerContent = busy ? (
    <Button disabled variant="secondary" size="sm">
      <Loader2 className="w-3.5 h-3.5 mr-1.5 animate-spin" />
      {t('settings:updating')}
    </Button>
  ) : stage === 'error' ? (
    <>
      <Button variant="outline" size="sm" onClick={onClose}>
        {t('common:close')}
      </Button>
      <Button
        variant="default"
        size="sm"
        onClick={() => {
          // Re-run the open-effect by cycling through checking state.
          setStage('checking')
          api
            .checkServerUpgrade(true)
            .then((result) => {
              setCheck(result)
              startedRef.current = false
              setStage('idle')
            })
            .catch((e) => {
              setErrorMsg(e instanceof Error ? e.message : String(e))
              setStage('error')
            })
        }}
      >
        {t('common:retry')}
      </Button>
    </>
  ) : (
    <>
      {upgradable && (
        <Button variant="ghost" size="sm" onClick={onClose} className="text-muted-foreground">
          {t('settings:remindLater')}
        </Button>
      )}
      {upgradable ? (
        <Button size="sm" onClick={handleUpgrade} className="gap-1.5">
          <Download className="w-3.5 h-3.5" />
          {t('settings:updateNow')}
        </Button>
      ) : (
        <Button variant="outline" size="sm" onClick={onClose}>
          {t('common:close')}
        </Button>
      )}
    </>
  )

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !canClose) return
        onClose()
      }}
      title={getTitle()}
      icon={dialogIcon}
      width={hasReleaseNotes ? 'lg' : 'md'}
      // Above SettingsDialog (z-[100]): this dialog opens FROM the About
      // section while the settings overlay stays mounted — the default z-50
      // would leave the confirm button covered by the settings page.
      className="z-[110]"
      // Keep the default preventCloseOnSubmit=true: while an upgrade is in
      // flight, UnifiedFormDialog's own guards disable the X / Esc / overlay
      // / cancel paths (defense in depth on top of the canClose check in
      // onOpenChange below, which stays the authoritative gate).
      isSubmitting={busy}
      hideFooter={hideFooter}
      footer={footerContent}
    >
      <div className="space-y-4">
        {/* Version banner — pinned above scrollable release notes */}
        {check && (
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-border bg-muted-20 px-3.5 py-2.5 text-xs">
            <span className="text-muted-foreground">{t('settings:currentVersion')}</span>
            <div className="flex items-center gap-1.5 font-mono tabular-nums">
              <span className="text-muted-foreground">{`v${check.current_version}`}</span>
              {check.latest_version && (
                <>
                  <span aria-hidden="true" className="text-muted-foreground">→</span>
                  <span className="rounded-md border border-border bg-card px-1.5 py-0.5 font-medium text-foreground">
                    {`v${check.latest_version}`}
                  </span>
                </>
              )}
            </div>
          </div>
        )}

        {/* Deployment hint (docker / unsupported / helper missing) */}
        {showHint && stage === 'idle' && check?.notes && (
          <div className="flex items-start gap-2 rounded-xl border border-border bg-muted-20 px-3.5 py-3">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <p className="text-xs leading-relaxed text-muted-foreground whitespace-pre-line">{check.notes}</p>
          </div>
        )}

        {/* Release notes — independently scrollable reading surface so long
            changelogs never distort the dialog proportion */}
        {hasReleaseNotes && (
          <div className="max-h-[46vh] overflow-y-auto rounded-xl border border-border bg-card px-4 py-3.5">
            <div className={releaseNotesProse}>
              <Suspense fallback={null}>
                <ReleaseNotes body={check.release_notes!} components={releaseNotesComponents} />
              </Suspense>
            </div>
          </div>
        )}

        {/* Up-to-date note */}
        {stage === 'idle' && check && !check.available && (
          <p className="text-xs text-muted-foreground">{t('settings:alreadyUpToDate')}</p>
        )}

        {/* Progress bar */}
        {(stage === 'upgrading' || stage === 'restarting') && (
          <div className="space-y-2">
            {stage === 'upgrading' && (
              <p className="text-xs text-muted-foreground">{phaseMessage()}</p>
            )}
            <Progress value={getProgressPercent()} className="h-1.5" />
            {stage === 'upgrading' && total > 0 && (
              <div className="flex justify-end text-xs tabular-nums text-muted-foreground">
                <span className="whitespace-nowrap">
                  {formatBytes(downloaded)} / {formatBytes(total)} · {getProgressPercent()}%
                </span>
              </div>
            )}
            {stage === 'restarting' && (
              <p className="text-xs text-muted-foreground">
                {t('settings:serverRestartingDesc')}
              </p>
            )}
          </div>
        )}

        {/* Success card */}
        {stage === 'done' && (
          <div className="flex items-center gap-2 rounded-lg bg-success-light px-3 py-2.5">
            <Check className="h-4 w-4 shrink-0 text-success" />
            <p className="text-body text-success">{t('settings:updateCompleteMessage')}</p>
          </div>
        )}

        {/* Error card */}
        {stage === 'error' && (
          <div className="flex items-start gap-2 rounded-lg bg-error-light px-3 py-2.5">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-error" />
            <p className="whitespace-pre-line break-all text-body text-error">
              {errorMsg ?? t('settings:serverUpgradeFailed')}
            </p>
          </div>
        )}
      </div>
    </UnifiedFormDialog>
  )
}

export default ServerUpgradeDialog

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
import { Badge } from '@/components/ui/badge'
import { AlertCircle, Check, Download, Loader2, Rocket, Server } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { useEvents } from '@/hooks/useEvents'
import { api, type ServerUpgradeCheck, type ServerUpgradePhase } from '@/lib/api'
import type { SystemUpgradeProgressEvent } from '@/lib/events'
import { notifySuccess } from '@/lib/notify'
import type { Components } from 'react-markdown'
import { cn } from '@/lib/utils'

// Lazy: keeps the markdown/highlight vendor chunk out of the initial graph
// (same chunk UpdateDialog shares).
const ReleaseNotes = lazy(() =>
  import('./ReleaseNotes').then((m) => ({ default: m.ReleaseNotes }))
)

const releaseNotesComponents: Components = {
  a: ({ node, className, children, href, ...props }) => (
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

  const getDescription = () => {
    switch (stage) {
      case 'checking':
        return t('settings:checkingForUpdates')
      case 'upgrading':
      case 'restarting':
        return phaseMessage()
      case 'done':
        return t('settings:serverUpgradeCompleteDesc')
      case 'error':
        return errorMsg ?? t('settings:serverUpgradeFailed')
      default:
        return t('settings:serverUpgradeDesc')
    }
  }

  const getStatusIcon = () => {
    switch (stage) {
      case 'done':
        return <Check className="w-5 h-5" />
      case 'error':
        return <AlertCircle className="w-5 h-5" />
      case 'checking':
      case 'upgrading':
      case 'restarting':
        return <Loader2 className="w-5 h-5 animate-spin" />
      default:
        return <Server className="w-5 h-5" />
    }
  }

  const getStatusColor = () => {
    switch (stage) {
      case 'done':
        return 'bg-success-light text-success'
      case 'error':
        return 'bg-error-light text-error'
      default:
        return 'bg-info-light text-info'
    }
  }

  const dialogIcon = (
    <div className={`flex items-center justify-center w-10 h-10 rounded-full ${getStatusColor()}`}>
      {getStatusIcon()}
    </div>
  )

  const upgradable = !!check?.available && check.supported
  const showHint = !!check && !check.supported

  const footerContent =
    stage === 'checking' ? (
      <Button disabled variant="secondary">
        <Loader2 className="w-4 h-4 mr-2 animate-spin" />
        {t('settings:checkingForUpdates')}
      </Button>
    ) : busy ? (
      <Button disabled variant="secondary">
        <Loader2 className="w-4 h-4 mr-2 animate-spin" />
        {t('settings:updating')}
      </Button>
    ) : stage === 'error' ? (
      <>
        <Button variant="outline" onClick={onClose}>
          {t('common:close')}
        </Button>
        <Button
          variant="default"
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
        <Button variant="ghost" onClick={onClose} className="text-muted-foreground">
          {t('settings:remindLater')}
        </Button>
        {upgradable ? (
          <Button onClick={handleUpgrade} className="gap-2">
            <Download className="w-4 h-4" />
            {t('settings:updateNow')}
          </Button>
        ) : (
          <Button variant="outline" onClick={onClose}>
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
      description={getDescription()}
      icon={dialogIcon}
      width="sm"
      // Above SettingsDialog (z-[100]): this dialog opens FROM the About
      // section while the settings overlay stays mounted — the default z-50
      // would leave the confirm button covered by the settings page.
      className="z-[110]"
      preventCloseOnSubmit={false}
      isSubmitting={busy}
      footer={footerContent}
    >
      <div className="space-y-4">
        {/* Version line */}
        {check && (
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <span className="text-muted-foreground">{t('settings:currentVersion')}</span>
            <Badge variant="secondary" className="font-mono">
              v{check.current_version}
            </Badge>
            {check.latest_version && (
              <>
                <span className="text-muted-foreground">→</span>
                <span className="text-muted-foreground">{t('settings:targetVersion')}</span>
                <Badge variant={check.available ? 'default' : 'secondary'} className="font-mono">
                  v{check.latest_version}
                </Badge>
              </>
            )}
          </div>
        )}

        {/* Deployment hint (docker / unsupported / helper missing) */}
        {showHint && stage === 'idle' && check?.notes && (
          <div className="flex items-start gap-2 p-3 rounded-md bg-muted border">
            <AlertCircle className="w-5 h-5 text-info mt-0.5 shrink-0" />
            <p className="text-sm whitespace-pre-line">{check.notes}</p>
          </div>
        )}

        {/* Release notes */}
        {upgradable && stage === 'idle' && check?.release_notes && (
          <div className="max-h-[40vh] overflow-y-auto rounded-md border p-3 text-sm">
            <div className="prose prose-sm dark:prose-invert max-w-none">
              <Suspense fallback={null}>
                <ReleaseNotes
                  body={check.release_notes}
                  components={releaseNotesComponents}
                />
              </Suspense>
            </div>
          </div>
        )}

        {/* Progress bar */}
        {(stage === 'upgrading' || stage === 'restarting') && (
          <div className="space-y-2">
            <Progress value={getProgressPercent()} className="h-2" />
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>{stage === 'restarting' ? t('settings:serverRestartingDesc') : phaseMessage()}</span>
              {stage === 'upgrading' && total > 0 && (
                <span className="whitespace-nowrap">
                  {formatBytes(downloaded)} / {formatBytes(total)} ({getProgressPercent()}%)
                </span>
              )}
            </div>
          </div>
        )}

        {/* Success card */}
        {stage === 'done' && (
          <div className="flex items-center gap-2 p-3 rounded-md bg-success-light border border-success-light dark:border-success-light">
            <Check className="w-5 h-5 text-success" />
            <p className="text-sm text-success">{t('settings:updateCompleteMessage')}</p>
          </div>
        )}

        {/* Error card */}
        {stage === 'error' && (
          <div className="flex items-start gap-2 p-3 rounded-md bg-error-light border border-error">
            <AlertCircle className="w-5 h-5 text-error mt-0.5" />
            <p className="text-sm text-error break-all">
              {errorMsg ?? t('settings:serverUpgradeFailed')}
            </p>
          </div>
        )}

        {/* Small hint under the confirm button context */}
        {upgradable && stage === 'idle' && (
          <p className="text-xs text-muted-foreground flex items-center gap-1">
            <Rocket className="w-3 h-3" />
            {t('settings:serverRestartingDesc')}
          </p>
        )}
      </div>
    </UnifiedFormDialog>
  )
}

export default ServerUpgradeDialog

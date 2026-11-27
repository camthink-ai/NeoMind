/**
 * UpdateDialog Component
 *
 * Dialog for displaying available updates and managing
 * the download and installation process.
 */

import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { Check, Download, AlertCircle, Loader2, Rocket } from 'lucide-react'
import { useUpdateCheck } from '@/hooks/useUpdateCheck'
import { useAppStore } from '@/store'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { lazy, Suspense } from 'react'
import type { Components } from 'react-markdown'

// Lazy: keeps the markdown/highlight vendor chunk out of the initial graph
const ReleaseNotes = lazy(() =>
  import('./ReleaseNotes').then((m) => ({ default: m.ReleaseNotes }))
)
import { cn } from '@/lib/utils'
import { releaseNotesProse } from './releaseNotesProse'

// react-markdown overrides for the release-notes panel. Links open in a new
// tab so the GitHub release page opens in the browser instead of navigating
// the desktop webview.
const releaseNotesComponents: Components = {
  a: ({ node: _node, className, children, href, ...props }) => (
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

export interface UpdateDialogProps {
  /** Whether the dialog is open */
  open: boolean
  /** Called when the dialog is closed */
  onClose: () => void
}

export function UpdateDialog({ open, onClose }: UpdateDialogProps) {
  const { t } = useTranslation(['common', 'settings'])
  const { updateInfo, downloadProgress } = useAppStore()

  // Stable empty callback to prevent unnecessary re-renders
  const handleUpdateAvailable = useCallback(() => {
    // Dialog is already open, no action needed
  }, [])

  const { downloadAndInstall, relaunchApp } = useUpdateCheck({
    autoCheck: false,
    onUpdateAvailable: handleUpdateAvailable,
  })

  const [installStatus, setInstallStatus] = useState<'idle' | 'downloading' | 'installing' | 'done' | 'error'>('idle')
  const [installError, setInstallError] = useState<string | null>(null)

  const currentUpdateInfo = updateInfo
  const currentProgress = downloadProgress

  useEffect(() => {
    // Reset state when dialog opens
    if (open) {
      setInstallStatus('idle')
      setInstallError(null)
    }
  }, [open])

  const handleUpdate = async () => {
    try {
      setInstallStatus('downloading')
      setInstallError(null)

      await downloadAndInstall()

      setInstallStatus('done')
    } catch (error) {
      setInstallStatus('error')
      setInstallError(error instanceof Error ? error.message : String(error))
    }
  }

  const handleRelaunch = async () => {
    await relaunchApp()
  }

  const getProgressPercent = () => {
    if (!currentProgress) return 0
    return Math.min(100, Math.max(0, currentProgress.progress))
  }

  const formatBytes = (bytes: number) => {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return Math.round((bytes / Math.pow(k, i)) * 100) / 100 + ' ' + sizes[i]
  }

  const getStatusMessage = () => {
    switch (installStatus) {
      case 'downloading':
        return t('settings:downloadingUpdate')
      case 'installing':
        return t('settings:installingUpdate')
      case 'done':
        return t('settings:updateReady')
      case 'error':
        return installError || t('settings:updateFailed')
      default:
        return ''
    }
  }

  const canClose = installStatus === 'idle' || installStatus === 'error' || installStatus === 'done'

  const getStatusIcon = () => {
    switch (installStatus) {
      case 'done':
        return <Check className="w-5 h-5" />
      case 'error':
        return <AlertCircle className="w-5 h-5" />
      case 'downloading':
      case 'installing':
        return <Loader2 className="w-5 h-5 animate-spin" />
      default:
        return <Rocket className="w-5 h-5" />
    }
  }

  const getTitle = () => {
    switch (installStatus) {
      case 'downloading':
      case 'installing':
        return t('settings:updating')
      case 'done':
        return t('settings:updateReadyTitle')
      case 'error':
        return t('settings:updateFailedTitle')
      default:
        return t('settings:newVersionAvailable')
    }
  }

  const dialogIcon = (() => {
    switch (installStatus) {
      case 'done':
        return <Check className="h-4 w-4 text-success" strokeWidth={2} />
      case 'error':
        return <AlertCircle className="h-4 w-4 text-error" strokeWidth={1.75} />
      case 'downloading':
      case 'installing':
        return <Loader2 className="h-4 w-4 text-foreground animate-spin" />
      default:
        return <Rocket className="h-4 w-4 text-foreground" strokeWidth={1.75} />
    }
  })()

  const hasReleaseNotes = installStatus === 'idle' && !!currentUpdateInfo?.body

  // Dynamic footer based on install status
  const footerContent = installStatus === 'idle' ? (
    <>
      <Button variant="ghost" size="sm" onClick={onClose} className="text-muted-foreground">
        {t('settings:remindLater')}
      </Button>
      <Button size="sm" onClick={handleUpdate} className="gap-1.5">
        <Download className="w-3.5 h-3.5" />
        {t('settings:updateNow')}
      </Button>
    </>
  ) : installStatus === 'downloading' ? (
    <Button disabled variant="secondary" size="sm">
      <Loader2 className="w-3.5 h-3.5 mr-1.5 animate-spin" />
      {t('settings:downloading')}
    </Button>
  ) : installStatus === 'installing' ? (
    <Button disabled variant="secondary" size="sm">
      <Loader2 className="w-3.5 h-3.5 mr-1.5 animate-spin" />
      {t('settings:installing')}
    </Button>
  ) : installStatus === 'done' ? (
    <Button size="sm" onClick={handleRelaunch} className="gap-1.5">
      <Rocket className="w-3.5 h-3.5" />
      {t('settings:relaunchToComplete')}
    </Button>
  ) : installStatus === 'error' ? (
    <>
      <Button variant="outline" size="sm" onClick={onClose}>
        {t('common:close')}
      </Button>
      <Button size="sm" onClick={handleUpdate} variant="default">
        {t('common:retry')}
      </Button>
    </>
  ) : null

  return (
    <UnifiedFormDialog
      open={open && canClose ? true : open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !canClose) return
        onClose()
      }}
      title={getTitle()}
      icon={dialogIcon}
      width={hasReleaseNotes ? 'lg' : 'md'}
      preventCloseOnSubmit={false}
      isSubmitting={installStatus === 'downloading' || installStatus === 'installing'}
      footer={footerContent}
    >
      <div className="space-y-4">
        {installStatus === 'idle' && currentUpdateInfo?.version && (
          <div className="flex items-center justify-between gap-2 rounded-xl border border-border bg-muted-20 px-3.5 py-2.5 text-xs">
            <span className="text-muted-foreground">{t('settings:version')}</span>
            <span className="rounded-md border border-border bg-card px-1.5 py-0.5 font-mono tabular-nums font-medium text-foreground">
              v{currentUpdateInfo.version}
            </span>
          </div>
        )}
        {hasReleaseNotes && (
          <div className="max-h-[46vh] overflow-y-auto rounded-xl border border-border bg-card px-4 py-3.5">
            <div className={releaseNotesProse}>
              <Suspense fallback={null}>
                <ReleaseNotes body={currentUpdateInfo.body!} components={releaseNotesComponents} />
              </Suspense>
            </div>
          </div>
        )}

        {/* Progress Bar */}
        {(installStatus === 'downloading' || installStatus === 'installing') && (
          <div className="space-y-2">
            <Progress value={getProgressPercent()} className="h-1.5" />
            {currentProgress && (
              <div className="flex justify-end text-xs tabular-nums text-muted-foreground">
                <span className="whitespace-nowrap">
                  {formatBytes(currentProgress.current)} / {formatBytes(currentProgress.total)} ·{' '}
                  {Math.round(getProgressPercent())}%
                </span>
              </div>
            )}
          </div>
        )}

        {/* Success Message */}
        {installStatus === 'done' && (
          <div className="flex items-center gap-2 rounded-lg bg-success-light px-3 py-2.5">
            <Check className="h-4 w-4 shrink-0 text-success" />
            <p className="text-body text-success">
              {t('settings:updateCompleteMessage')}
            </p>
          </div>
        )}

        {/* Error Message */}
        {installStatus === 'error' && (
          <div className="flex items-start gap-2 rounded-lg bg-error-light px-3 py-2.5">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-error" />
            <p className="whitespace-pre-line break-words text-body text-error">
              {getStatusMessage()}
            </p>
          </div>
        )}
      </div>
    </UnifiedFormDialog>
  )
}

export default UpdateDialog

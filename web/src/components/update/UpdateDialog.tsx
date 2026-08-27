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
import { Badge } from '@/components/ui/badge'
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

// react-markdown overrides for the release-notes panel. Links open in a new
// tab so the GitHub release page opens in the browser instead of navigating
// the desktop webview.
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

  const getStatusColor = () => {
    switch (installStatus) {
      case 'done':
        return 'bg-success-light text-success'
      case 'error':
        return 'bg-error-light text-error'
      case 'downloading':
      case 'installing':
        return 'bg-info-light text-info'
      default:
        return 'bg-info-light text-info'
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

  const getDescription = () => {
    switch (installStatus) {
      case 'idle':
        return t('settings:updateAvailableDesc')
      case 'downloading':
        return t('settings:downloadingUpdateDesc')
      case 'installing':
        return t('settings:installingUpdateDesc')
      case 'done':
        return t('settings:updateReadyDesc')
      case 'error':
        return getStatusMessage()
      default:
        return ''
    }
  }

  // Dynamic icon with colored background
  const dialogIcon = (
    <div className={`flex items-center justify-center w-10 h-10 rounded-full ${getStatusColor()}`}>
      {getStatusIcon()}
    </div>
  )

  // Dynamic footer based on install status
  const footerContent = installStatus === 'idle' ? (
    <>
      <Button variant="ghost" onClick={onClose} className="text-muted-foreground">
        {t('settings:remindLater')}
      </Button>
      <Button onClick={handleUpdate} className="gap-2">
        <Download className="w-4 h-4" />
        {t('settings:updateNow')}
      </Button>
    </>
  ) : installStatus === 'downloading' ? (
    <Button disabled variant="secondary">
      <Loader2 className="w-4 h-4 mr-2 animate-spin" />
      {t('settings:downloading')}
    </Button>
  ) : installStatus === 'installing' ? (
    <Button disabled variant="secondary">
      <Loader2 className="w-4 h-4 mr-2 animate-spin" />
      {t('settings:installing')}
    </Button>
  ) : installStatus === 'done' ? (
    <Button onClick={handleRelaunch} className="gap-2">
      <Rocket className="w-4 h-4" />
      {t('settings:relaunchToComplete')}
    </Button>
  ) : installStatus === 'error' ? (
    <>
      <Button variant="outline" onClick={onClose}>
        {t('common:close')}
      </Button>
      <Button onClick={handleUpdate} variant="default">
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
      description={getDescription()}
      icon={dialogIcon}
      width="sm"
      preventCloseOnSubmit={false}
      isSubmitting={installStatus === 'downloading' || installStatus === 'installing'}
      footer={footerContent}
    >
      <div className="space-y-4">
        {/* Version Badge */}
        {currentUpdateInfo?.version && (
          <div>
            <Badge variant="secondary" className="text-sm">
              v{updateInfo?.version}
            </Badge>
          </div>
        )}

        {/* Release Notes */}
        {currentUpdateInfo?.body && installStatus === 'idle' && (
          <div className="max-h-[40vh] overflow-y-auto rounded-md border p-3 text-sm">
            <div className="prose prose-sm dark:prose-invert max-w-none">
              <Suspense fallback={null}>
                <ReleaseNotes body={updateInfo?.body ?? ''} components={releaseNotesComponents} />
              </Suspense>
            </div>
          </div>
        )}

        {/* Progress Bar */}
        {(installStatus === 'downloading' || installStatus === 'installing') && (
          <div className="space-y-2">
            <Progress value={getProgressPercent()} className="h-2" />
            <div className="flex justify-between text-xs text-muted-foreground">
              <span>{getStatusMessage()}</span>
              {currentProgress && (
                <span>
                  {formatBytes(currentProgress.current)} / {formatBytes(currentProgress.total)} ({Math.round(getProgressPercent())}%)
                </span>
              )}
            </div>
          </div>
        )}

        {/* Success Message */}
        {installStatus === 'done' && (
          <div className="flex items-center gap-2 p-3 rounded-md bg-success-light border border-success-light dark:border-success-light">
            <Check className="w-5 h-5 text-success" />
            <p className="text-sm text-success">
              {t('settings:updateCompleteMessage')}
            </p>
          </div>
        )}

        {/* Error Message */}
        {installStatus === 'error' && (
          <div className="flex items-start gap-2 p-3 rounded-md bg-error-light border border-error">
            <AlertCircle className="w-5 h-5 text-error mt-0.5" />
            <p className="text-sm text-error">
              {getStatusMessage()}
            </p>
          </div>
        )}
      </div>
    </UnifiedFormDialog>
  )
}

export default UpdateDialog

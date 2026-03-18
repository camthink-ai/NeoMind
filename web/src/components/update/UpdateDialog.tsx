/**
 * UpdateDialog Component
 *
 * Dialog for displaying available updates and managing
 * the download and installation process.
 */

import { useState, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { Badge } from '@/components/ui/badge'
import { Check, Download, AlertCircle, Loader2, Rocket } from 'lucide-react'
import { useUpdateCheck } from '@/hooks/useUpdateCheck'
import { useAppStore } from '@/store'

export interface UpdateDialogProps {
  /** Whether the dialog is open */
  open: boolean
  /** Called when the dialog is closed */
  onClose: () => void
}

export function UpdateDialog({ open, onClose }: UpdateDialogProps) {
  const { t } = useTranslation(['common', 'settings'])
  const { updateStatus, updateInfo, downloadProgress } = useAppStore()

  // Stable empty callback to prevent unnecessary re-renders
  const handleUpdateAvailable = useCallback(() => {
    // Dialog is already open, no action needed
  }, [])

  const { downloadAndInstall, relaunchApp } = useUpdateCheck({
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

      setInstallStatus('installing')
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

  return (
    <Dialog open={open} onOpenChange={(openValue) => {
      if (canClose && !openValue) {
        onClose()
      }
    }}>
      <DialogContent className="sm:max-w-md" onPointerDownOutside={(e) => {
        if (!canClose) {
          e.preventDefault()
        }
      }} onEscapeKeyDown={(e) => {
        if (!canClose) {
          e.preventDefault()
        }
      }}>
        <DialogHeader>
          <div className="flex items-center gap-2">
            <div className={`flex items-center justify-center w-10 h-10 rounded-full ${
              installStatus === 'done'
                ? 'bg-green-100 dark:bg-green-900/30 text-green-600 dark:text-green-400'
                : installStatus === 'error'
                  ? 'bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400'
                  : installStatus === 'downloading' || installStatus === 'installing'
                    ? 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
                    : 'bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400'
            }`}>
              {installStatus === 'done' ? (
                <Check className="w-5 h-5" />
              ) : installStatus === 'error' ? (
                <AlertCircle className="w-5 h-5" />
              ) : installStatus === 'downloading' || installStatus === 'installing' ? (
                <Loader2 className="w-5 h-5 animate-spin" />
              ) : (
                <Rocket className="w-5 h-5" />
              )}
            </div>
            <div className="flex-1">
              <DialogTitle className="text-lg">
                {installStatus === 'downloading' || installStatus === 'installing'
                  ? t('settings:updating')
                  : installStatus === 'done'
                    ? t('settings:updateReadyTitle')
                    : installStatus === 'error'
                      ? t('settings:updateFailedTitle')
                      : t('settings:newVersionAvailable')}
              </DialogTitle>
            </div>
            {currentUpdateInfo?.version && (
              <Badge variant="secondary" className="text-sm">
                v{updateInfo?.version}
              </Badge>
            )}
          </div>
          <DialogDescription className="pt-2">
            {installStatus === 'idle' && t('settings:updateAvailableDesc')}
            {installStatus === 'downloading' && t('settings:downloadingUpdateDesc')}
            {installStatus === 'installing' && t('settings:installingUpdateDesc')}
            {installStatus === 'done' && t('settings:updateReadyDesc')}
            {installStatus === 'error' && getStatusMessage()}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Release Notes */}
          {currentUpdateInfo?.body && installStatus === 'idle' && (
            <div className="max-h-60 overflow-y-auto rounded-md border p-3 text-sm">
              <div className="prose prose-sm dark:prose-invert max-w-none">
                {updateInfo?.body}
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
            <div className="flex items-center gap-2 p-3 rounded-md bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800">
              <Check className="w-5 h-5 text-green-600 dark:text-green-400" />
              <p className="text-sm text-green-800 dark:text-green-200">
                {t('settings:updateCompleteMessage')}
              </p>
            </div>
          )}

          {/* Error Message */}
          {installStatus === 'error' && (
            <div className="flex items-start gap-2 p-3 rounded-md bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800">
              <AlertCircle className="w-5 h-5 text-red-600 dark:text-red-400 mt-0.5" />
              <p className="text-sm text-red-800 dark:text-red-200">
                {getStatusMessage()}
              </p>
            </div>
          )}
        </div>

        <DialogFooter className="gap-2">
          {installStatus === 'idle' && (
            <>
              <Button variant="outline" onClick={onClose}>
                {t('common:cancel')}
              </Button>
              <Button onClick={handleUpdate} className="gap-2">
                <Download className="w-4 h-4" />
                {t('settings:updateNow')}
              </Button>
            </>
          )}

          {installStatus === 'downloading' && (
            <Button disabled variant="secondary">
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              {t('settings:downloading')}
            </Button>
          )}

          {installStatus === 'installing' && (
            <Button disabled variant="secondary">
              <Loader2 className="w-4 h-4 mr-2 animate-spin" />
              {t('settings:installing')}
            </Button>
          )}

          {installStatus === 'done' && (
            <Button onClick={handleRelaunch} className="gap-2">
              <Rocket className="w-4 h-4" />
              {t('settings:relaunchToComplete')}
            </Button>
          )}

          {installStatus === 'error' && (
            <>
              <Button variant="outline" onClick={onClose}>
                {t('common:close')}
              </Button>
              <Button onClick={handleUpdate} variant="default">
                {t('common:retry')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default UpdateDialog

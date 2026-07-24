import { useTranslation } from 'react-i18next'
import { AlertTriangle, RotateCcw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { BrandLogoHorizontal } from '@/components/shared/BrandName'

interface BackendUnavailableOverlayProps {
  /** True when the backend reported a port-in-use error (Tauri path). */
  portConflict?: boolean
  /** Raw backend error string, shown only in the non-port-conflict branch. */
  error?: string
  /** Manual reconnect / retry callback. */
  onRetry?: () => void
}

/**
 * Full-screen overlay shown when the embedded backend (:9375) failed to start
 * (e.g. port already in use by another NeoMind instance) or when the frontend
 * WebSocket could never reach it. Replaces the old behavior where a silent
 * `start_server()` failure left the user staring at an endless "Reconnecting"
 * with no clue why.
 *
 * Two trigger sources (wired in App.tsx):
 *  - Tauri `backend-start-failed` event (port_conflict detected on the Rust side)
 *  - WebSocket `ConnectionState.status === 'error' && !wasConnected`
 *    (fallback for the web/non-Tauri build, or a missed event)
 */
export function BackendUnavailableOverlay({
  portConflict,
  error,
  onRetry,
}: BackendUnavailableOverlayProps) {
  const { t } = useTranslation('common')

  return (
    <div className="fixed inset-0 z-[200] flex flex-col items-center justify-center bg-background p-6">
      <div className="flex flex-col items-center gap-4 max-w-md text-center">
        <BrandLogoHorizontal className="h-10 mb-2" />

        <div className="w-14 h-14 rounded-full bg-error-light flex items-center justify-center">
          <AlertTriangle className="w-7 h-7 text-error" />
        </div>

        <h2 className="text-xl font-semibold text-foreground">
          {t('backendUnavailable.title')}
        </h2>

        <p className="text-sm text-muted-foreground leading-relaxed">
          {portConflict
            ? t('backendUnavailable.portConflict')
            : t('backendUnavailable.message')}
        </p>

        {!portConflict && error && (
          <p className="text-xs text-muted-foreground/70 break-all font-mono max-w-sm">
            {error}
          </p>
        )}

        {onRetry && (
          <Button onClick={onRetry} className="mt-2 gap-2">
            <RotateCcw className="w-4 h-4" />
            {t('backendUnavailable.retry')}
          </Button>
        )}
      </div>
    </div>
  )
}

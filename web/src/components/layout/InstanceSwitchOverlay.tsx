/**
 * InstanceSwitchOverlay - Full-screen overlay during instance switching
 *
 * Reuses StartupLoading visual style.  Auto-dismisses once the app boots.
 */

import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { Button } from '@/components/ui/button'
import { BrandLogoHorizontal } from '@/components/shared/BrandName'
import { AlertTriangle } from 'lucide-react'
import { clearPendingSwitch } from '@/store/slices/instanceSlice'

export function InstanceSwitchOverlay() {
  const { t } = useTranslation(['instances', 'common'])
  const switchingState = useStore((s) => s.switchingState)
  const switchingError = useStore((s) => s.switchingError)
  const instances = useStore((s) => s.instances)
  const currentInstanceId = useStore((s) => s.currentInstanceId)
  const clearSwitchingError = useStore((s) => s.clearSwitchingError)

  const targetInstance = instances.find((i) => i.id === currentInstanceId)

  // Auto-dismiss the overlay once the app has booted successfully
  useEffect(() => {
    if (switchingState === 'switching') {
      const timer = setTimeout(() => {
        clearPendingSwitch()
        useStore.setState({ switchingState: 'idle', previousInstanceId: null })
      }, 1500)
      return () => clearTimeout(timer)
    }
  }, [switchingState])

  if (switchingState === 'idle' || switchingState === 'success') return null

  return (
    <div className="fixed inset-0 z-[300] min-h-screen flex flex-col items-center justify-center bg-background overflow-hidden">
      {/* Animated background — matches StartupLoading */}
      <div className="fixed inset-0">
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted" />
        <div className="absolute inset-0 dot-grid" />
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-muted rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
      </div>

      {/* Main content */}
      <div className="relative z-10 flex flex-col items-center gap-6">
        <BrandLogoHorizontal className="h-12" />

        {switchingState === 'switching' && (
          <>
            <div className="flex items-center gap-3">
              <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '0ms' }} />
              <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '150ms' }} />
              <div className="w-2 h-2 rounded-full bg-primary animate-bounce" style={{ animationDelay: '300ms' }} />
            </div>
            <p className="text-sm text-muted-foreground">
              {t('switch.connecting', { name: targetInstance?.name || '...' })}
            </p>
          </>
        )}

        {switchingState === 'error' && (
          <div className="bg-surface border border-glass-border rounded-xl shadow-xl p-8 max-w-sm w-full mx-4 text-center">
            <AlertTriangle className="h-10 w-10 mx-auto mb-4 text-warning" />
            <p className="text-sm font-medium mb-2">{t('switch.error')}</p>
            {switchingError && (
              <p className="text-xs text-muted-foreground mb-4">
                {switchingError === 'apiKeyRejected'
                  ? t('switch.apiKeyRejected')
                  : switchingError === 'unreachable'
                    ? t('switch.unreachable')
                    : switchingError}
              </p>
            )}
            <div className="flex justify-center">
              <Button variant="default" size="sm" onClick={clearSwitchingError}>
                {t('common:cancel', { defaultValue: 'Cancel' })}
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

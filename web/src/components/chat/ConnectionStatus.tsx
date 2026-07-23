// ConnectionStatus component - displays WebSocket connection state
import { cn } from '@/lib/utils'
import type { ConnectionState } from '@/lib/websocket'
import { useTranslation } from 'react-i18next'
import { RotateCcw, Check, RefreshCw, AlertCircle, XCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface ConnectionStatusProps {
  state: ConnectionState
  className?: string
  onManualReconnect?: () => void
}

export function ConnectionStatus({ state, className, onManualReconnect }: ConnectionStatusProps) {
  const { t } = useTranslation('chat')

  const getStatusInfo = (connectionState: ConnectionState) => {
    switch (connectionState.status) {
      case 'connected':
        return {
          icon: <Check className="w-4 h-4" />,
          text: t('connection.connected'),
          bgClass: 'bg-success-light text-success border-success-light dark:bg-success-light dark:text-success dark:border-success-light'
        }
      case 'reconnecting':
        return {
          icon: <RefreshCw className="w-4 h-4 animate-spin" />,
          text: t('connection.reconnecting'),
          bgClass: 'bg-warning-light text-warning border-warning'
        }
      case 'error':
        return {
          icon: <AlertCircle className="w-4 h-4" />,
          text: connectionState.errorMessage || t('connection.error'),
          bgClass: 'bg-error-light text-error border-error'
        }
      case 'disconnected':
        return {
          icon: <XCircle className="w-4 h-4" />,
          text: t('connection.disconnected'),
          bgClass: 'bg-muted text-foreground border-border'
        }
    }
  }

  const info = getStatusInfo(state)

  return (
    <div className={cn(
      "connection-status flex items-center gap-2 px-3 py-2 rounded-lg text-sm border transition-colors",
      info.bgClass,
      className
    )}>
      {info.icon}
      <span>{info.text}</span>

      {/* Show manual reconnect button when max retries reached */}
      {state.status === 'error' && onManualReconnect && (
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-2 text-xs gap-1 hover:bg-inherit"
          onClick={onManualReconnect}
        >
          <RotateCcw className="w-4 h-4" />
          {t('connection.reconnect')}
        </Button>
      )}
    </div>
  )
}

/**
 * InstanceSelector - Pill badge showing current instance name + status
 *
 * Click opens the full-screen InstanceManagerDialog for switching + managing.
 */

import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { Server } from 'lucide-react'

interface InstanceSelectorProps {
  onManageInstances: () => void
  /** Icon-only square for the sidebar rail (no name/status text) */
  compact?: boolean
}

export function InstanceSelector({ onManageInstances, compact = false }: InstanceSelectorProps) {
  const { t } = useTranslation('instances')
  const instances = useStore((s) => s.instances)
  const currentInstanceId = useStore((s) => s.currentInstanceId)
  const switchingState = useStore((s) => s.switchingState)
  const fetchInstances = useStore((s) => s.fetchInstances)
  const isConnected = useStore((s) => s.wsConnected)

  useEffect(() => {
    fetchInstances()
  }, [fetchInstances])

  const currentInstance = instances.find((i) => i.id === currentInstanceId)
  const isSwitching = switchingState === 'switching'
  // Liveness = the WebSocket to the current backend. last_status is a legacy
  // field (defaults to "unknown", no health loop refreshes it) — gating on it
  // made the local instance permanently red while the dialog showed it green.
  const isOnline = isConnected

  return (
    <button
      disabled={isSwitching}
      onClick={onManageInstances}
      className={cn(
        "rounded-lg text-sm font-medium transition-colors cursor-pointer hover:opacity-80 disabled:opacity-50",
        compact
          ? "flex items-center justify-center h-10 w-10"
          // Expanded: full-width row matching the other sidebar footer rows
          // (w-full h-10 px-3 text-sm) so the rail doesn't reflow per
          // instance-name length; name truncates, status shows as a dot.
          : "w-full flex items-center px-3 h-10",
        isOnline
          ? cn("bg-success-light text-success", !compact && "border border-success-light")
          : "text-error bg-muted"
      )}
    >
      <Server className="h-5 w-5 shrink-0" />
      {!compact && (
        <>
          <span className="ml-3 flex-1 min-w-0 truncate text-left">
            {currentInstance?.name || t('local')}
          </span>
          <span
            className={cn(
              'h-2 w-2 rounded-full shrink-0',
              isOnline ? 'bg-success' : 'bg-error'
            )}
            aria-label={isOnline ? t('status.online') : t('status.offline')}
          />
        </>
      )}
    </button>
  )
}

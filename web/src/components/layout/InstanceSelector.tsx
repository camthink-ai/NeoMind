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
  // Remote instances: rely solely on wsConnected (last_status is from cache, may be stale)
  // Local instance: check both wsConnected and last_status
  const isOnline = isConnected && (!currentInstance || currentInstance.is_local ? (currentInstance?.last_status === 'online' || !currentInstance) : true)

  return (
    <button
      disabled={isSwitching}
      onClick={onManageInstances}
      className={cn(
        "rounded-lg text-xs font-medium transition-colors cursor-pointer hover:opacity-80 disabled:opacity-50",
        compact
          ? "flex items-center justify-center h-10 w-10"
          : "flex items-center gap-1.5 px-2.5 h-10",
        isOnline
          ? cn("bg-success-light text-success", !compact && "border border-success-light")
          : "text-error bg-muted"
      )}
    >
      <Server className="h-4 w-4 shrink-0" />
      {!compact && (
        <>
          <span className="hidden sm:inline max-w-[120px] truncate">
            {currentInstance?.name || t('local')}
          </span>
          <span className="sm:hidden">
            {isOnline ? t('status.online') : t('status.offline')}
          </span>
        </>
      )}
    </button>
  )
}

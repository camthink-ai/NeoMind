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
}

export function InstanceSelector({ onManageInstances }: InstanceSelectorProps) {
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
        // Compact pill for the 48px TopBar — py-1 keeps ~32px so the bar
        // breathes around it instead of the pill crowding the top edge
        "flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium transition-colors",
        "cursor-pointer hover:opacity-80 disabled:opacity-50",
        isOnline
          ? "bg-success-light text-success border border-success-light"
          : "text-error bg-muted"
      )}
    >
      <Server className="h-4 w-4 shrink-0" />
      <span className="hidden sm:inline max-w-[120px] truncate">
        {currentInstance?.name || t('local')}
      </span>
      <span className="sm:hidden">
        {isOnline ? t('status.online') : t('status.offline')}
      </span>
    </button>
  )
}

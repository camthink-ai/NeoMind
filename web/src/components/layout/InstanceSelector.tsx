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
        "rounded-lg text-sm font-medium transition-colors cursor-pointer disabled:opacity-50",
        // Same muted resting color + hover as every other sidebar utility
        // row/icon (nav items, Setup Guide, collapse toggle) — an unstyled
        // inherit renders the Server icon a step darker than its neighbors.
        "text-muted-foreground hover:text-foreground hover:bg-muted-50",
        compact
          ? "flex items-center justify-center h-10 w-10"
          // Expanded: full-width row matching the other sidebar footer rows
          // (w-full h-10 px-3 text-sm) so the rail doesn't reflow per
          // instance-name length; name truncates, status shows as a dot.
          : "w-full flex items-center px-3 h-10",
        // Neutral surface — status lives on the dot alone. A tinted block
        // here out-shouted the active-page indicator (inverted hierarchy).
      )}
    >
      <div className="relative shrink-0">
        {/* 18px, not the rail's 20px: the Server glyph is the densest ink in
            the rail (full-width stacked slabs vs. airy Rocket/gear) — at the
            shared size it reads a step larger. Optical, not pixel, parity. */}
        <Server className="h-[18px] w-[18px]" />
        {/* Status dot on the icon's top-right corner — same anchor as the
            Setup Guide badge, so both markers align across rows. */}
        <span
          className={cn(
            'absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full',
            isOnline ? 'bg-success' : 'bg-error'
          )}
          aria-label={isOnline ? t('status.online') : t('status.offline')}
        />
      </div>
      {!compact && (
        <span className="ml-3 flex-1 min-w-0 truncate text-left">
          {currentInstance?.name || t('local')}
        </span>
      )}
    </button>
  )
}

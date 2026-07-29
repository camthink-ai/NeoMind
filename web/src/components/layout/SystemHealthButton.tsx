import { useEffect, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Wifi, WifiOff, Cpu, Bell, Activity } from 'lucide-react'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'

/**
 * System health indicator button with a status dot + dropdown mini-panel.
 * Shows backend connection, device online count, and unread alert count.
 */
export function SystemHealthButton() {
  const { t } = useTranslation('common')
  const navigate = useNavigate()

  const wsConnected = useStore((s) => s.wsConnected)
  const devices = useStore((s) => s.devices)
  const fetchDevices = useStore((s) => s.fetchDevices)
  const alerts = useStore((s) => s.alerts)

  // Lightweight device fetch on mount
  useEffect(() => {
    fetchDevices()
  }, [fetchDevices])

  const onlineCount = useMemo(
    () => devices.filter((d) => d.online || d.status === 'online').length,
    [devices]
  )
  const totalCount = devices.length

  const unreadCount = useMemo(
    () => alerts.filter((a) => !a.acknowledged && a.status !== 'resolved' && a.status !== 'acknowledged').length,
    [alerts]
  )

  // Status dot color
  const dotColor = !wsConnected
    ? 'bg-error'
    : unreadCount > 0
      ? 'bg-warning'
      : 'bg-success'

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="relative w-10 h-10 rounded-lg bg-[var(--chrome)]"
          aria-label={t('systemHealth.title')}
        >
          <Activity className="h-4 w-4" />
          <span
            className={cn('absolute top-1.5 right-1.5 w-2 h-2 rounded-full', dotColor)}
          />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-72">
        <div className="px-3 py-2 border-b">
          <span className="font-semibold text-sm">{t('systemHealth.title')}</span>
        </div>

        {/* Backend status */}
        <div className="px-3 py-2 flex items-center gap-2">
          {wsConnected ? (
            <Wifi className="h-4 w-4 text-success" />
          ) : (
            <WifiOff className="h-4 w-4 text-error" />
          )}
          <span className="text-sm">
            {wsConnected ? t('systemHealth.backendConnected') : t('systemHealth.backendDisconnected')}
          </span>
        </div>

        {/* Devices */}
        <DropdownMenuItem
          className="cursor-pointer"
          onClick={() => navigate('/devices')}
        >
          <Cpu className="h-4 w-4 mr-2" />
          <span className="text-sm">
            {t('systemHealth.devicesOnline', { online: onlineCount, total: totalCount })}
          </span>
        </DropdownMenuItem>

        {/* Alerts */}
        <DropdownMenuItem
          className="cursor-pointer"
          onClick={() => navigate('/messages')}
        >
          <Bell className="h-4 w-4 mr-2" />
          <span className="text-sm">
            {t('systemHealth.unreadAlerts', { count: unreadCount })}
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

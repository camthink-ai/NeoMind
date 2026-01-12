import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Radar, Square, RefreshCw, Trash2, RotateCcw } from 'lucide-react'
import { cn } from '@/lib/utils'
import { toast } from '@/components/ui/use-toast'

interface HassDiscoveryDialogProps {
  open: boolean
  onClose: () => void
  hassDiscoveryStatus: any
  hassDiscoveredDevices: any[]
  hassDiscovering: boolean
  onStartDiscovery: () => Promise<void>
  onStopDiscovery: () => Promise<void>
  onRefresh: () => Promise<void>
  onRegisterDevice: (device: any) => Promise<boolean>
  onUnregisterDevice: (deviceId: string) => Promise<boolean>
  onClearDevices: () => void
}

export function HassDiscoveryDialog({
  open,
  onClose,
  hassDiscoveryStatus,
  hassDiscoveredDevices,
  hassDiscovering,
  onStartDiscovery,
  onStopDiscovery,
  onRefresh,
  onRegisterDevice,
  onUnregisterDevice,
  onClearDevices,
}: HassDiscoveryDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [message, setMessage] = useState('')
  const [processingId, setProcessingId] = useState<string | null>(null)

  const handleStart = async () => {
    setMessage('')
    try {
      await onStartDiscovery()
      await onRefresh()
      setMessage(t('devices:hass.discoveryStarted'))
    } catch (e) {
      setMessage(t('devices:hass.startFailed', { error: (e as Error).message }))
    }
  }

  const handleStop = async () => {
    setMessage('')
    try {
      await onStopDiscovery()
      await onRefresh()
      setMessage(t('devices:hass.discoveryStopped'))
    } catch (e) {
      setMessage(t('devices:hass.stopFailed', { error: (e as Error).message }))
    }
  }

  const handleRefresh = async () => {
    setMessage('')
    try {
      await onRefresh()
      setTimeout(() => setMessage(''), 2000)
    } catch (e) {
      setMessage(t('devices:hass.refreshFailed', { error: (e as Error).message }))
    }
  }

  const handleRegister = async (device: any) => {
    setProcessingId(device.device_id)
    setMessage('')
    try {
      const success = await onRegisterDevice(device)
      if (success) {
        toast({
          title: t('devices:hass.registerSuccess'),
          description: t('devices:hass.registered', { name: device.name || device.device_id }),
        })
        await onRefresh()
      }
    } catch (e) {
      toast({
        title: t('devices:hass.registerFailed'),
        description: (e as Error).message,
        variant: "destructive"
      })
    } finally {
      setProcessingId(null)
    }
  }

  const handleUnregister = async (device: any) => {
    if (!confirm(t('devices:hass.unregisterConfirm', { name: device.name || device.device_id }))) return

    setProcessingId(device.device_id)
    setMessage('')
    try {
      const success = await onUnregisterDevice(device.device_id)
      if (success) {
        toast({
          title: t('devices:hass.unregisterSuccess'),
          description: t('devices:hass.unregistered', { name: device.name || device.device_id }),
        })
        await onRefresh()
      }
    } catch (e) {
      toast({
        title: t('devices:hass.unregisterFailed'),
        description: (e as Error).message,
        variant: "destructive"
      })
    } finally {
      setProcessingId(null)
    }
  }

  const handleReregister = async (device: any) => {
    if (!confirm(t('devices:hass.reregisterConfirm', { name: device.name || device.device_id }))) return

    setProcessingId(device.device_id)
    setMessage('')
    try {
      // First unregister if already registered
      if (device.already_registered) {
        await onUnregisterDevice(device.device_id)
      }
      // Then register again
      const success = await onRegisterDevice(device)
      if (success) {
        toast({
          title: t('devices:hass.reregisterSuccess'),
          description: t('devices:hass.reregistered', { name: device.name || device.device_id }),
        })
        await onRefresh()
      }
    } catch (e) {
      toast({
        title: t('devices:hass.reregisterFailed'),
        description: (e as Error).message,
        variant: "destructive"
      })
    } finally {
      setProcessingId(null)
    }
  }

  const handleClear = () => {
    onClearDevices()
    setMessage(t('devices:hass.cleared'))
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-3xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Radar className="h-5 w-5" />
            {t('devices:hass.title')}
          </DialogTitle>
          <DialogDescription>
            {t('devices:hass.description')}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="border rounded-lg p-4">
            <div className="flex items-center justify-between mb-4">
              <h4 className="font-semibold">{t('devices:hass.discoveryStatus')}</h4>
              <Button variant="outline" size="sm" onClick={handleRefresh}>
                <RefreshCw className="h-4 w-4 mr-2" />
                {t('common:refresh')}
              </Button>
            </div>

            <div className="grid grid-cols-3 gap-4 mb-4">
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">{t('devices:hass.runningStatus')}</p>
                <p className="text-sm font-medium">
                  {hassDiscoveryStatus?.hass_discovery?.enabled ? (
                    <span className="text-green-600">{t('devices:hass.running')}</span>
                  ) : (
                    <span className="text-muted-foreground">{t('devices:hass.notStarted')}</span>
                  )}
                </p>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">{t('devices:hass.discoveredDevices')}</p>
                <p className="text-sm font-medium">{t('devices:hass.count', { count: hassDiscoveredDevices.length })}</p>
              </div>
              <div className="space-y-1">
                <p className="text-xs text-muted-foreground">{t('devices:hass.supportedComponents')}</p>
                <p className="text-sm font-medium">{t('devices:hass.countTypes', { count: hassDiscoveryStatus?.component_count || 0 })}</p>
              </div>
            </div>

            <div className="flex gap-2">
              {hassDiscoveryStatus?.hass_discovery?.enabled ? (
                <>
                  <Button variant="outline" onClick={handleClear}>
                    {t('devices:hass.clearDevices')}
                  </Button>
                  <Button variant="destructive" onClick={handleStop} disabled={hassDiscovering}>
                    <Square className="h-4 w-4 mr-2" />
                    {t('devices:hass.stopDiscovery')}
                  </Button>
                </>
              ) : (
                <Button onClick={handleStart} disabled={hassDiscovering}>
                  <Radar className="h-4 w-4 mr-2" />
                  {t('devices:hass.startDiscovery')}
                </Button>
              )}
            </div>

            {message && (
              <div className={cn(
                "mt-3 p-3 rounded-md text-sm",
                message.includes('失败') || message.includes('错误')
                  ? "bg-destructive/10 text-destructive"
                  : "bg-green-50 dark:bg-green-950/20 text-green-700 dark:text-green-400"
              )}>
                {message}
              </div>
            )}
          </div>

          <div className="border rounded-lg p-4">
            <h4 className="font-semibold mb-3">{t('devices:hass.discoveredDevicesList')}</h4>

            {hassDiscoveredDevices.length === 0 ? (
              <div className="text-center py-8 text-muted-foreground">
                <Radar className="h-10 w-10 mx-auto mb-3 opacity-50" />
                <p>{t('devices:hass.noDiscoveredDevices')}</p>
                <p className="text-xs mt-2">
                  {hassDiscoveryStatus?.hass_discovery?.enabled
                    ? t('devices:hass.waitingForConfig')
                    : t('devices:hass.startToDetect')}
                </p>
              </div>
            ) : (
              <div className="grid grid-cols-1 gap-3 max-h-[400px] overflow-y-auto">
                {hassDiscoveredDevices.map((device) => (
                  <div key={device.device_id} className="border rounded-lg p-3 space-y-2">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <h5 className="font-medium">{device.name || device.device_id}</h5>
                        <p className="text-xs text-muted-foreground font-mono">{device.device_id}</p>
                        <p className="text-xs text-muted-foreground mt-1">{device.description}</p>
                      </div>
                      <Badge
                        variant={device.already_registered ? "default" : "outline"}
                        className="text-xs"
                      >
                        {t('devices:hass.entityCount', { count: device.entity_count })}
                      </Badge>
                    </div>

                    {device.entities && device.entities.length > 0 && (
                      <div className="flex flex-wrap gap-1">
                        {device.entities.slice(0, 4).map((entity: any) => (
                          <Badge key={entity.entity_id} variant="secondary" className="text-xs">
                            {entity.name || entity.component}
                          </Badge>
                        ))}
                        {device.entities.length > 4 && (
                          <Badge variant="secondary" className="text-xs">
                            +{device.entities.length - 4}
                          </Badge>
                        )}
                      </div>
                    )}

                    <div className="flex items-center gap-4 text-xs text-muted-foreground">
                      <span>{t('devices:hass.metrics', { count: device.total_metrics })}</span>
                      <span>{t('devices:hass.commands', { count: device.total_commands })}</span>
                    </div>

                    <div className="pt-2 border-t flex items-center justify-between">
                      {device.already_registered ? (
                        <div className="flex items-center gap-2">
                          <Badge variant="default" className="text-xs">{t('devices:hass.registeredBadge')}</Badge>
                          <Button
                            size="sm"
                            variant="outline"
                            className="text-xs h-7"
                            onClick={() => handleReregister(device)}
                            disabled={processingId === device.device_id}
                          >
                            <RotateCcw className="h-3 w-3 mr-1" />
                            {t('devices:hass.reregister')}
                          </Button>
                          <Button
                            size="sm"
                            variant="ghost"
                            className="text-xs h-7 text-destructive hover:text-destructive"
                            onClick={() => handleUnregister(device)}
                            disabled={processingId === device.device_id}
                          >
                            <Trash2 className="h-3 w-3 mr-1" />
                            {t('devices:hass.unregister')}
                          </Button>
                        </div>
                      ) : (
                        <Button
                          size="sm"
                          variant="outline"
                          className="text-xs h-7"
                          onClick={() => handleRegister(device)}
                          disabled={processingId === device.device_id}
                        >
                          {processingId === device.device_id ? t('devices:hass.registering') : t('devices:hass.registerDevice')}
                        </Button>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="p-3 border rounded-lg bg-muted/50 text-sm text-muted-foreground">
            <p>{t('devices:hass.instructions.1')}</p>
            <p>{t('devices:hass.instructions.2')}</p>
            <p>{t('devices:hass.instructions.3')}</p>
            <p>{t('devices:hass.instructions.4')}</p>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>{t('common:close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

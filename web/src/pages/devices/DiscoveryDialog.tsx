import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { RefreshCw, Radar, PlusCircle } from "lucide-react"
import type { DiscoveredDevice, DeviceType } from "@/types"

interface DiscoveryDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  discovering: boolean
  discoveredDevices: DiscoveredDevice[]
  deviceTypes: DeviceType[]
  onDiscover: (host: string) => Promise<void>
  onAddDiscovered: (device: DiscoveredDevice) => void
}

export function DiscoveryDialog({
  open,
  onOpenChange,
  discovering,
  discoveredDevices,
  deviceTypes,
  onDiscover,
  onAddDiscovered,
}: DiscoveryDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [host, setHost] = useState("localhost")
  const [scanned, setScanned] = useState(false)

  const handleDiscover = async () => {
    if (!host.trim()) return
    setScanned(false)
    await onDiscover(host.trim())
    setScanned(true)
  }

  const getDeviceTypeName = (typeId: string | null) => {
    if (!typeId) return t('devices:discoveryDialog.unknown')
    const dt = deviceTypes.find((t) => t.device_type === typeId)
    return dt?.name || typeId
  }

  const getDeviceTypeLabel = (deviceType: string | null) => {
    if (deviceType === "mqtt_gateway") return t('devices:discoveryDialog.types.mqtt')
    if (deviceType === "modbus_controller") return t('devices:discoveryDialog.types.modbus')
    if (deviceType === "http_device") return t('devices:discoveryDialog.types.http')
    if (deviceType === "coap_device") return t('devices:discoveryDialog.types.coap')
    return ""
  }

  const handleAddDevice = (device: DiscoveredDevice) => {
    onAddDiscovered(device)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('devices:discoveryDialog.title')}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-4">
            <div className="flex gap-2">
              <Input
                value={host}
                onChange={(e) => setHost(e.target.value)}
                placeholder={t('devices:discoveryDialog.hostPlaceholder')}
              />
              <Button
                onClick={handleDiscover}
                disabled={!host.trim() || discovering}
                size="sm"
              >
                {discovering ? (
                  <>
                    <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                    {t('devices:discoveryDialog.scanning')}
                  </>
                ) : (
                  <>
                    <Radar className="mr-2 h-4 w-4" />
                    {t('devices:discoveryDialog.scan')}
                  </>
                )}
              </Button>
            </div>

            {scanned && discoveredDevices.length === 0 && (
              <p className="text-sm text-muted-foreground text-center py-8">
                {t('devices:discoveryDialog.noDevices')}
              </p>
            )}

            {discoveredDevices.length > 0 && (
              <ScrollArea className="h-64">
                <div className="space-y-2">
                  {discoveredDevices.map((device) => (
                    <div
                      key={device.id}
                      className="flex items-center justify-between rounded-md border p-3"
                    >
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-mono text-sm">{device.host}:{device.port}</span>
                          <Badge variant="outline" className="text-xs">
                            {getDeviceTypeName(device.device_type)}
                          </Badge>
                        </div>
                        <p className="text-xs text-muted-foreground mt-1">
                          {getDeviceTypeLabel(device.device_type)}
                        </p>
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleAddDevice(device)}
                      >
                        <PlusCircle className="mr-2 h-4 w-4" />
                        {t('devices:discoveryDialog.add')}
                      </Button>
                    </div>
                  ))}
                </div>
              </ScrollArea>
            )}

            {!scanned && (
              <div className="py-8 text-center text-sm text-muted-foreground">
                {t('devices:discoveryDialog.instruction')}
              </div>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

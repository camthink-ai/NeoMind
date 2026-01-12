import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { toast } from "@/components/ui/use-toast"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { RefreshCw } from "lucide-react"
import type { DeviceType } from "@/types"

// Generate 10-character random string (lowercase alphanumeric)
function generateRandomId(): string {
  const chars = 'abcdefghijklmnopqrstuvwxyz0123456789'
  let result = ''
  for (let i = 0; i < 10; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length))
  }
  return result
}

interface AddDeviceDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceTypes: DeviceType[]
  onAdd: (deviceType: string, deviceId?: string, deviceName?: string) => Promise<boolean>
  adding: boolean
}

export function AddDeviceDialog({
  open,
  onOpenChange,
  deviceTypes,
  onAdd,
  adding,
}: AddDeviceDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [selectedDeviceType, setSelectedDeviceType] = useState("")
  const [deviceId, setDeviceId] = useState("")
  const [deviceName, setDeviceName] = useState("")

  // Generate random ID when dialog opens
  useEffect(() => {
    if (open && !deviceId) {
      setDeviceId(generateRandomId())
    }
  }, [open])

  const handleAdd = async () => {
    if (!selectedDeviceType) return

    const success = await onAdd(selectedDeviceType, deviceId || undefined, deviceName || undefined)
    if (success) {
      setSelectedDeviceType("")
      setDeviceId("")
      setDeviceName("")
      onOpenChange(false)
      toast({
        title: t('devices:add.success'),
        description: deviceId ? t('devices:add.successWithId', { deviceId }) : t('devices:add.successGeneric'),
      })
    } else {
      toast({
        title: t('devices:add.error'),
        description: t('devices:add.retryMessage'),
        variant: "destructive",
      })
    }
  }

  // Reset form when dialog opens
  const handleOpenChange = (open: boolean) => {
    if (open) {
      // Generate new random ID when opening
      setDeviceId(generateRandomId())
    } else {
      setSelectedDeviceType("")
      setDeviceId("")
      setDeviceName("")
    }
    onOpenChange(open)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="max-w-md max-h-[85vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('devices:add.title')}</DialogTitle>
        </DialogHeader>
        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <Label htmlFor="device-type" dangerouslySetInnerHTML={{ __html: t('devices:add.typeRequired') }} />
            <Select value={selectedDeviceType} onValueChange={setSelectedDeviceType}>
              <SelectTrigger id="device-type">
                <SelectValue placeholder={t('devices:add.typePlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {deviceTypes.map((type) => (
                  <SelectItem key={type.device_type} value={type.device_type}>
                    {type.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {!selectedDeviceType && (
              <p className="text-xs text-destructive">{t('devices:add.typeValidation')}</p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="device-id">{t('devices:add.id')}</Label>
            <div className="flex gap-2">
              <Input
                id="device-id"
                value={deviceId}
                onChange={(e) => setDeviceId(e.target.value)}
                placeholder={t('devices:id.autoGenerate')}
                className="font-mono"
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => setDeviceId(generateRandomId())}
                title={t('devices:id.regenerate')}
              >
                <RefreshCw className="h-4 w-4" />
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">{t('devices:id.topicHint', { type: selectedDeviceType || '{type}', id: deviceId || '{id}' })}</p>
          </div>
          <div className="space-y-2">
            <Label htmlFor="device-name">{t('devices:deviceName')}</Label>
            <Input
              id="device-name"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              placeholder={t('common:optional')}
            />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={handleAdd} disabled={!selectedDeviceType || adding} size="sm">
            {adding ? t('devices:adding') : t('common:add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

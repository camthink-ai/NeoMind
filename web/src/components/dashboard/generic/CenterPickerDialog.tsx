/**
 * Center Picker Dialog
 *
 * A dialog for visually selecting the map center point by clicking on the map.
 * Provides an intuitive way to set map center coordinates.
 */

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Check, MapPin, Crosshair } from 'lucide-react'
import { MapDisplay } from './MapDisplay'
import { cn } from '@/lib/utils'

interface CenterPickerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  center: { lat: number; lng: number }
  zoom: number
  tileLayer: string
  onSave: (center: { lat: number; lng: number }) => void
}

export function CenterPickerDialog({
  open,
  onOpenChange,
  center: initialCenter,
  zoom,
  tileLayer,
  onSave,
}: CenterPickerDialogProps) {
  const { t } = useTranslation('dashboardComponents')
  const [selectedCenter, setSelectedCenter] = useState(initialCenter)
  const [hasSelected, setHasSelected] = useState(false)

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setSelectedCenter(initialCenter)
      setHasSelected(false)
    }
  }, [open, initialCenter])

  // Handle map click to set new center
  const handleMapClick = useCallback((lat: number, lng: number) => {
    setSelectedCenter({ lat, lng })
    setHasSelected(true)
  }, [])

  // Handle save
  const handleSave = useCallback(() => {
    onSave(selectedCenter)
    onOpenChange(false)
  }, [selectedCenter, onSave, onOpenChange])

  // Create a marker to show the selected position
  const previewMarker = {
    id: 'center-preview',
    latitude: selectedCenter.lat,
    longitude: selectedCenter.lng,
    label: t('mapDisplay.selectedCenter', '选中的中心点'),
    markerType: 'marker' as const,
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl h-[70vh] p-0 gap-0 flex flex-col">
        <DialogHeader className="px-6 py-4 border-b">
          <DialogTitle className="text-lg flex items-center gap-2">
            <Crosshair className="h-5 w-5" />
            {t('mapDisplay.selectCenter', '选择地图中心点')}
          </DialogTitle>
        </DialogHeader>

        {/* Instructions */}
        <div className="px-6 py-3 bg-muted/30 border-b text-sm text-muted-foreground">
          <div className="flex items-center gap-2">
            <MapPin className="h-4 w-4 text-primary" />
            <span>{t('mapDisplay.clickToSelectCenter', '点击地图选择中心点位置')}</span>
          </div>
        </div>

        {/* Map Preview */}
        <div className="flex-1 relative bg-muted/30">
          <div className="absolute inset-0">
            <MapDisplay
              center={selectedCenter}
              zoom={zoom}
              tileLayer={tileLayer}
              markers={[previewMarker]}
              showControls={true}
              showFullscreen={false}
              interactive={true}
              onMapClick={handleMapClick}
              className="w-full h-full"
            />
          </div>

          {/* Crosshair overlay at center */}
          <div className="absolute inset-0 pointer-events-none flex items-center justify-center">
            <div className={cn(
              "w-8 h-8 relative transition-all duration-200",
              hasSelected && "scale-110"
            )}>
              {/* Horizontal line */}
              <div className="absolute top-1/2 left-0 right-0 h-0.5 bg-primary/50 -translate-y-1/2" />
              {/* Vertical line */}
              <div className="absolute left-1/2 top-0 bottom-0 w-0.5 bg-primary/50 -translate-x-1/2" />
            </div>
          </div>
        </div>

        {/* Selected coordinates display */}
        <div className="px-6 py-3 border-t bg-muted/20">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className="flex items-center gap-2">
                <span className="text-sm text-muted-foreground">{t('visualDashboard.latitude')}:</span>
                <span className="font-mono text-sm">{selectedCenter.lat.toFixed(6)}</span>
              </div>
              <div className="flex items-center gap-2">
                <span className="text-sm text-muted-foreground">{t('visualDashboard.longitude')}:</span>
                <span className="font-mono text-sm">{selectedCenter.lng.toFixed(6)}</span>
              </div>
            </div>
            {hasSelected && (
              <div className="text-sm text-green-600 dark:text-green-400">
                ✓ {t('mapDisplay.centerSelected', '已选择新位置')}
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="px-6 py-4 border-t bg-muted/20">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button onClick={handleSave} disabled={!hasSelected}>
            <Check className="h-4 w-4 mr-1" />
            {t('common.confirm')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
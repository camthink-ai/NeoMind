/**
 * Map Display Component
 *
 * Interactive map component for visualizing devices, metrics, and commands.
 * Supports device markers, metric overlays, and command interactions.
 */

import { getPortalRoot } from '@/lib/portal'
import { useState, useCallback, useRef, useEffect, memo, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { createPortal } from 'react-dom'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { normalizeImageUrl } from '@/lib/imageUtils'
import { findDevice } from '@/lib/deviceUtils'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { useDataSource } from '@/hooks/useDataSource'
import { toast } from '@/components/ui/use-toast'
import {
  MapPin,
  Navigation,
  ZoomIn,
  ZoomOut,
  Map as MapIcon,
  Maximize2,
  Minimize2,
  X,
  Activity,
  Zap,
  Monitor,
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'
import type { MapBinding } from './MapEditorDialog'
import { useStore } from '@/store'
import { shallow } from 'zustand/shallow'

// Get sendCommand from store
const useSendCommand = () => useStore(state => state.sendCommand)

// ============================================================================
// Types
// ============================================================================

export interface MapMarker {
  id: string
  /** @deprecated Use sourceId instead */
  deviceId?: string
  sourceId?: string
  latitude: number
  longitude: number
  label?: string
  value?: number
  status?: 'online' | 'offline' | 'error' | 'warning'
  onClick?: () => void
  metricValue?: string
  command?: string
  // Names for tooltip display
  deviceName?: string
  metricName?: string
  commandName?: string
  // Marker type for styling
  markerType?: 'device' | 'metric' | 'command' | 'marker'
}

export interface MapDisplayProps {
  dataSource?: DataSource
  markers?: MapMarker[]
  bindings?: MapBinding[]  // Bindings from config with type and position info

  // Map options
  center?: { lat: number; lng: number }
  zoom?: number
  minZoom?: number
  maxZoom?: number

  // Display options
  size?: 'sm' | 'md' | 'lg'
  showControls?: boolean
  showFullscreen?: boolean
  interactive?: boolean

  // Styling
  tileLayer?: string
  markerColor?: string
  className?: string

  // Editor mode - callback for map click
  onMapClick?: (lat: number, lng: number) => void

  // Device binding options
  deviceBinding?: {
    latField?: string      // Field name for latitude (default: 'lat', 'latitude')
    lngField?: string      // Field name for longitude (default: 'lng', 'lon', 'longitude')
    labelField?: string    // Field name for marker label (default: 'name', 'id')
    valueField?: string    // Field name to show as metric value
    statusField?: string   // Field name for online status (default: 'status', 'online')
  }
}

// Default tile layers
const TILE_LAYERS: Record<string, string> = {
  osm: 'https://tile.openstreetmap.org/{z}/{x}/{y}.png',
  satellite: 'https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}',
  dark: 'https://basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png',
  terrain: 'https://tile.opentopomap.org/{z}/{x}/{y}.png',
}

// ============================================================================
// Marker Component
// ============================================================================

interface MapMarkerDotProps {
  marker: MapMarker
  onClick: () => void
  isSelected?: boolean
  t: (key: string) => string
  onImageClick?: (info: { src: string; deviceName?: string; metricName?: string }) => void
}

const MapMarkerDot = memo(function MapMarkerDot({ marker, onClick, isSelected = false, t, onImageClick }: MapMarkerDotProps) {
  // Marker type config for colors and icons — memoized
  const config = useMemo(() => {
    const type = marker.markerType || 'device'
    switch (type) {
      case 'device':
        return {
          bgColor: 'bg-success',
          pingColor: 'bg-success',
          icon: MapPin,
        }
      case 'metric':
        return {
          bgColor: 'bg-accent-purple',
          pingColor: 'bg-accent-purple',
          icon: Activity,
        }
      case 'command':
        return {
          bgColor: 'bg-info',
          pingColor: 'bg-info',
          icon: Zap,
        }
      case 'marker':
        return {
          bgColor: 'bg-accent-orange',
          pingColor: 'bg-accent-orange',
          icon: Monitor,
        }
      default:
        return {
          bgColor: 'bg-success',
          pingColor: 'bg-success',
          icon: MapPin,
        }
    }
  }, [marker.markerType])

  // Status indicator color helper
  const getStatusColor = (status?: string) => {
    switch (status) {
      case 'online': return 'bg-success'
      case 'offline': return 'bg-muted-foreground'
      case 'error': return 'bg-error'
      case 'warning': return 'bg-warning'
      default: return 'bg-muted-foreground'
    }
  }

  const statusColor = getStatusColor(marker.status)
  const finalBgColor = statusColor || config.bgColor
  const Icon = config.icon

  return (
    <>
      <button
        onClick={(e) => {
          e.stopPropagation()
          onClick()
        }}
        className={cn(
          "absolute transform -translate-x-1/2 -translate-y-1/2 group",
          isSelected && "z-20"
        )}
        style={{ left: '50%', top: '50%' }}
      >
        {/* Pulsing effect for online devices or active markers */}
        {marker.status === 'online' && (
          <span className={cn(
            "absolute inline-flex h-full w-full animate-ping opacity-75 rounded-full",
            config.pingColor
          )} />
        )}

        {/* Marker with icon */}
        <span
          className={cn(
            'relative inline-flex rounded-full items-center justify-center',
            finalBgColor,
            'h-6 w-6 border-2 border-background shadow-sm',
            'group-hover:scale-125 transition-transform',
            isSelected && 'ring-2 ring-primary-foreground ring-offset-2'
          )}
        >
          <Icon className="h-4 w-4 text-primary-foreground" strokeWidth={2.5} />
        </span>
      </button>

      {/* Details Popup — same style as CustomLayer */}
      {isSelected && (() => {
        const _isImg = !!normalizeImageUrl(marker.markerType === 'metric' ? marker.metricValue : null)
        return (
        <div
          className={cn(
            'absolute z-50 rounded-lg shadow-xl animate-in fade-in zoom-in-95 duration-150',
            _isImg
              ? 'min-w-[160px] max-w-[280px] overflow-hidden'
              : 'min-w-[200px] max-w-[280px] border bg-bg-95 backdrop-blur p-3',
          )}
          style={{
            left: '50%',
            top: '100%',
            transform: 'translate(-50%, 8px)',
          }}
          onClick={(e) => e.stopPropagation()}
        >
          {!_isImg && (<>
          {/* Close button */}
          <button
            className="absolute top-1.5 right-1.5 p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground flex items-center justify-center"
            onClick={(e) => { e.stopPropagation(); onClick() }}
          >
            <X className="h-4 w-4" />
          </button>

          {/* Header with icon and type */}
          <div className="flex items-center gap-2 mb-2 pb-2 border-b">
            <div className={cn('p-1.5 rounded-md', config.bgColor)}>
              <Icon className={cn('h-4 w-4 text-primary-foreground')} />
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium text-muted-foreground uppercase">
                {marker.markerType === 'device' && t('mapDisplay.device')}
                {marker.markerType === 'metric' && t('mapDisplay.metric')}
                {marker.markerType === 'command' && t('mapDisplay.command')}
                {marker.markerType === 'marker' && t('mapDisplay.marker')}
              </p>
              <p className="text-sm font-semibold truncate">{marker.label || marker.deviceName || marker.id}</p>
            </div>
          </div>
          </>)}

          {/* Content based on type */}
          <div className="space-y-1.5 text-sm">
            {marker.markerType === 'device' && (
              <>
                {marker.sourceId && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.device')} ID:</span>
                    <span className="font-mono text-xs">{marker.sourceId}</span>
                  </div>
                )}
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('mapDisplay.status')}:</span>
                  <span className={cn(
                    'flex items-center gap-1.5',
                    marker.status === 'online' && 'text-success',
                    marker.status === 'offline' && 'text-muted-foreground'
                  )}>
                    <span className={cn('w-2 h-2 rounded-full', getStatusColor(marker.status))} />
                    {marker.status === 'online' ? t('mapDisplay.online') : marker.status === 'offline' ? t('mapDisplay.offline') : (marker.status || '-')}
                  </span>
                </div>
                {marker.deviceName && marker.deviceName !== marker.label && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.device')}:</span>
                    <span className="text-xs">{marker.deviceName}</span>
                  </div>
                )}
              </>
            )}

            {marker.markerType === 'metric' && (() => {
              const img = normalizeImageUrl(marker.metricValue)
              if (img) {
                return (
                  <div className="relative">
                    <img
                      src={img.src}
                      alt={marker.metricName || 'metric'}
                      className="w-full max-h-[120px] object-cover cursor-pointer hover:opacity-80 transition-opacity"
                      onClick={(e) => {
                        e.stopPropagation()
                        onImageClick?.({ src: img.src, deviceName: marker.deviceName, metricName: marker.metricName })
                      }}
                    />
                    {marker.deviceName && (
                      <span className="absolute bottom-0 inset-x-0 px-1.5 py-0.5 text-[10px] text-white bg-overlay-medium truncate">
                        {marker.deviceName}
                      </span>
                    )}
                  </div>
                )
              }
              return (
              <>
                {marker.deviceName && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.device')}:</span>
                    <span className="text-xs">{marker.deviceName}</span>
                  </div>
                )}
                {marker.metricName && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.metric')}:</span>
                    <span className="text-xs font-mono">{marker.metricName}</span>
                  </div>
                )}
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('mapDisplay.currentValue')}:</span>
                  <span className="text-sm font-semibold tabular-nums text-accent-purple max-w-[120px] truncate" title={marker.metricValue || '-'}>
                    {marker.metricValue || '-'}
                  </span>
                </div>
              </>
              )
            })()}

            {marker.markerType === 'command' && (
              <>
                {marker.deviceName && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.device')}:</span>
                    <span className="text-xs">{marker.deviceName}</span>
                  </div>
                )}
                {marker.commandName && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.command')}:</span>
                    <span className="text-xs font-mono">{marker.commandName}</span>
                  </div>
                )}
                <Button
                  size="sm"
                  className="w-full mt-2 bg-info hover:opacity-90 text-primary-foreground"
                  onClick={async (e) => {
                    e.stopPropagation()
                    const markerSourceId = marker.sourceId ?? marker.deviceId
                    if (markerSourceId && marker.command) {
                      const sendCommand = useStore.getState().sendCommand
                      try {
                        await sendCommand(markerSourceId, marker.command)
                      } catch (error) {
                        console.error('Failed to execute command:', error)
                      }
                    }
                  }}
                >
                  <Zap className="h-4 w-4 mr-1" />
                  {t('mapDisplay.executeCommand')}
                </Button>
              </>
            )}

            {(marker.markerType === 'marker' || !marker.markerType) && (
              <>
                {marker.label && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('mapDisplay.label')}:</span>
                    <span>{marker.label}</span>
                  </div>
                )}
              </>
            )}
          </div>
          {_isImg && (
            <button
              className="absolute top-1 right-1 flex items-center justify-center h-3.5 w-3.5 rounded-full bg-overlay-medium text-white/70 hover:text-white hover:bg-overlay-heavy"
              onClick={(e) => { e.stopPropagation(); onClick() }}
            >
              <X className="h-2 w-2" />
            </button>
          )}
        </div>
        )
      })()}
    </>
  )
})

// ============================================================================
// Simple SVG Map (embedded solution without external deps)
// ============================================================================

interface SimpleSvgMapProps {
  center: { lat: number; lng: number }
  zoom: number
  width: number
  height: number
  markers: MapMarker[]
  onMarkerClick: (marker: MapMarker) => void
  onZoomIn: () => void
  onZoomOut: () => void
  onCenterChange?: (newCenter: { lat: number; lng: number }) => void
  interactive: boolean
  tileLayer: string
  onMapClick?: (lat: number, lng: number) => void
  selectedMarkerId?: string | null
  t: (key: string) => string
  onImageClick?: (info: { src: string; deviceName?: string; metricName?: string }) => void
}

function SimpleSvgMap({
  center,
  zoom,
  width,
  height,
  markers,
  onMarkerClick,
  onZoomIn,
  onZoomOut,
  onCenterChange,
  interactive,
  tileLayer,
  onMapClick,
  selectedMarkerId,
  t,
  onImageClick,
}: SimpleSvgMapProps) {
  const [dragOffset, setDragOffset] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const dragStartRef = useRef({ x: 0, y: 0 })
  const mouseDownRef = useRef({ x: 0, y: 0 }) // Track mouse down position for click detection
  const mapRef = useRef<HTMLDivElement>(null)
  const [actualSize, setActualSize] = useState({ width, height })

  // Track actual container size
  useEffect(() => {
    const updateSize = () => {
      if (mapRef.current) {
        const rect = mapRef.current.getBoundingClientRect()
        if (rect.width > 0 && rect.height > 0) {
          setActualSize({ width: rect.width, height: rect.height })
        }
      }
    }

    // Initial size
    updateSize()

    // Watch for size changes
    const observer = new ResizeObserver(updateSize)
    if (mapRef.current) {
      observer.observe(mapRef.current)
    }

    return () => observer.disconnect()
  }, [])

  const TILE_SIZE = 256

  // Get the tile coordinates for a lat/lng
  const getTileCoords = (lat: number, lng: number, z: number) => {
    const n = Math.pow(2, z)
    const x = Math.floor((lng + 180) / 360 * n)
    const latRad = lat * Math.PI / 180
    const y = Math.floor((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2 * n)
    return { x: Math.max(0, Math.min(n - 1, x)), y: Math.max(0, Math.min(n - 1, y)) }
  }

  // Get the pixel offset of a lat/lng within its tile (0-255)
  const getPixelOffsetInTile = (lat: number, lng: number, z: number) => {
    const n = Math.pow(2, z)
    const x = ((lng + 180) / 360 * n * TILE_SIZE) % TILE_SIZE
    const latRad = lat * Math.PI / 180
    const y = ((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2 * n * TILE_SIZE) % TILE_SIZE
    return { x, y }
  }

  // Get the tile URL for a specific tile coordinate
  const getTileUrl = (x: number, y: number, z: number) => {
    const templateUrl = TILE_LAYERS[tileLayer] || TILE_LAYERS.osm
    return templateUrl
      .replace('{z}', String(z))
      .replace('{x}', String(x))
      .replace('{y}', String(y))
  }

  // Calculate which tiles are visible and their positions
  const getVisibleTiles = () => {
    const n = Math.pow(2, zoom)
    const centerTile = getTileCoords(center.lat, center.lng, zoom)
    const centerOffset = getPixelOffsetInTile(center.lat, center.lng, zoom)

    // Use actual container size
    const containerWidth = actualSize.width || width
    const containerHeight = actualSize.height || height

    // How many tiles do we need in each direction?
    const tilesX = Math.ceil(containerWidth / TILE_SIZE) + 2
    const tilesY = Math.ceil(containerHeight / TILE_SIZE) + 2

    const tiles: { x: number; y: number; px: number; py: number; key: string; url: string }[] = []

    // Calculate starting tile (so center is at center of viewport)
    const startTileX = centerTile.x - Math.floor(tilesX / 2)
    const startTileY = centerTile.y - Math.floor(tilesY / 2)

    for (let dy = 0; dy < tilesY; dy++) {
      for (let dx = 0; dx < tilesX; dx++) {
        // Handle wraparound for X, clamp for Y
        const tileX = ((startTileX + dx) % n + n) % n
        const tileY = Math.max(0, Math.min(n - 1, startTileY + dy))

        // Calculate pixel position relative to center of viewport
        // The center point of the map should be at centerOffset within its tile
        const baseX = (dx - Math.floor(tilesX / 2)) * TILE_SIZE
        const baseY = (dy - Math.floor(tilesY / 2)) * TILE_SIZE

        const px = containerWidth / 2 + baseX - centerOffset.x + dragOffset.x
        const py = containerHeight / 2 + baseY - centerOffset.y + dragOffset.y

        const url = getTileUrl(tileX, tileY, zoom)

        tiles.push({ x: tileX, y: tileY, px, py, key: `${zoom}-${tileX}-${tileY}-${dx}-${dy}`, url })
      }
    }

    return tiles
  }

  // Convert lat/lng to viewport pixel coordinates (for markers)
  const latLngToViewport = (lat: number, lng: number) => {
    const n = Math.pow(2, zoom)
    const tile = getTileCoords(lat, lng, zoom)
    const offset = getPixelOffsetInTile(lat, lng, zoom)
    const centerTile = getTileCoords(center.lat, center.lng, zoom)
    const centerOffset = getPixelOffsetInTile(center.lat, center.lng, zoom)

    const containerWidth = actualSize.width || width
    const containerHeight = actualSize.height || height

    // Calculate tile distance from center
    const tileDiffX = tile.x - centerTile.x
    const tileDiffY = tile.y - centerTile.y

    // Handle X wraparound
    const adjustedTileDiffX = ((tileDiffX % n) + n) % n
    const finalTileDiffX = adjustedTileDiffX > n / 2 ? adjustedTileDiffX - n : adjustedTileDiffX

    // Calculate screen position
    const x = containerWidth / 2 + finalTileDiffX * TILE_SIZE + offset.x - centerOffset.x + dragOffset.x
    const y = containerHeight / 2 + tileDiffY * TILE_SIZE + offset.y - centerOffset.y + dragOffset.y

    return { x, y }
  }

  // Convert viewport pixel coordinates to lat/lng (for click handling)
  const viewportToLatLng = (pixelX: number, pixelY: number) => {
    const n = Math.pow(2, zoom)
    const containerWidth = actualSize.width || width
    const containerHeight = actualSize.height || height
    const centerTile = getTileCoords(center.lat, center.lng, zoom)
    const centerOffset = getPixelOffsetInTile(center.lat, center.lng, zoom)

    // Calculate pixel distance from center
    const dx = pixelX - containerWidth / 2 - dragOffset.x
    const dy = pixelY - containerHeight / 2 - dragOffset.y

    // Convert back to world coordinates
    const worldX = centerTile.x * TILE_SIZE + centerOffset.x + dx
    const worldY = centerTile.y * TILE_SIZE + centerOffset.y + dy

    // Convert to lng
    const lng = (worldX / (n * TILE_SIZE)) * 360 - 180

    // Convert to lat
    const latRad = Math.atan(Math.sinh(Math.PI * (1 - 2 * worldY / (n * TILE_SIZE))))
    const lat = latRad * 180 / Math.PI

    return { lat, lng }
  }

  const handleMouseDown = (e: React.MouseEvent) => {
    if (!interactive) return
    setIsDragging(true)
    dragStartRef.current = { x: e.clientX - dragOffset.x, y: e.clientY - dragOffset.y }
    mouseDownRef.current = { x: e.clientX, y: e.clientY }
  }

  const dragRafRef = useRef<number | null>(null)
  const handleMouseMove = (e: React.MouseEvent) => {
    if (!isDragging || !interactive) return
    if (dragRafRef.current !== null) return
    dragRafRef.current = requestAnimationFrame(() => {
      dragRafRef.current = null
      setDragOffset({
        x: e.clientX - dragStartRef.current.x,
        y: e.clientY - dragStartRef.current.y,
      })
    })
  }

  const handleMouseUp = (e: React.MouseEvent) => {
    const wasDragging = isDragging
    const hadDragOffset = dragOffset.x !== 0 || dragOffset.y !== 0
    setIsDragging(false)

    // Check if this was a click (not a drag)
    const mouseUpPos = { x: e.clientX, y: e.clientY }
    const mouseDownPos = mouseDownRef.current
    const distance = Math.sqrt(
      Math.pow(mouseUpPos.x - mouseDownPos.x, 2) +
      Math.pow(mouseUpPos.y - mouseDownPos.y, 2)
    )

    // Update center position based on drag offset
    if (onCenterChange && hadDragOffset) {
      const n = Math.pow(2, zoom)
      const containerWidth = actualSize.width || width
      const containerHeight = actualSize.height || height

      // Convert pixel offset to lat/lng offset
      // Each pixel at zoom level Z represents 360 / (2^Z * 256) degrees of longitude
      const pixelsPerDegreeLng = (n * TILE_SIZE) / 360
      const pixelsPerDegreeLat = (n * TILE_SIZE) / 170 // Approximate for mercator

      const lngOffset = -dragOffset.x / pixelsPerDegreeLng
      const latOffset = dragOffset.y / pixelsPerDegreeLat

      onCenterChange({
        lat: center.lat + latOffset,
        lng: center.lng + lngOffset,
      })

      // Reset drag offset
      setDragOffset({ x: 0, y: 0 })
    }
  }

  const visibleTiles = getVisibleTiles()

  // Handle click on map - only use onClick handler, not mouseUp
  // This prevents duplicate calls and ensures clicks work properly
  const handleMapContainerClick = (e: React.MouseEvent) => {
    if (!onMapClick || !interactive) return

    // Don't trigger if clicking on interactive elements (buttons, etc.)
    const target = e.target as HTMLElement
    if (target.tagName === 'BUTTON' || target.closest('button')) {
      return
    }

    const rect = mapRef.current?.getBoundingClientRect()
    if (rect) {
      const clickX = e.clientX - rect.left
      const clickY = e.clientY - rect.top
      const { lat, lng } = viewportToLatLng(clickX, clickY)
      onMapClick(lat, lng)
    }
  }

  return (
    <div
      ref={mapRef}
      className="relative w-full h-full overflow-hidden bg-muted-20"
      onMouseDown={handleMouseDown}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
      onClick={handleMapContainerClick}
      style={{ cursor: interactive ? 'grab' : 'default' }}
    >
      {/* Render map tiles */}
      {visibleTiles.map((tile) => (
        <img
          key={tile.key}
          src={tile.url}
          alt={`Tile ${tile.x},${tile.y}`}
          className="absolute"
          style={{
            left: tile.px,
            top: tile.py,
            width: TILE_SIZE,
            height: TILE_SIZE,
          }}
          draggable={false}
          loading="lazy"
        />
      ))}

      {/* Render markers as absolute positioned elements */}
      {markers.map((marker) => {
        const pos = latLngToViewport(marker.latitude, marker.longitude)
        const isSelected = selectedMarkerId === marker.id

        return (
          <div
            key={marker.id}
            className="absolute"
            style={{
              left: pos.x,
              top: pos.y,
              pointerEvents: interactive ? 'auto' : 'none',
            }}
          >
            <MapMarkerDot
              marker={marker}
              onClick={() => onMarkerClick(marker)}
              isSelected={isSelected}
              t={t}
              onImageClick={onImageClick}
            />
          </div>
        )
      })}

      {/* Zoom controls */}
      {interactive && (
        <div className="absolute top-2 right-2 flex flex-col gap-1" style={{ pointerEvents: 'auto' }}>
          <Button
            variant="secondary"
            size="icon"
            className="h-6 w-6 bg-bg-80 backdrop-blur"
            onClick={onZoomIn}
          >
            <ZoomIn className="h-4 w-4" />
          </Button>
          <Button
            variant="secondary"
            size="icon"
            className="h-6 w-6 bg-bg-80 backdrop-blur"
            onClick={onZoomOut}
          >
            <ZoomOut className="h-4 w-4" />
          </Button>
        </div>
      )}

    </div>
  )
}

// ============================================================================
// Main Component
// ============================================================================

// Helper function to find metric value with fuzzy matching (handles nested paths like 'values.image')
function findMetricValue(currentValues: Record<string, unknown> | undefined, metricId: string): unknown {
  if (!currentValues) return undefined

  // 1. Try exact match
  if (metricId in currentValues) {
    return currentValues[metricId]
  }

  // 2. Try case-insensitive match
  const lowerMetricId = metricId.toLowerCase()
  for (const key of Object.keys(currentValues)) {
    if (key.toLowerCase() === lowerMetricId) {
      return currentValues[key]
    }
  }

  // 3. Try nested path like "values.image"
  const parts = metricId.split('.')
  let nested: any = currentValues
  for (const part of parts) {
    if (nested && typeof nested === 'object' && part in nested) {
      nested = nested[part]
    } else {
      // Try case-insensitive nested access
      let found = false
      for (const key of Object.keys(nested || {})) {
        if (key.toLowerCase() === part.toLowerCase()) {
          nested = nested[key]
          found = true
          break
        }
      }
      if (!found) {
        return undefined
      }
    }
  }
  return nested
}

// Helper function to extract value from object with fallback field names
function extractValue(obj: Record<string, unknown>, preferredField: string | undefined, fallbackFields: string[]): number | string | null {
  if (preferredField && preferredField in obj) {
    const val = obj[preferredField]
    if (val !== null && val !== undefined) return val as number | string
  }
  for (const field of fallbackFields) {
    if (field in obj) {
      const val = obj[field]
      if (val !== null && val !== undefined) return val as number | string
    }
  }
  return null
}

// Helper function to determine status from value
function determineStatus(value: unknown): 'online' | 'offline' | 'error' | 'warning' | undefined {
  if (value === null || value === undefined) return undefined
  if (typeof value === 'boolean') return value ? 'online' : 'offline'
  if (typeof value === 'string') {
    const lower = value.toLowerCase()
    if (lower === 'online' || lower === 'true' || lower === '1' || lower === 'ok') return 'online'
    if (lower === 'offline' || lower === 'false' || lower === '0') return 'offline'
    if (lower === 'error' || lower === 'failed') return 'error'
    if (lower === 'warning' || lower === 'warn') return 'warning'
  }
  return undefined
}

export function MapDisplay({
  dataSource,
  markers: propMarkers = [],
  bindings,
  center = { lat: 39.9042, lng: 116.4074 }, // Default: Beijing
  zoom = 10,
  minZoom = 2,
  maxZoom = 18,
  size = 'md',
  showControls = true,
  showFullscreen = true,
  interactive = true,
  tileLayer = 'osm',
  markerColor,
  className,
  deviceBinding,
  onMapClick,
  title,
}: MapDisplayProps & { title?: string }) {
  const { t } = useTranslation('dashboardComponents')
  const sendCommand = useSendCommand()

  // Get devices from store for real-time metric updates
  const devices = useStore(state => state.devices)
  // Only subscribe to telemetry for devices referenced by bindings (avoids re-rendering on every device's telemetry change)
  const boundDeviceIds = useMemo(() => {
    if (!bindings || bindings.length === 0) return [] as string[]
    return bindings
      .map(b => b.dataSource?.id || b.dataSource?.sourceId || (b.dataSource as any)?.deviceId || (b.dataSource?.metricId ? b.dataSource.metricId.split(':')[0] : undefined))
      .filter((id): id is string => !!id)
  }, [bindings])
  const deviceTelemetry = useStore(useCallback((state: any) => {
    if (boundDeviceIds.length === 0) return {} as Record<string, Record<string, unknown>>
    const result: Record<string, Record<string, unknown>> = {}
    for (const id of boundDeviceIds) {
      if (state.deviceTelemetry[id]) result[id] = state.deviceTelemetry[id]
    }
    return result
  }, [boundDeviceIds]), shallow)

  // Helper function to get device metric value with fuzzy matching
  const getDeviceMetricValue = useCallback((deviceId: string, metricId: string): string | number | undefined => {
    if (!deviceId) return undefined
    const device = findDevice(devices, deviceId)
    const cv = deviceTelemetry[deviceId] || device?.current_values
    if (!cv) return undefined
    const value = findMetricValue(cv, metricId)
    if (value !== undefined && value !== null) {
      return typeof value === 'number' ? value : String(value)
    }
    return undefined
  }, [devices, deviceTelemetry])

  // Helper function to get device status
  const getDeviceStatus = useCallback((deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
    if (!deviceId) return undefined
    const device = findDevice(devices, deviceId)
    if (!device) return undefined
    return device.online ? 'online' : 'offline'
  }, [devices])

  // Convert bindings to markers - this preserves the type info from bindings
  const convertBindingsToMarkers = useCallback((bindings: MapBinding[] | undefined): MapMarker[] => {
    if (!bindings || bindings.length === 0) return []

    const getDeviceName = (deviceId: string) => {
      const device = findDevice(devices, deviceId)
      return device?.name || deviceId
    }

    return bindings.map((binding): MapMarker => {
      const position = binding.position === 'auto' || !binding.position
        ? center
        : binding.position

      const ds = binding.dataSource
      // Resolve device ID from dataSource: try sourceId, then deviceId field, then metricId prefix
      const deviceId = ds.id || ds.sourceId || (ds as any).deviceId || (ds.metricId ? ds.metricId.split(':')[0] : undefined)

      const marker: MapMarker = {
        id: binding.id,
        deviceId,
        sourceId: deviceId,
        latitude: position.lat,
        longitude: position.lng,
        label: binding.name,
        // IMPORTANT: Use binding.type to set markerType
        markerType: binding.type || binding.icon || 'device',
      }

      // Set type-specific fields
      if (binding.type === 'metric') {
        const metricId = ds?.metricId || ds?.property || ''
        const metricValue = getDeviceMetricValue(deviceId || '', metricId)
        marker.metricValue = metricValue !== undefined ? String(metricValue) : '-'
        marker.markerType = 'metric'
        marker.deviceName = getDeviceName(deviceId || '')
        marker.metricName = metricId
      } else if (binding.type === 'command') {
        marker.command = ds?.command
        marker.markerType = 'command'
        marker.deviceName = getDeviceName(deviceId || '')
        marker.commandName = ds?.command || ''
      } else if (binding.type === 'device') {
        const status = getDeviceStatus(deviceId || '')
        marker.status = status || (deviceId ? 'offline' : undefined)
        marker.markerType = 'device'
        marker.deviceName = getDeviceName(deviceId || '')
      } else if (binding.type === 'marker') {
        marker.markerType = 'marker'
      }

      return marker
    })
  }, [center, devices, getDeviceMetricValue, getDeviceStatus])

  // Transform function to convert device data to MapMarker format
  const transformDeviceDataToMarkers = useCallback((rawData: unknown): MapMarker[] => {
    // If already in correct format, return as-is
    if (Array.isArray(rawData) && rawData.length > 0) {
      const firstItem = rawData[0]
      if (typeof firstItem === 'object' && firstItem !== null &&
          'latitude' in firstItem && 'longitude' in firstItem) {
        return rawData as MapMarker[]
      }
    }

    // If it's a single device object, convert it
    if (typeof rawData === 'object' && rawData !== null && !Array.isArray(rawData)) {
      const data = rawData as Record<string, unknown>
      const markers: MapMarker[] = []

      // Try to find lat/lng from various possible field names
      const lat = extractValue(data, deviceBinding?.latField, ['lat', 'latitude', 'y'])
      const lng = extractValue(data, deviceBinding?.lngField, ['lng', 'lon', 'longitude', 'x'])

      if (lat !== null && lng !== null) {
        const label = extractValue(data, deviceBinding?.labelField, ['name', 'id', 'device_id', 'label'])
        const value = extractValue(data, deviceBinding?.valueField, ['value', 'metric', 'temperature', 'humidity'])
        const status = extractValue(data, deviceBinding?.statusField, ['status', 'online', 'state'])
        const deviceId = data.id || data.device_id

        markers.push({
          id: String(label || deviceId || 'marker'),
          deviceId: deviceId ? String(deviceId) : undefined,
          sourceId: deviceId ? String(deviceId) : undefined,
          latitude: Number(lat),
          longitude: Number(lng),
          label: String(label || ''),
          value: value !== null ? Number(value) : undefined,
          status: determineStatus(status),
          metricValue: value !== null ? String(value) : undefined,
          markerType: 'device',  // Device marker
        })
      }

      return markers
    }

    // If it's an array of devices, convert each one
    if (Array.isArray(rawData)) {
      return rawData.flatMap((item) => {
        if (typeof item === 'object' && item !== null) {
          const data = item as Record<string, unknown>
          const lat = extractValue(data, deviceBinding?.latField, ['lat', 'latitude', 'y'])
          const lng = extractValue(data, deviceBinding?.lngField, ['lng', 'lon', 'longitude', 'x'])

          if (lat !== null && lng !== null) {
            const label = extractValue(data, deviceBinding?.labelField, ['name', 'id', 'device_id', 'label'])
            const value = extractValue(data, deviceBinding?.valueField, ['value', 'metric', 'temperature', 'humidity'])
            const status = extractValue(data, deviceBinding?.statusField, ['status', 'online', 'state'])
            const deviceId = data.id || data.device_id

            return [{
              id: String(label || deviceId || `marker-${Math.random()}`),
              deviceId: deviceId ? String(deviceId) : undefined,
              sourceId: deviceId ? String(deviceId) : undefined,
              latitude: Number(lat),
              longitude: Number(lng),
              label: String(label || ''),
              value: value !== null ? Number(value) : undefined,
              status: determineStatus(status),
              metricValue: value !== null ? String(value) : undefined,
              markerType: 'device',  // Device marker
            } as MapMarker]
          }
        }
        return []
      })
    }

    return propMarkers
  }, [deviceBinding, propMarkers])

  const { data, loading, error } = useDataSource<MapMarker[]>(dataSource, {
    fallback: propMarkers,
    transform: transformDeviceDataToMarkers,
  })

  // Convert bindings to markers - bindings take priority over dataSource data
  const bindingsMarkers = useMemo(() => convertBindingsToMarkers(bindings), [convertBindingsToMarkers, bindings])

  // Determine final markers with priority:
  // 1. bindings (highest priority - contains type info from config)
  // 2. dataSource data (for backward compatibility)
  // 3. propMarkers (fallback)
  const markers = bindings && bindings.length > 0
    ? bindingsMarkers
    : !dataSource
      ? propMarkers
      : (error
        ? propMarkers
        : ((data && data.length > 0) ? data : propMarkers))

  const [currentZoom, setCurrentZoom] = useState(zoom)
  const [currentCenter, setCurrentCenter] = useState(center)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [selectedMarker, setSelectedMarker] = useState<MapMarker | null>(null)
  const [fullscreenImage, setFullscreenImage] = useState<{ src: string; deviceName?: string; metricName?: string } | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [containerSize, setContainerSize] = useState({ width: 400, height: 300 })

  // Sync internal state when props change (e.g., after configuration update)
  useEffect(() => {
    setCurrentZoom(zoom)
  }, [zoom])

  useEffect(() => {
    setCurrentCenter(prev => {
      // Only update if values actually changed (prevent infinite loop)
      if (prev.lat === center.lat && prev.lng === center.lng) {
        return prev
      }
      return center
    })
  }, [center.lat, center.lng])

  // Track container size
  useEffect(() => {
    const updateSize = () => {
      if (containerRef.current) {
        setContainerSize({
          width: containerRef.current.offsetWidth,
          height: containerRef.current.offsetHeight,
        })
      }
    }

    updateSize()
    const observer = new ResizeObserver(updateSize)
    if (containerRef.current) {
      observer.observe(containerRef.current)
    }

    return () => observer.disconnect()
  }, [])

  const sizeConfig = dashboardComponentSize[size]

  const handleZoomIn = useCallback(() => {
    setCurrentZoom(prev => Math.min(prev + 1, maxZoom))
  }, [maxZoom])

  const handleZoomOut = useCallback(() => {
    setCurrentZoom(prev => Math.max(prev - 1, minZoom))
  }, [minZoom])

  const handleMarkerClick = useCallback((marker: MapMarker) => {
    if (!interactive) return

    // Toggle selection: if clicking the same marker, deselect it
    if (selectedMarker?.id === marker.id) {
      setSelectedMarker(null)
      return
    }

    setSelectedMarker(marker)

    // Execute marker's click handler if defined
    if (marker.onClick) {
      marker.onClick()
    }
  }, [interactive, selectedMarker])

  // Loading state - only show skeleton for the initial empty load.
  // During refreshes, keep rendering the previous/fallback markers so Tauri
  // fast scrolling does not look like the whole component disappeared.
  const showLoading = loading && !!dataSource && markers.length === 0
  if (showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full rounded-lg')} />
      </div>
    )
  }

  const mapContent = (
    <>
      {/* Normal view */}
      <div className={cn(dashboardCardBase, 'relative overflow-hidden flex flex-col', className)}>
        {/* Header */}
        {showControls && (
          <div className="flex items-center justify-between px-3 py-2 border-b bg-muted-20">
            <div className="flex items-center gap-2">
              <MapPin className="h-4 w-4 text-muted-foreground" />
              <span className="text-sm font-medium">{title || 'Map'}</span>
              <span className="text-xs text-muted-foreground">
                ({currentCenter.lat.toFixed(2)}, {currentCenter.lng.toFixed(2)})
              </span>
            </div>
            <div className="flex items-center gap-1">
              {showFullscreen && !isFullscreen && (
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-6 w-6"
                  onClick={() => setIsFullscreen(true)}
                >
                  <Maximize2 className="h-4 w-4" />
                </Button>
              )}
            </div>
          </div>
        )}

        {/* Map container */}
        <div
          ref={containerRef}
          className="flex-1 relative min-h-[200px]"
        >
          <SimpleSvgMap
            center={currentCenter}
            zoom={currentZoom}
            width={containerSize.width}
            height={containerSize.height}
            markers={markers}
            onMarkerClick={handleMarkerClick}
            onZoomIn={handleZoomIn}
            onZoomOut={handleZoomOut}
            onCenterChange={setCurrentCenter}
            interactive={interactive}
            tileLayer={tileLayer}
            onMapClick={onMapClick}
            selectedMarkerId={selectedMarker?.id}
            t={t}
            onImageClick={setFullscreenImage}
          />
        </div>
      </div>
    </>
  )

  // Fullscreen overlay (rendered via Portal — single instance only)
  const fullscreenOverlay = isFullscreen ? createPortal(
    <div className="fixed inset-0 z-[110] flex flex-col bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b bg-bg-95">
        <div className="flex items-center gap-2">
          <MapPin className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium">Map</span>
          <span className="text-xs text-muted-foreground">
            ({currentCenter.lat.toFixed(2)}, {currentCenter.lng.toFixed(2)})
          </span>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => setIsFullscreen(false)}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Map container */}
      <div className="flex-1 relative">
        <SimpleSvgMap
          center={currentCenter}
          zoom={currentZoom}
          width={containerSize.width}
          height={containerSize.height}
          markers={markers}
          onMarkerClick={handleMarkerClick}
          onZoomIn={handleZoomIn}
          onZoomOut={handleZoomOut}
          onCenterChange={setCurrentCenter}
          interactive={interactive}
          tileLayer={tileLayer}
          onMapClick={onMapClick}
          selectedMarkerId={selectedMarker?.id}
          t={t}
          onImageClick={setFullscreenImage}
        />
      </div>
    </div>, getPortalRoot()
  ) : null

  return (
    <>
      {/* Normal view (hidden when fullscreen to avoid dual rendering) */}
      {!isFullscreen && mapContent}
      {fullscreenOverlay}
      {fullscreenImage && createPortal(
        <div
          className="fixed inset-0 z-[110] bg-overlay-heavy backdrop-blur-sm flex items-center justify-center"
          onClick={() => setFullscreenImage(null)}
        >
          <div className="absolute top-4 left-4 right-4 flex items-center justify-between">
            <div className="flex items-center gap-3 px-3 py-2 rounded-lg bg-overlay-medium text-white">
              {fullscreenImage.deviceName && <span className="text-sm font-medium">{fullscreenImage.deviceName}</span>}
              {fullscreenImage.metricName && <span className="text-xs text-white/70 font-mono">{fullscreenImage.metricName}</span>}
            </div>
            <Button variant="ghost" size="icon" className="text-white hover:bg-white/20" onClick={() => setFullscreenImage(null)}>
              <X className="h-5 w-5" />
            </Button>
          </div>
          <img
            src={fullscreenImage.src}
            alt={fullscreenImage.metricName || 'metric'}
            className="w-full h-full object-cover"
            onClick={(e) => e.stopPropagation()}
          />
        </div>,
        getPortalRoot()
      )}
    </>
  )
}

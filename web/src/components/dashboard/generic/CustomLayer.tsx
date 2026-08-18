/**
 * Custom Layer Component
 *
 * A free-form container component that can hold other components.
 * Supports placing devices, metrics, commands, and other content at custom positions.
 * Can be used as a background layer with overlaid interactive elements.
 * Data binding support similar to Map component with typed bindings.
 */

import { getPortalRoot } from '@/lib/portal'
import { findDevice } from '@/lib/deviceUtils'
import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { createPortal } from 'react-dom'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { normalizeImageUrl } from '@/lib/imageUtils'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { useDataSource } from '@/hooks/useDataSource'
import {
  Layers,
  Plus,
  Settings,
  Lock,
  Unlock,
  Eye,
  EyeOff,
  GripVertical,
  Maximize2,
  Minimize2,
  Image as ImageIcon,
  Square,
  MapPin,
  Activity,
  Zap,
  Type,
  Sparkles,
  ZoomIn,
  ZoomOut,
  RotateCcw,
  X,
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { useStore } from '@/store'
import { shallow } from 'zustand/shallow'

// ============================================================================
// Types
// ============================================================================

export type LayerBindingType = 'device' | 'metric' | 'command' | 'text' | 'icon'

export interface LayerBinding {
  id: string
  type: LayerBindingType
  name: string
  dataSource: DataSource
  position?: { x: number; y: number } | 'auto' // Relative position (0-100%)
  icon?: LayerBindingType
  // Styling options
  color?: string
  backgroundColor?: string
  fontSize?: 'xs' | 'sm' | 'md' | 'lg'
  fontWeight?: 'normal' | 'medium' | 'semibold' | 'bold'
  opacity?: number // Icon opacity (0-100)
  markerSize?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' // Marker/icon size
}

export interface LayerItem {
  id: string
  type: 'device' | 'metric' | 'command' | 'text' | 'icon' | 'component'
  position: { x: number; y: number } // Relative position (0-100%)
  size?: { width: number; height: number }

  // Content
  label?: string
  value?: string | number
  icon?: string
  deviceId?: string
  metricId?: string
  command?: string
  commandParams?: Record<string, unknown>

  // Styling
  color?: string
  backgroundColor?: string
  borderColor?: string
  fontSize?: 'xs' | 'sm' | 'md' | 'lg'
  fontWeight?: 'normal' | 'medium' | 'semibold' | 'bold'
  opacity?: number // Icon opacity (0-100)
  markerSize?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' // Marker/icon size

  // State
  visible?: boolean
  locked?: boolean
  draggable?: boolean

  // Actions
  onClick?: () => void
  onValueChange?: (value: unknown) => void

  // Real-time data tracking
  deviceName?: string
  metricName?: string
  status?: 'online' | 'offline' | 'error' | 'warning'
}

export interface CustomLayerProps {
  dataSource?: DataSource
  items?: LayerItem[]
  bindings?: LayerBinding[] // Bindings from config with type and position info

  // Layer options
  backgroundType?: 'color' | 'image' | 'transparent' | 'grid'
  backgroundColor?: string
  backgroundImage?: string
  gridSize?: number

  // Display options
  size?: 'sm' | 'md' | 'lg'
  interactive?: boolean
  showControls?: boolean
  editable?: boolean
  showFullscreen?: boolean

  // Scale options
  maintainAspectRatio?: boolean
  aspectRatio?: number

  // Callbacks
  onItemsChange?: (items: LayerItem[]) => void
  onBindingsChange?: (bindings: LayerBinding[]) => void
  onLayerClick?: (x: number, y: number) => void

  className?: string
}

// ============================================================================
// Layer Item Component
// ============================================================================

interface LayerItemComponentProps {
  item: LayerItem
  isEditing: boolean
  isSelected: boolean
  onSelect: () => void
  onDrag: (item: LayerItem, newPosition: { x: number; y: number }) => void
  onToggleVisibility: (id: string) => void
  onToggleLock: (id: string) => void
  onExecuteCommand?: (deviceId: string, command: string) => Promise<void>
  onImageClick?: (info: { src: string; deviceName?: string; metricName?: string }) => void
  t: (key: string) => string
}

// Type config for styling similar to MapDisplay
const TYPE_CONFIG = {
  device: {
    icon: MapPin,
    color: 'text-primary-foreground',
    bgColor: 'bg-success',
    borderColor: 'border-success',
  },
  metric: {
    icon: Activity,
    color: 'text-primary-foreground',
    bgColor: 'bg-accent-purple',
    borderColor: 'border-accent-purple',
  },
  command: {
    icon: Zap,
    color: 'text-primary-foreground',
    bgColor: 'bg-info',
    borderColor: 'border-info',
  },
  text: {
    icon: Type,
    color: 'text-foreground',
    bgColor: 'bg-muted',
    borderColor: 'border-border',
  },
  icon: {
    icon: Sparkles,
    color: 'text-primary-foreground',
    bgColor: 'bg-accent-orange',
    borderColor: 'border-accent-orange',
  },
  component: {
    icon: Layers,
    color: 'text-primary-foreground',
    bgColor: 'bg-accent-cyan',
    borderColor: 'border-accent-cyan',
  },
} as const

function LayerItemComponent({
  item,
  isEditing,
  isSelected,
  onSelect,
  onDrag,
  onToggleVisibility,
  onToggleLock,
  onExecuteCommand,
  onImageClick,
  t,
}: LayerItemComponentProps) {
  const [isDragging, setIsDragging] = useState(false)
  const [hasMoved, setHasMoved] = useState(false) // Track if mouse moved during drag
  const [dragStartMouse, setDragStartMouse] = useState({ x: 0, y: 0 })
  const [dragStartItem, setDragStartItem] = useState({ x: 0, y: 0 })
  const [showDetails, setShowDetails] = useState(false)
  const itemRef = useRef<HTMLDivElement>(null)
  const rafRef = useRef<number | null>(null)

  // Determine if this is a text-only item (shows full label, others show icon only)
  const isTextOnly = item.type === 'text' && !item.icon

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    // Only drag with left click, not for clicking to view details
    if (item.locked) return
    if (e.button !== 0) return
    e.stopPropagation()
    setHasMoved(false) // Reset movement flag
    setIsDragging(true)
    // Store both mouse start AND item start positions
    setDragStartMouse({ x: e.clientX, y: e.clientY })
    setDragStartItem({ x: item.position.x, y: item.position.y })
    onSelect()
  }, [item.locked, item.position, onSelect])

  useEffect(() => {
    if (!isDragging) return

    const handleMouseMove = (e: MouseEvent) => {
      // Mark that we've moved
      setHasMoved(true)

      // Cancel any pending animation frame
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current)
      }

      // Use requestAnimationFrame for smooth updates
      rafRef.current = requestAnimationFrame(() => {
        // Calculate delta from initial positions
        const deltaX = e.clientX - dragStartMouse.x
        const deltaY = e.clientY - dragStartMouse.y

        // Get the canvas container and zoom level
        const canvas = itemRef.current?.closest('[style*="transform"]') as HTMLElement
        const container = itemRef.current?.closest('.relative.w-full.h-full')?.parentElement

        if (container) {
          const containerWidth = container.offsetWidth
          const containerHeight = container.offsetHeight

          // Get current zoom factor from the transform style
          const currentZoom = canvas ? parseFloat(canvas.style.transform?.match(/scale\(([^)]+)\)/)?.[1] || '1') : 1

          // Convert pixel delta to percentage, accounting for zoom
          const percentDeltaX = (deltaX / containerWidth / currentZoom) * 100
          const percentDeltaY = (deltaY / containerHeight / currentZoom) * 100

          // Calculate new position from INITIAL item position + delta
          const newX = dragStartItem.x + percentDeltaX
          const newY = dragStartItem.y + percentDeltaY

          // Clamp to bounds (keep at least 5% inside)
          const clampedX = Math.max(5, Math.min(95, newX))
          const clampedY = Math.max(5, Math.min(95, newY))

          // Directly update the parent via callback
          onDrag(item, { x: clampedX, y: clampedY })
        }
      })
    }

    const handleMouseUp = () => {
      setIsDragging(false)
      setHasMoved(false) // Reset movement flag
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current)
        rafRef.current = null
      }
    }

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current)
      }
    }
  }, [isDragging, dragStartMouse, dragStartItem, item, onDrag])

  // Handle click to show details (for non-text items)
  const handleClick = useCallback((e: React.MouseEvent) => {
    // Only show details if we didn't drag (hasMoved would be true if we dragged)
    if (hasMoved) return
    if (!isTextOnly) {
      // Show details popup for non-text items
      setShowDetails(true)
    }
    if (item.onClick) {
      e.stopPropagation()
      item.onClick()
    }
  }, [hasMoved, isTextOnly, item.onClick])

  // Helper functions (must be after all hooks)
  const getFontSizeClass = () => {
    switch (item.fontSize) {
      case 'xs': return 'text-xs'
      case 'sm': return 'text-sm'
      case 'md': return 'text-base'
      case 'lg': return 'text-lg'
      default: return 'text-sm'
    }
  }

  const getFontWeightClass = () => {
    switch (item.fontWeight) {
      case 'normal': return 'font-normal'
      case 'medium': return 'font-medium'
      case 'semibold': return 'font-semibold'
      case 'bold': return 'font-bold'
      default: return 'font-medium'
    }
  }

  const getMarkerSizeClass = () => {
    switch (item.markerSize) {
      case 'xs': return 'w-6 h-6' // 24px
      case 'sm': return 'w-8 h-8' // 32px
      case 'md': return 'w-10 h-10' // 40px (default)
      case 'lg': return 'w-12 h-12' // 48px
      case 'xl': return 'w-16 h-16' // 64px
      default: return 'w-10 h-10'
    }
  }

  const getIconSizeClass = () => {
    switch (item.markerSize) {
      case 'xs': return 'h-4 w-4' // 12px
      case 'sm': return 'h-4 w-4' // 16px
      case 'md': return 'h-5 w-5' // 20px (default)
      case 'lg': return 'h-6 w-6' // 24px
      case 'xl': return 'h-8 w-8' // 32px
      default: return 'h-5 w-5'
    }
  }

  // Only hide if explicitly set to false
  if (item.visible === false) {
    return null
  }

  // Get type config for styling
  const typeConfig = TYPE_CONFIG[item.type] || TYPE_CONFIG.text
  const Icon = typeConfig.icon

  // Status indicator color
  const getStatusColor = () => {
    switch (item.status) {
      case 'online': return 'bg-success'
      case 'offline': return 'bg-muted-foreground'
      case 'error': return 'bg-error'
      case 'warning': return 'bg-warning'
      default: return null
    }
  }

  return (
    <>
      <div
        ref={itemRef}
        className={cn(
          'absolute transform -translate-x-1/2 -translate-y-1/2',
          // Always show grab cursor for draggable items (unless locked)
          !item.locked && 'cursor-grab active:cursor-grabbing'
          // Selection border removed for cleaner appearance
        )}
        style={{
          left: `${item.position.x}%`,
          top: `${item.position.y}%`,
        }}
        onMouseDown={handleMouseDown}
        onClick={handleClick}
      >
        {/* Icon-only display for non-text items */}
        {!isTextOnly ? (
          <div
            className={cn(
              'relative rounded-full flex items-center justify-center border-2 shadow-lg',
              getMarkerSizeClass(),
              typeConfig.bgColor,
              typeConfig.borderColor,
            )}
            style={{
              backgroundColor: item.backgroundColor,
              borderColor: item.borderColor,
              opacity: item.opacity !== undefined ? item.opacity / 100 : undefined,
            }}
          >
            <Icon className={cn(getIconSizeClass(), typeConfig.color)} style={{ color: item.color }} />

            {/* Status indicator for devices */}
            {item.type === 'device' && item.status && (
              <span className={cn(
                'absolute -top-0.5 -right-0.5 w-4 h-4 rounded-full border-2 border-background',
                getStatusColor()
              )} />
            )}

            {/* Command indicator */}
            {item.type === 'command' && (
              <span className="absolute -bottom-0.5 -right-0.5 w-4 h-4 bg-info rounded-full border-2 border-background animate-pulse" />
            )}
          </div>
        ) : (
          // Full text display for text items
          <div
            className={cn(
              'inline-flex items-center gap-1.5 px-2 py-1.5 rounded-lg border shadow-sm',
              getFontSizeClass(),
              getFontWeightClass(),
              !item.backgroundColor && typeConfig.bgColor,
              !item.borderColor && typeConfig.borderColor,
              !item.color && typeConfig.color
            )}
            style={{
              backgroundColor: item.backgroundColor,
              borderColor: item.borderColor,
              color: item.color,
              opacity: item.opacity !== undefined ? item.opacity / 100 : undefined,
            }}
          >
            {/* Icon for text items if specified */}
            {item.icon ? (
              <span className="flex-shrink-0">{item.icon}</span>
            ) : (
              <Icon className="h-4 w-4 flex-shrink-0 text-muted-foreground" />
            )}

            {/* Label */}
            {item.label && (
              <span className={cn(item.type === 'device' && 'font-medium')}>
                {item.label}
              </span>
            )}

            {/* Value */}
            {item.value !== undefined && (() => {
              const img = normalizeImageUrl(item.value)
              return img ? (
                <img
                  src={img.src}
                  alt={item.label || 'metric'}
                  className="h-5 w-5 rounded object-cover flex-shrink-0"
                />
              ) : (
                <span className={cn(
                  'text-xs tabular-nums max-w-[100px] truncate',
                  item.type === 'metric' ? 'font-semibold' : ''
                )} title={String(item.value)}>
                  {item.value}
                </span>
              )
            })()}
          </div>
        )}

        {/* Edit controls */}
        {isEditing && isSelected && (
          <div className="absolute -top-6 left-1/2 -translate-x-1/2 flex items-center gap-1">
            <Button
              variant="secondary"
              size="icon"
              className="h-5 w-5 bg-bg-90"
              onClick={(e) => { e.stopPropagation(); onToggleVisibility(item.id); }}
              title={(item.visible as boolean | undefined) === false ? t('customLayer.show') : t('customLayer.hide')}
            >
              {(item.visible as boolean | undefined) === false ? <EyeOff className="h-2.5 w-2.5" /> : <Eye className="h-2.5 w-2.5" />}
            </Button>
            <Button
              variant="secondary"
              size="icon"
              className="h-5 w-5 bg-bg-90"
              onClick={(e) => { e.stopPropagation(); onToggleLock(item.id); }}
            >
              {item.locked ? <Lock className="h-2.5 w-2.5" /> : <Unlock className="h-2.5 w-2.5" />}
            </Button>
          </div>
        )}
      </div>

      {/* Details Popup */}
      {showDetails && !isTextOnly && (() => {
        const _isImg = !!normalizeImageUrl(item.type === 'metric' ? item.value : null)
        return (
        <div
          className={cn(
            'absolute z-50 rounded-lg shadow-xl animate-in fade-in zoom-in-95 duration-150',
            _isImg
              ? 'min-w-[160px] max-w-[280px] overflow-hidden'
              : 'min-w-[200px] max-w-[280px] border bg-bg-95 backdrop-blur p-3',
          )}
          style={{
            left: `${item.position.x}%`,
            top: `${item.position.y + 2}%`,
            transform: 'translate(-50%, 0)',
          }}
          onClick={(e) => e.stopPropagation()}
        >
          {!_isImg && (<>
          {/* Close button */}
          <button
            className="absolute top-1.5 right-1.5 p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground flex items-center justify-center"
            onClick={() => setShowDetails(false)}
          >
            <X className="h-4 w-4" />
          </button>

          {/* Header with icon and type */}
          <div className="flex items-center gap-2 mb-2 pb-2 border-b">
            <div className={cn('p-1.5 rounded-md', typeConfig.bgColor)}>
              <Icon className={cn('h-4 w-4', typeConfig.color)} />
            </div>
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium text-muted-foreground uppercase">
                {item.type === 'device' && t('customLayer.device')}
                {item.type === 'metric' && t('customLayer.metric')}
                {item.type === 'command' && t('customLayer.command')}
                {item.type === 'icon' && t('customLayer.iconLabel')}
              </p>
              <p className="text-sm font-semibold truncate">{item.label || item.deviceId || item.id}</p>
            </div>
          </div>
          </>)}

          {/* Content based on type */}
          <div className="space-y-1.5 text-sm">
            {item.type === 'device' && (
              <>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.deviceId')}:</span>
                  <span className="font-mono text-xs">{item.deviceId}</span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.status')}:</span>
                  <span className={cn(
                    'flex items-center gap-1.5',
                    item.status === 'online' && 'text-success',
                    item.status === 'offline' && 'text-muted-foreground'
                  )}>
                    <span className={cn('w-2 h-2 rounded-full', getStatusColor())} />
                    {item.status === 'online' ? t('customLayer.online') : t('customLayer.offline')}
                  </span>
                </div>
              </>
            )}

            {item.type === 'metric' && (() => {
              const img = normalizeImageUrl(item.value)
              if (img) {
                return (
                  <div className="relative">
                    <img
                      src={img.src}
                      alt={item.metricName || 'metric'}
                      className="w-full max-h-[120px] object-cover cursor-pointer hover:opacity-80 transition-opacity"
                      onClick={(e) => {
                        e.stopPropagation()
                        onImageClick?.({ src: img.src, deviceName: item.deviceName, metricName: item.metricName })
                      }}
                    />
                    {(item.deviceName || item.deviceId) && (
                      <span className="absolute bottom-0 inset-x-0 px-1.5 py-0.5 text-[10px] text-white bg-overlay-medium truncate">
                        {item.deviceName || item.deviceId}
                      </span>
                    )}
                  </div>
                )
              }
              return (
              <>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.device')}:</span>
                  <span className="text-xs">{item.deviceName || item.deviceId}</span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.metric')}:</span>
                  <span className="text-xs font-mono">{item.metricName || item.metricId}</span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.currentValue')}:</span>
                  <span className={cn('text-sm font-semibold tabular-nums max-w-[120px] truncate', typeConfig.color)}
                    title={item.value !== undefined ? String(item.value) : '-'}>
                    {item.value !== undefined ? item.value : '-'}
                  </span>
                </div>
              </>
              )
            })()}

            {item.type === 'command' && (
              <>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.device')}:</span>
                  <span className="text-xs">{item.deviceName || item.deviceId}</span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.command')}:</span>
                  <span className="text-xs font-mono">{item.command}</span>
                </div>
                <Button
                  size="sm"
                  className="w-full mt-2 bg-info hover:opacity-90 text-primary-foreground"
                  onClick={async (e) => {
                    e.stopPropagation()
                    if (onExecuteCommand && item.deviceId && item.command) {
                      await onExecuteCommand(item.deviceId, item.command)
                    }
                    setShowDetails(false)
                  }}
                >
                  <Zap className="h-4 w-4 mr-1" />
                  {t('mapDisplay.executeCommand')}
                </Button>
              </>
            )}

            {item.type === 'icon' && (
              <>
                <div className="flex justify-between items-center">
                  <span className="text-muted-foreground">{t('customLayer.label')}:</span>
                  <span>{item.label || '-'}</span>
                </div>
                {item.value !== undefined && (
                  <div className="flex justify-between items-center">
                    <span className="text-muted-foreground">{t('customLayer.value')}:</span>
                    <span className="tabular-nums">{item.value}</span>
                  </div>
                )}
              </>
            )}
          </div>
          {_isImg && (
            <button
              className="absolute top-1 right-1 flex items-center justify-center h-3.5 w-3.5 rounded-full bg-overlay-medium text-white/70 hover:text-white hover:bg-overlay-heavy"
              onClick={() => setShowDetails(false)}
            >
              <X className="h-2 w-2" />
            </button>
          )}
        </div>
        )
      })()}
    </>
  )
}

// ============================================================================
// Background Component
// ============================================================================

interface LayerBackgroundProps {
  type: 'color' | 'image' | 'transparent' | 'grid'
  color?: string
  image?: string
  gridSize?: number
}

/**
 * Generate background style based on type
 * This is used both by LayerBackground component and directly on the canvas
 */
function getBackgroundStyle(
  type: 'color' | 'image' | 'transparent' | 'grid',
  color?: string,
  image?: string,
  gridSize = 20,
  maintainAspectRatio?: boolean,
  aspectRatio?: number
): React.CSSProperties {
  const backgroundStyle: React.CSSProperties = {}

  switch (type) {
    case 'color':
      backgroundStyle.backgroundColor = color || 'color-mix(in oklch, var(--muted) 30%, transparent)'
      backgroundStyle.backgroundImage = 'none'
      break
    case 'image':
      backgroundStyle.backgroundImage = image ? `url(${image})` : undefined
      backgroundStyle.backgroundSize = 'cover'
      backgroundStyle.backgroundPosition = 'center'
      break
    case 'transparent':
      // Chessboard pattern for transparency
      backgroundStyle.backgroundImage = `
        linear-gradient(45deg, color-mix(in oklch, var(--muted) 10%, transparent) 25%, transparent 25%),
        linear-gradient(-45deg, color-mix(in oklch, var(--muted) 10%, transparent) 25%, transparent 25%),
        linear-gradient(45deg, transparent 75%, color-mix(in oklch, var(--muted) 10%, transparent) 75%),
        linear-gradient(-45deg, transparent 75%, color-mix(in oklch, var(--muted) 10%, transparent) 75%)
      `
      backgroundStyle.backgroundSize = '20px 20px'
      backgroundStyle.backgroundPosition = '0 0, 0 10px, 10px -10px, -10px 0px'
      break
    case 'grid':
      backgroundStyle.backgroundColor = color || 'var(--background)'
      backgroundStyle.backgroundImage = `
        linear-gradient(to right, color-mix(in oklch, var(--border) 30%, transparent) 1px, transparent 1px),
        linear-gradient(to bottom, color-mix(in oklch, var(--border) 30%, transparent) 1px, transparent 1px)
      `
      backgroundStyle.backgroundSize = `${gridSize}px ${gridSize}px`
      break
  }

  // Handle aspect ratio if needed
  if (maintainAspectRatio && aspectRatio) {
    backgroundStyle.aspectRatio = `${aspectRatio}`
  }

  return backgroundStyle
}

function LayerBackground({ type, color, image, gridSize = 20 }: LayerBackgroundProps) {
  const backgroundStyle = getBackgroundStyle(type, color, image, gridSize)
  return (
    <div
      className="absolute inset-0 -z-10"
      style={backgroundStyle}
      data-bg-type={type}
      data-bg-color={color}
    />
  )
}

// ============================================================================
// Helper Functions
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

// ============================================================================
// Main Component
// ============================================================================

export function CustomLayer({
  dataSource,
  items: propItems = [],
  bindings,
  backgroundType = 'grid',
  backgroundColor,
  backgroundImage,
  gridSize = 20,
  size = 'md',
  interactive = true,
  showControls = true,
  editable = false,
  showFullscreen = true,
  maintainAspectRatio,
  aspectRatio,
  onItemsChange,
  onBindingsChange,
  onLayerClick,
  className,
  title,
}: CustomLayerProps & { title?: string }) {
  const { t } = useTranslation('dashboardComponents')

  // Get store for device data and command execution
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
  const sendCommand = useStore(state => state.sendCommand)

  // Data source hook for backward compatibility
  const { data, loading, error } = useDataSource<LayerItem[]>(dataSource, {
    fallback: propItems,
  })

  // Helper functions for device data - use devices directly to get real-time updates
  const getDeviceName = useCallback((deviceId: string) => {
    if (!deviceId) return deviceId
    const device = findDevice(devices, deviceId)
    return device?.name || deviceId
  }, [devices])

  const getDeviceStatus = useCallback((deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
    if (!deviceId) return undefined
    const device = findDevice(devices, deviceId)
    if (!device) return undefined
    return device.online ? 'online' : 'offline'
  }, [devices])

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

  // Convert bindings to layer items - recomputes when bindings or devices change
  // This ensures metric values and device status are updated in real-time
  const bindingsItems = useMemo((): LayerItem[] => {
    if (!bindings || bindings.length === 0) return []

    return bindings.map((binding): LayerItem => {
      const position = binding.position === 'auto' || !binding.position
        ? { x: 50, y: 50 }
        : binding.position

      const ds = binding.dataSource
      const deviceId = ds.id || ds.sourceId || (ds as any).deviceId || (ds.metricId ? ds.metricId.split(':')[0] : undefined)

      const item: LayerItem = {
        id: binding.id,
        type: binding.type || binding.icon || 'text',
        position,
        label: binding.name,
        color: binding.color,
        backgroundColor: binding.backgroundColor,
        fontSize: binding.fontSize,
        fontWeight: binding.fontWeight,
        opacity: binding.opacity,
        markerSize: binding.markerSize,
        visible: true,
        locked: false,
        draggable: true,
      }

      if (binding.type === 'metric') {
        item.deviceId = deviceId
        item.metricId = ds.field || ds.metricId || ds.property
        item.deviceName = getDeviceName(deviceId || '')
        item.metricName = ds.field || ds.metricId || ds.property
        const metricValue = getDeviceMetricValue(deviceId || '', item.metricId || '')
        item.value = metricValue !== undefined ? metricValue : '-'
      } else if (binding.type === 'command') {
        item.command = ds.command
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
      } else if (binding.type === 'device') {
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
        item.status = getDeviceStatus(deviceId || '')
      } else if (binding.type === 'text') {
        item.value = (ds as any)?.text || ''
      } else if (binding.type === 'icon') {
        item.icon = (ds as any)?.icon || ''
      }

      return item
    })
  }, [bindings, getDeviceName, getDeviceStatus, getDeviceMetricValue])
  const sourceItems = error ? propItems : (data ?? propItems)

  // Use useMemo to stabilize items reference and prevent infinite loops
  const items = useMemo(() => {
    if (bindings && bindings.length > 0) {
      return bindingsItems
    }
    if (!dataSource) {
      return propItems
    }
    return sourceItems
  }, [bindings, bindingsItems, dataSource, propItems, sourceItems])

  const [isEditing, setIsEditing] = useState(editable)
  const [selectedItem, setSelectedItem] = useState<string | null>(null)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [fullscreenImage, setFullscreenImage] = useState<{ src: string; deviceName?: string; metricName?: string } | null>(null)

  // Canvas zoom and pan state
  const [zoom, setZoom] = useState(1)
  const [pan, setPan] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 })
  const canvasRef = useRef<HTMLDivElement>(null)

  // Only use internalItems state when NOT using bindings (for dataSource/propItems mode)
  // When using bindings, always use the computed items directly to avoid sync issues
  // Memoize this to prevent unnecessary re-renders
  const useInternalItems = useMemo(() => !(bindings && bindings.length > 0), [bindings])
  const [internalItems, setInternalItems] = useState<LayerItem[]>([])

  // The items to use for rendering - direct from bindings or from internal state
  // Use useMemo to prevent unnecessary re-renders
  const renderItems = useMemo(() => {
    return useInternalItems ? internalItems : items
  }, [useInternalItems, internalItems, items])

  // Sync isEditing when editable prop changes (e.g., after configuration update)
  useEffect(() => {
    setIsEditing(editable)
  }, [editable])

  // Sync items when props change (only for dataSource/propItems mode)
  // Use ref to track previous items and avoid unnecessary updates
  const prevItemsRef = useRef<LayerItem[] | null>(null)

  useEffect(() => {
    if (!useInternalItems) {
      // In bindings mode, clear internal items to free memory
      if (internalItems.length > 0) {
        setInternalItems([])
      }
      prevItemsRef.current = null
      return
    }

    // Only update if items actually changed (compare by reference or content)
    const itemsChanged = prevItemsRef.current !== items &&
                        (prevItemsRef.current === null ||
                         prevItemsRef.current.length !== items.length ||
                         prevItemsRef.current.some((item, i) => item.id !== items[i]?.id))

    if (itemsChanged) {
      setInternalItems(items)
      prevItemsRef.current = items
    }
  }, [items, useInternalItems, internalItems.length])

  // Real-time updates for device/metric bindings (only for dataSource/propItems mode)
  // In bindings mode, items are computed directly from bindings/devices, so no manual update needed
  const prevDeviceIdsRef = useRef<string[]>([])

  useEffect(() => {
    if (!bindings || bindings.length === 0) return
    // Skip this effect in bindings mode - items are computed directly
    if (!useInternalItems) return

    // Check if devices actually changed (by ID, not reference)
    const currentDeviceIds = devices.map(d => d.id)
    const prevDeviceIds = prevDeviceIdsRef.current
    const devicesChanged = currentDeviceIds.length !== prevDeviceIds.length ||
                        currentDeviceIds.some((id, i) => id !== prevDeviceIds[i])

    if (!devicesChanged) {
      // Devices haven't changed, skip update
      // But check if any current_values changed for relevant bindings
      const hasMetricBindings = bindings.some(b => b.type === 'metric')
      const hasDeviceBindings = bindings.some(b => b.type === 'device')

      if (!hasMetricBindings && !hasDeviceBindings) return
    }

    prevDeviceIdsRef.current = currentDeviceIds

    // Update items with fresh data from store
    const updateItemFromDevice = (binding: LayerBinding) => {
      const ds = binding.dataSource
      const deviceId = ds.id || ds.sourceId || (ds as any).deviceId || (ds.metricId ? ds.metricId.split(':')[0] : undefined)

      if (binding.type === 'metric' && deviceId) {
        const metricId = ds.metricId || ds.property
        const device = findDevice(devices, deviceId)
        const cv = deviceTelemetry[deviceId] || device?.current_values
        const metricValue = cv ? findMetricValue(cv, metricId || '') : undefined

        setInternalItems(prev =>
          prev.map(i => {
            if (i.id === binding.id && metricValue !== undefined) {
              return { ...i, value: typeof metricValue === 'number' ? metricValue : String(metricValue) }
            }
            return i
          })
        )
      } else if (binding.type === 'device' && deviceId) {
        const device = findDevice(devices, deviceId)
        const status = device?.online ? 'online' : 'offline'

        setInternalItems(prev =>
          prev.map(i => {
            if (i.id === binding.id) {
              return { ...i, status }
            }
            return i
          })
        )
      }
    }

    // Update each binding
    bindings.forEach(updateItemFromDevice)
  }, [devices, deviceTelemetry, bindings, useInternalItems])

  const sizeConfig = dashboardComponentSize[size]

  // Notify parent of items change (debounced to avoid rapid updates)
  // Only notify in dataSource/propItems mode, not in bindings mode
  const prevRenderItemsRef = useRef<LayerItem[] | null>(null)

  useEffect(() => {
    if (!useInternalItems || !onItemsChange) return

    // Only notify if renderItems actually changed
    const renderItemsChanged = prevRenderItemsRef.current !== renderItems &&
                              (prevRenderItemsRef.current === null ||
                               prevRenderItemsRef.current.length !== renderItems.length ||
                               prevRenderItemsRef.current.some((item, i) => item.id !== renderItems[i]?.id || item.position.x !== renderItems[i]?.position.x || item.position.y !== renderItems[i]?.position.y))

    if (!renderItemsChanged) return

    // Use a small timeout to batch rapid changes
    const timeoutId = setTimeout(() => {
      onItemsChange(renderItems)
      prevRenderItemsRef.current = renderItems
    }, 100)

    return () => clearTimeout(timeoutId)
  }, [renderItems, onItemsChange, useInternalItems])

  // Handle item position drag - also update bindings
  const handleItemDrag = useCallback((item: LayerItem, newPosition: { x: number; y: number }) => {
    // Only update internalItems in dataSource/propItems mode
    if (useInternalItems) {
      setInternalItems(prev =>
        prev.map(i => i.id === item.id ? { ...i, position: newPosition } : i)
      )
    }

    // Always update bindings if present (this is the primary data source in bindings mode)
    if (bindings && onBindingsChange) {
      const updatedBindings = bindings.map(b =>
        b.id === item.id ? { ...b, position: newPosition } : b
      )
      onBindingsChange(updatedBindings)
    }

    // Also notify parent via onItemsChange for bindings mode
    // This allows LayerEditorDialog to receive drag updates
    if (bindings && onItemsChange) {
      const updatedItems = items.map(i =>
        i.id === item.id ? { ...i, position: newPosition } : i
      )
      onItemsChange(updatedItems)
    }
  }, [bindings, onBindingsChange, onItemsChange, useInternalItems, items])

  // Handle toggle visibility
  const handleToggleVisibility = useCallback((id: string) => {
    // Only update internalItems in dataSource/propItems mode
    if (useInternalItems) {
      setInternalItems(prev =>
        prev.map(i => i.id === id ? { ...i, visible: i.visible === false ? undefined : false } : i)
      )
    }
    // Note: bindings don't have visible field, this is only for dataSource/propItems mode
  }, [useInternalItems])

  // Handle toggle lock
  const handleToggleLock = useCallback((id: string) => {
    // Only update internalItems in dataSource/propItems mode
    if (useInternalItems) {
      setInternalItems(prev =>
        prev.map(i => i.id === id ? { ...i, locked: !i.locked } : i)
      )
    }
  }, [useInternalItems])

  // Handle command execution
  const handleExecuteCommand = useCallback(async (deviceId: string, command: string) => {
    try {
      await sendCommand(deviceId, command)
    } catch (error) {
      console.error('Failed to execute command:', error)
    }
  }, [sendCommand])

  const handleAddItem = useCallback(() => {
    // Only work in dataSource/propItems mode - in bindings mode use LayerEditorDialog
    if (!useInternalItems) return

    const newItem: LayerItem = {
      id: `item-${Date.now()}`,
      type: 'text',
      position: { x: 50, y: 50 },
      label: 'New Item',
      value: '0',
      visible: true,
      locked: false,
      draggable: true,
    }
    setInternalItems(prev => [...prev, newItem])
    setSelectedItem(newItem.id)
  }, [useInternalItems])

  // Zoom handlers
  const handleZoomIn = useCallback(() => {
    setZoom(prev => Math.min(prev + 0.25, 3))
  }, [])

  const handleZoomOut = useCallback(() => {
    setZoom(prev => Math.max(prev - 0.25, 0.25))
  }, [])

  const handleResetZoom = useCallback(() => {
    setZoom(1)
    setPan({ x: 0, y: 0 })
  }, [])

  // Pan handlers
  const handleCanvasMouseDown = useCallback((e: React.MouseEvent) => {
    // Left-click on canvas background for panning
    // Items will stop propagation so they can be dragged independently
    if (e.button === 0) {
      setIsDragging(true)
      setDragStart({ x: e.clientX - pan.x, y: e.clientY - pan.y })
    }
  }, [pan])

  useEffect(() => {
    if (!isDragging) return

    const handleMouseMove = (e: MouseEvent) => {
      const newPan = {
        x: e.clientX - dragStart.x,
        y: e.clientY - dragStart.y,
      }
      setPan(newPan)
    }

    const handleMouseUp = () => {
      setIsDragging(false)
    }

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [isDragging, dragStart])

  // Loading state - only show skeleton before the layer has anything to draw.
  // Refreshes should keep the current items visible to avoid dashboard flicker.
  const showLoading = loading && renderItems.length === 0
  if (showLoading) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full rounded-lg')} />
      </div>
    )
  }

  const content = (
    <div className={cn(dashboardCardBase, 'flex flex-col overflow-hidden', className)}>
      {/* Header with controls */}
      {showControls && (
        <div className="flex items-center justify-between px-3 py-2 border-b bg-muted-20 shrink-0">
          <div className="flex items-center gap-2">
            <Layers className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">{title || 'Custom Layer'}</span>
          </div>
          <div className="flex items-center gap-1">
            {/* Zoom controls */}
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={handleZoomOut}
              title={t('customLayer.zoomOut')}
            >
              <ZoomOut className="h-4 w-4" />
            </Button>
            <span className="text-xs text-muted-foreground w-12 text-center">{Math.round(zoom * 100)}%</span>
            <Button
              variant="ghost"
              size="icon"
              className="h-6 w-6"
              onClick={handleZoomIn}
              title={t('customLayer.zoomIn')}
            >
              <ZoomIn className="h-4 w-4" />
            </Button>
            {(zoom !== 1 || pan.x !== 0 || pan.y !== 0) && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={handleResetZoom}
                title={t('customLayer.reset')}
              >
                <RotateCcw className="h-4 w-4" />
              </Button>
            )}
            <div className="w-px h-4 bg-border mx-1" />
            {editable && (
              <Button
                variant="ghost"
                size="icon"
                className={cn('h-6 w-6', isEditing && 'bg-accent')}
                onClick={() => setIsEditing(!isEditing)}
              >
                <Settings className="h-4 w-4" />
              </Button>
            )}
            {isEditing && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={handleAddItem}
              >
                <Plus className="h-4 w-4" />
              </Button>
            )}
            {showFullscreen && (
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6"
                onClick={() => setIsFullscreen(!isFullscreen)}
              >
                {isFullscreen ? <Minimize2 className="h-4 w-4" /> : <Maximize2 className="h-4 w-4" />}
              </Button>
            )}
          </div>
        </div>
      )}

      {/* Layer canvas - fill remaining space */}
      <div
        ref={canvasRef}
        className="relative flex-1 min-h-0 overflow-hidden cursor-crosshair"
        onMouseDown={handleCanvasMouseDown}
      >
        {/* Transform container for zoom and pan - includes background and items */}
        <div
          className="absolute inset-0"
          style={{
            transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
            transformOrigin: '0 0',
            transition: isDragging ? 'none' : 'transform 150ms ease-out',
          }}
        >
          {/* Grid background - scales with items */}
          <div
            className="absolute inset-0 -z-10"
            style={getBackgroundStyle(backgroundType, backgroundColor, backgroundImage, gridSize, maintainAspectRatio, aspectRatio)}
          />

          {/* Items container */}
          <div className="relative w-full h-full">
            {/* Render items */}
            {renderItems.map((item) => (
              <LayerItemComponent
                key={item.id}
                item={item}
                isEditing={isEditing}
                isSelected={selectedItem === item.id}
                onSelect={() => setSelectedItem(item.id)}
                onDrag={handleItemDrag}
                onToggleVisibility={handleToggleVisibility}
                onToggleLock={handleToggleLock}
                onExecuteCommand={handleExecuteCommand}
                onImageClick={setFullscreenImage}
                t={t}
              />
            ))}
          </div>
        </div>

        {/* Edit mode indicator */}
        {isEditing && (
          <div className="absolute top-2 left-2 px-2 py-1 bg-accent text-accent-foreground rounded text-xs font-medium z-50">
            {t('customLayer.editMode')}
          </div>
        )}

        {/* Zoom indicator */}
        {(zoom !== 1 || pan.x !== 0 || pan.y !== 0) && (
          <div className="absolute bottom-2 right-2 px-2 py-1 bg-overlay-medium text-white rounded text-xs z-50">
            {Math.round(zoom * 100)}% | {Math.round(pan.x)}, {Math.round(pan.y)}
          </div>
        )}
      </div>
    </div>
  )

  // Fullscreen overlay (rendered via Portal to document.body)
  const fullscreenOverlay = isFullscreen ? createPortal(
    <div className="fixed inset-0 z-[110] flex flex-col bg-background">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b bg-bg-95">
        <div className="flex items-center gap-2">
          <Layers className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium">Custom Layer</span>
        </div>
        <div className="flex items-center gap-1">
          {/* Zoom controls */}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={handleZoomOut}
            title={t('customLayer.zoomOut')}
          >
            <ZoomOut className="h-4 w-4" />
          </Button>
          <span className="text-xs text-muted-foreground w-12 text-center">{Math.round(zoom * 100)}%</span>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={handleZoomIn}
            title={t('customLayer.zoomIn')}
          >
            <ZoomIn className="h-4 w-4" />
          </Button>
          {(zoom !== 1 || pan.x !== 0 || pan.y !== 0) && (
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8"
              onClick={handleResetZoom}
              title={t('customLayer.reset')}
            >
              <RotateCcw className="h-4 w-4" />
            </Button>
          )}
          <div className="w-px h-5 bg-border mx-1" />
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => setIsFullscreen(false)}
            title={t('customLayer.exitFullscreen')}
          >
            <Minimize2 className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {/* Fullscreen canvas */}
      <div className="flex-1 relative overflow-hidden">
        <div
          ref={canvasRef}
          className="absolute inset-0 cursor-crosshair"
          onMouseDown={handleCanvasMouseDown}
        >
          {/* Transform container for zoom and pan - includes background and items */}
          <div
            className="absolute inset-0"
            style={{
              transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})`,
              transformOrigin: '0 0',
              transition: isDragging ? 'none' : 'transform 150ms ease-out',
            }}
          >
            {/* Grid background - scales with items */}
            <div
              className="absolute inset-0 -z-10"
              style={getBackgroundStyle(backgroundType, backgroundColor, backgroundImage, gridSize, maintainAspectRatio, aspectRatio)}
            />

            {/* Items container */}
            <div className="relative w-full h-full">
              {/* Render items */}
              {renderItems.map((item) => (
                <LayerItemComponent
                  key={item.id}
                  item={item}
                  isEditing={isEditing}
                  isSelected={selectedItem === item.id}
                  onSelect={() => setSelectedItem(item.id)}
                  onDrag={handleItemDrag}
                  onToggleVisibility={handleToggleVisibility}
                  onToggleLock={handleToggleLock}
                  onExecuteCommand={handleExecuteCommand}
                  t={t}
                />
              ))}
            </div>
          </div>

          {/* Edit mode indicator */}
          {isEditing && (
            <div className="absolute top-2 left-2 px-2 py-1 bg-accent text-accent-foreground rounded text-xs font-medium z-50">
              {t('customLayer.editMode')}
            </div>
          )}

          {/* Zoom indicator */}
          {(zoom !== 1 || pan.x !== 0 || pan.y !== 0) && (
            <div className="absolute bottom-2 right-2 px-2 py-1 bg-overlay-medium text-white rounded text-xs z-50">
              {Math.round(zoom * 100)}% | {Math.round(pan.x)}, {Math.round(pan.y)}
            </div>
          )}
        </div>
      </div>
    </div>, getPortalRoot()
  ) : null

  return (
    <>
      {/* Normal view (hidden when fullscreen to avoid dual rendering and ref conflicts) */}
      {!isFullscreen && content}
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

// Export the editor dialog
export { LayerEditorDialog } from './LayerEditorDialog'

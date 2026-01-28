/**
 * Custom Layer Component
 *
 * A free-form container component that can hold other components.
 * Supports placing devices, metrics, commands, and other content at custom positions.
 * Can be used as a background layer with overlaid interactive elements.
 * Data binding support similar to Map component with typed bindings.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
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
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { useStore } from '@/store'

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
}

// Type config for styling similar to MapDisplay
const TYPE_CONFIG = {
  device: {
    icon: MapPin,
    color: 'text-green-600 dark:text-green-400',
    bgColor: 'bg-green-500/10 dark:bg-green-500/20',
    borderColor: 'border-green-500/30',
  },
  metric: {
    icon: Activity,
    color: 'text-purple-600 dark:text-purple-400',
    bgColor: 'bg-purple-500/10 dark:bg-purple-500/20',
    borderColor: 'border-purple-500/30',
  },
  command: {
    icon: Zap,
    color: 'text-blue-600 dark:text-blue-400',
    bgColor: 'bg-blue-500/10 dark:bg-blue-500/20',
    borderColor: 'border-blue-500/30',
  },
  text: {
    icon: Type,
    color: 'text-foreground',
    bgColor: 'bg-muted/50',
    borderColor: 'border-border',
  },
  icon: {
    icon: Sparkles,
    color: 'text-orange-600 dark:text-orange-400',
    bgColor: 'bg-orange-500/10 dark:bg-orange-500/20',
    borderColor: 'border-orange-500/30',
  },
  component: {
    icon: Layers,
    color: 'text-cyan-600 dark:text-cyan-400',
    bgColor: 'bg-cyan-500/10 dark:bg-cyan-500/20',
    borderColor: 'border-cyan-500/30',
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
}: LayerItemComponentProps) {
  const [isDragging, setIsDragging] = useState(false)
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 })
  const [position, setPosition] = useState(item.position)
  const itemRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    setPosition(item.position)
  }, [item.position])

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    if (!isEditing || item.locked || !item.draggable) return
    e.stopPropagation()
    setIsDragging(true)
    setDragStart({ x: e.clientX - position.x, y: e.clientY - position.y })
    onSelect()
  }, [isEditing, item.locked, item.draggable, position, onSelect])

  useEffect(() => {
    if (!isDragging) return

    const handleMouseMove = (e: MouseEvent) => {
      // Convert pixel delta to percentage
      if (itemRef.current?.parentElement) {
        const parent = itemRef.current.parentElement
        const newX = ((e.clientX - dragStart.x) / parent.offsetWidth) * 100
        const newY = ((e.clientY - dragStart.y) / parent.offsetHeight) * 100
        const clampedX = Math.max(0, Math.min(95, newX))
        const clampedY = Math.max(0, Math.min(95, newY))
        setPosition({ x: clampedX, y: clampedY })
      }
    }

    const handleMouseUp = () => {
      setIsDragging(false)
      onDrag(item, position)
    }

    document.addEventListener('mousemove', handleMouseMove)
    document.addEventListener('mouseup', handleMouseUp)

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [isDragging, dragStart, item, position, onDrag])

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
      case 'online': return 'bg-green-500'
      case 'offline': return 'bg-gray-400'
      case 'error': return 'bg-red-500'
      case 'warning': return 'bg-yellow-500'
      default: return null
    }
  }

  return (
    <div
      ref={itemRef}
      className={cn(
        'absolute transform -translate-x-1/2 -translate-y-1/2',
        isEditing && !item.locked && 'cursor-move',
        isDragging && 'cursor-grabbing',
        isSelected && 'ring-2 ring-primary ring-offset-2'
      )}
      style={{
        left: `${position.x}%`,
        top: `${position.y}%`,
        width: item.size?.width ? `${item.size.width}px` : 'auto',
        height: item.size?.height ? `${item.size.height}px` : 'auto',
      }}
      onMouseDown={handleMouseDown}
      onClick={(e) => {
        if (!isDragging && item.onClick) {
          e.stopPropagation()
          item.onClick()
        }
      }}
    >
      {/* Content based on type */}
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
        }}
      >
        {/* Icon based on type */}
        {item.type === 'device' && (
          <Icon className="h-3.5 w-3.5 flex-shrink-0" />
        )}
        {item.type === 'metric' && (
          <Icon className="h-3.5 w-3.5 flex-shrink-0" />
        )}
        {item.type === 'command' && (
          <Icon className="h-3.5 w-3.5 flex-shrink-0" />
        )}
        {item.type === 'text' && !item.icon && (
          <Icon className="h-3.5 w-3.5 flex-shrink-0 text-muted-foreground" />
        )}
        {item.type === 'icon' && !item.icon && (
          <Icon className="h-3.5 w-3.5 flex-shrink-0" />
        )}

        {/* Custom icon from prop */}
        {item.icon && item.type !== 'device' && item.type !== 'metric' && item.type !== 'command' && (
          <span className="flex-shrink-0">{item.icon}</span>
        )}

        {/* Label */}
        {item.label && (
          <span className={cn(
            item.type === 'device' && 'font-medium'
          )}>
            {item.label}
          </span>
        )}

        {/* Value for metrics */}
        {item.value !== undefined && item.type === 'metric' && (
          <span className={cn(
            'tabular-nums font-semibold',
            typeConfig.color
          )}>
            {item.value}
          </span>
        )}

        {/* Value for other types */}
        {item.value !== undefined && item.type !== 'metric' && (
          <span className="tabular-nums">
            {item.value}
          </span>
        )}

        {/* Status indicator for devices */}
        {item.type === 'device' && item.status && (
          <span className={cn(
            'w-2 h-2 rounded-full flex-shrink-0',
            getStatusColor()
          )} />
        )}

        {/* Command button for command type */}
        {item.type === 'command' && !isEditing && (
          <Button
            size="sm"
            className="h-5 px-1.5 text-xs bg-blue-500 hover:bg-blue-600 text-white"
            onClick={async (e) => {
              e.stopPropagation()
              if (onExecuteCommand && item.deviceId && item.command) {
                await onExecuteCommand(item.deviceId, item.command)
              }
            }}
          >
            执行
          </Button>
        )}
      </div>

      {/* Edit controls */}
      {isEditing && isSelected && (
        <div className="absolute -top-6 left-1/2 -translate-x-1/2 flex items-center gap-1">
          <Button
            variant="secondary"
            size="icon"
            className="h-5 w-5 bg-background/90"
            onClick={(e) => { e.stopPropagation(); onToggleVisibility(item.id); }}
            title={(item.visible as boolean | undefined) === false ? '显示' : '隐藏'}
          >
            {(item.visible as boolean | undefined) === false ? <EyeOff className="h-2.5 w-2.5" /> : <Eye className="h-2.5 w-2.5" />}
          </Button>
          <Button
            variant="secondary"
            size="icon"
            className="h-5 w-5 bg-background/90"
            onClick={(e) => { e.stopPropagation(); onToggleLock(item.id); }}
          >
            {item.locked ? <Lock className="h-2.5 w-2.5" /> : <Unlock className="h-2.5 w-2.5" />}
          </Button>
        </div>
      )}

      {/* Drag handle */}
      {isEditing && (item.locked !== true) && (
        <div className="absolute -left-6 top-1/2 -translate-y-1/2 opacity-0 hover:opacity-100 group-hover:opacity-100">
          <GripVertical className="h-4 w-4 text-muted-foreground" />
        </div>
      )}
    </div>
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
      backgroundStyle.backgroundColor = color || 'hsl(var(--muted) / 0.3)'
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
        linear-gradient(45deg, hsl(var(--muted) / 0.1) 25%, transparent 25%),
        linear-gradient(-45deg, hsl(var(--muted) / 0.1) 25%, transparent 25%),
        linear-gradient(45deg, transparent 75%, hsl(var(--muted) / 0.1) 75%),
        linear-gradient(-45deg, transparent 75%, hsl(var(--muted) / 0.1) 75%)
      `
      backgroundStyle.backgroundSize = '20px 20px'
      backgroundStyle.backgroundPosition = '0 0, 0 10px, 10px -10px, -10px 0px'
      break
    case 'grid':
      backgroundStyle.backgroundColor = color || 'hsl(var(--background))'
      backgroundStyle.backgroundImage = `
        linear-gradient(to right, hsl(var(--border)) 1px, transparent 1px),
        linear-gradient(to bottom, hsl(var(--border)) 1px, transparent 1px)
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
}: CustomLayerProps) {
  // Get store for device data and command execution
  const devices = useStore(state => state.devices)
  const sendCommand = useStore(state => state.sendCommand)

  // Store devices in a ref to avoid unnecessary recalculations
  // Only update when devices actually change (by content, not reference)
  const devicesRef = useRef<typeof devices>([])

  // Use a separate ref to track device IDs for comparison
  const deviceIdsRef = useRef<string[]>([])
  const currentDeviceIds = devices.map(d => d.id)

  // Only update devicesRef if device IDs actually changed
  const deviceIdsChanged = currentDeviceIds.length !== deviceIdsRef.current.length ||
                      currentDeviceIds.some((id, i) => id !== deviceIdsRef.current[i])

  if (deviceIdsChanged) {
    devicesRef.current = devices
    deviceIdsRef.current = currentDeviceIds
  }

  // Data source hook for backward compatibility
  const { data, loading, error } = useDataSource<LayerItem[]>(dataSource, {
    fallback: propItems,
  })

  // Helper functions for device data (use ref to avoid dependency on devices)
  const getDeviceName = useCallback((deviceId: string) => {
    const device = devicesRef.current.find(d => d.id === deviceId)
    return device?.name || deviceId
  }, [])

  const getDeviceStatus = useCallback((deviceId: string): 'online' | 'offline' | 'error' | 'warning' | undefined => {
    const device = devicesRef.current.find(d => d.id === deviceId)
    if (!device) return undefined
    return device.online ? 'online' : 'offline'
  }, [])

  const getDeviceMetricValue = useCallback((deviceId: string, metricId: string): string | number | undefined => {
    const device = devicesRef.current.find(d => d.id === deviceId)
    if (!device?.current_values) return undefined
    const value = device.current_values[metricId]
    if (value !== undefined && value !== null) {
      return typeof value === 'number' ? value : String(value)
    }
    return undefined
  }, [])

  // Convert bindings to layer items - only depends on bindings, not devices
  const bindingsItems = useMemo((): LayerItem[] => {
    if (!bindings || bindings.length === 0) return []

    return bindings.map((binding): LayerItem => {
      const position = binding.position === 'auto' || !binding.position
        ? { x: 50, y: 50 }
        : binding.position

      const ds = binding.dataSource as any
      const deviceId = ds?.deviceId

      const item: LayerItem = {
        id: binding.id,
        type: binding.type || binding.icon || 'text',
        position,
        label: binding.name,
        color: binding.color,
        backgroundColor: binding.backgroundColor,
        fontSize: binding.fontSize,
        fontWeight: binding.fontWeight,
        visible: true,
        locked: false,
        draggable: true,
      }

      if (binding.type === 'metric') {
        item.deviceId = deviceId
        item.metricId = ds?.metricId || ds?.property
        item.deviceName = getDeviceName(deviceId || '')
        item.metricName = ds?.metricId || ds?.property
        const metricValue = getDeviceMetricValue(deviceId || '', item.metricId || '')
        item.value = metricValue !== undefined ? metricValue : '--'
      } else if (binding.type === 'command') {
        item.command = ds?.command
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
      } else if (binding.type === 'device') {
        item.deviceId = deviceId
        item.deviceName = getDeviceName(deviceId || '')
        item.status = getDeviceStatus(deviceId || '')
      } else if (binding.type === 'text') {
        item.value = ds?.text || ''
      } else if (binding.type === 'icon') {
        item.icon = ds?.icon || ''
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
      const ds = binding.dataSource as any
      const deviceId = ds?.deviceId

      if (binding.type === 'metric' && deviceId) {
        const metricId = ds?.metricId || ds?.property
        const device = devices.find(d => d.id === deviceId)
        const metricValue = device?.current_values?.[metricId || '']

        setInternalItems(prev =>
          prev.map(i => {
            if (i.id === binding.id && metricValue !== undefined) {
              return { ...i, value: typeof metricValue === 'number' ? metricValue : String(metricValue) }
            }
            return i
          })
        )
      } else if (binding.type === 'device' && deviceId) {
        const device = devices.find(d => d.id === deviceId)
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
  }, [devices, bindings, useInternalItems])

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
  }, [bindings, onBindingsChange, useInternalItems])

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

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full rounded-lg')} />
      </div>
    )
  }

  const content = (
    <div className={cn(dashboardCardBase, 'relative overflow-hidden', className)}>
      {/* Header with controls */}
      {showControls && (
        <div className="flex items-center justify-between px-3 py-2 border-b bg-muted/20">
          <div className="flex items-center gap-2">
            <Layers className="h-4 w-4 text-muted-foreground" />
            <span className="text-sm font-medium">Custom Layer</span>
            <span className="text-xs text-muted-foreground">({renderItems.length} items)</span>
          </div>
          <div className="flex items-center gap-1">
            {editable && (
              <Button
                variant="ghost"
                size="icon"
                className={cn('h-7 w-7', isEditing && 'bg-accent')}
                onClick={() => setIsEditing(!isEditing)}
              >
                <Settings className="h-3.5 w-3.5" />
              </Button>
            )}
            {isEditing && (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleAddItem}
              >
                <Plus className="h-3.5 w-3.5" />
              </Button>
            )}
            {showFullscreen && (
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={() => setIsFullscreen(!isFullscreen)}
              >
                {isFullscreen ? <Minimize2 className="h-3.5 w-3.5" /> : <Maximize2 className="h-3.5 w-3.5" />}
              </Button>
            )}
          </div>
        </div>
      )}

      {/* Layer canvas */}
      <div
        className={cn(
          'relative overflow-hidden',
          isFullscreen ? 'fixed inset-0 z-50' : 'min-h-[300px]'
        )}
        style={getBackgroundStyle(backgroundType, backgroundColor, backgroundImage, gridSize, maintainAspectRatio, aspectRatio)}
        onClick={(e) => {
          // Clear selection
          setSelectedItem(null)

          // Handle layer click for positioning
          if (onLayerClick && e.currentTarget) {
            const rect = e.currentTarget.getBoundingClientRect()
            const x = ((e.clientX - rect.left) / rect.width) * 100
            const y = ((e.clientY - rect.top) / rect.height) * 100
            onLayerClick(x, y)
          }
        }}
      >
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
          />
        ))}

        {/* Empty state */}
        {renderItems.length === 0 && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground/60">
            <Square className="h-12 w-12" />
            <div className="text-center">
              <p className="text-sm">Empty Layer</p>
              <p className="text-xs mt-1">Add items to this layer</p>
            </div>
            {isEditing && (
              <Button variant="outline" size="sm" onClick={handleAddItem}>
                <Plus className="h-3.5 w-3.5 mr-1.5" />
                Add Item
              </Button>
            )}
          </div>
        )}

        {/* Edit mode indicator */}
        {isEditing && (
          <div className="absolute top-2 left-2 px-2 py-1 bg-accent text-accent-foreground rounded text-xs font-medium">
            Edit Mode
          </div>
        )}
      </div>
    </div>
  )

  return content
}

// Export the editor dialog
export { LayerEditorDialog } from './LayerEditorDialog'

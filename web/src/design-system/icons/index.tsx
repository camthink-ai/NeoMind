/**
 * Design System - Icons
 *
 * Unified icon system using lucide-react.
 * Replaces emoji icons with accessible SVG icons.
 */

import {
  // Indicators
  Thermometer,
  Droplets,
  Gauge,
  Battery,
  Lightbulb,
  Power,
  Activity,
  TrendingUp,
  TrendingDown,
  Minus,

  // Device states
  Wifi,
  WifiOff,
  AlertTriangle,
  CheckCircle2,
  XCircle,
  Clock,

  // Controls
  ToggleLeft,
  ToggleRight,

  // Charts
  BarChart3,
  PieChart as PieChartIcon,
  LineChart as LineChartIcon,
  Donut,
  Radar,

  // Misc
  MoreVertical,
  Settings2,
  Copy,
  Trash2,
  Plus,
  Home,
  Zap,
  Fan,
  Lock,
  Unlock,
  DoorOpen,
  DoorClosed,
  Eye,
  EyeOff,
  Volume2,
  Sun,
  Moon,
  Cloud,
  CloudRain,
  Wind,
  MapPin,
  Calendar,
  List,
  Table,
} from 'lucide-react'

// Icon mapping for common entity types
export const entityIcons = {
  // Temperature
  temperature: Thermometer,
  temp: Thermometer,
  thermometer: Thermometer,

  // Humidity
  humidity: Droplets,
  hygro: Droplets,
  moisture: Droplets,

  // Pressure
  pressure: Gauge,
  barometer: Gauge,

  // Energy/Power
  battery: Battery,
  power: Power,
  energy: Zap,
  electricity: Zap,

  // Light
  light: Lightbulb,
  lamp: Lightbulb,
  bulb: Lightbulb,

  // Motion
  motion: Activity,
  presence: Activity,
  occupancy: Activity,

  // Door/Window
  door: DoorOpen,
  window: Minus, // Or use a custom icon
  garage: Home,

  // Lock
  lock: Lock,
  unlock: Unlock,

  // Fan
  fan: Fan,

  // Volume/Brightness
  volume: Volume2,
  brightness: Sun,

  // Weather
  sun: Sun,
  moon: Moon,
  rain: CloudRain,
  wind: Wind,
  cloud: Cloud,

  // Location
  location: MapPin,
  gps: MapPin,

  // Status
  online: Wifi,
  offline: WifiOff,
  error: XCircle,
  warning: AlertTriangle,
  success: CheckCircle2,
  unknown: Minus,

  // Trend
  trendUp: TrendingUp,
  trendDown: TrendingDown,
  trendNeutral: Minus,

  // Time
  time: Clock,
  date: Calendar,

  // Data
  data: Table,
  list: List,

  // Chart
  chart: BarChart3,
  lineChart: LineChartIcon,
  barChart: BarChart3,
  pieChart: PieChartIcon,
  radarChart: Radar,
} as const

export type EntityIcon = keyof typeof entityIcons

// Default icon fallback
export const DefaultIcon = Activity

// Helper to get icon for entity type
export function getIconForEntity(type: string): React.ComponentType<{ className?: string; style?: React.CSSProperties }> {
  const key = type.toLowerCase() as EntityIcon
  return (entityIcons[key] || DefaultIcon) as React.ComponentType<{ className?: string; style?: React.CSSProperties }>
}

// Helper component for rendering entity icons
export interface EntityIconProps {
  type?: string
  className?: string
  size?: number
}

export function EntityIcon({ type, className, size = 24 }: EntityIconProps) {
  if (!type) {
    return (
      <div className={className} style={{ width: size, height: size, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <Activity style={{ width: size, height: size }} />
      </div>
    )
  }

  const IconComponent = getIconForEntity(type)

  return (
    <div className={className} style={{ width: size, height: size, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      <IconComponent style={{ width: size, height: size }} />
    </div>
  )
}

// Status icons
export const statusIcons = {
  online: { icon: Wifi, color: 'text-green-500' },
  offline: { icon: WifiOff, color: 'text-gray-400' },
  error: { icon: XCircle, color: 'text-red-500' },
  warning: { icon: AlertTriangle, color: 'text-yellow-500' },
  success: { icon: CheckCircle2, color: 'text-green-500' },
  loading: { icon: Clock, color: 'text-blue-500 animate-spin' },
  unknown: { icon: Minus, color: 'text-gray-400' },
} as const

export type StatusIconName = keyof typeof statusIcons

// Action icons
export const actionIcons = {
  settings: Settings2,
  duplicate: Copy,
  delete: Trash2,
  add: Plus,
  more: MoreVertical,
  toggleOn: ToggleRight,
  toggleOff: ToggleLeft,
} as const

export type ActionIconName = keyof typeof actionIcons

// Re-export all lucide icons for convenience
export {
  Thermometer,
  Droplets,
  Gauge,
  Battery,
  Lightbulb,
  Power,
  Activity,
  TrendingUp,
  TrendingDown,
  Minus,
  Wifi,
  WifiOff,
  AlertTriangle,
  CheckCircle2,
  XCircle,
  Clock,
  ToggleLeft,
  ToggleRight,
  BarChart3,
  PieChartIcon,
  LineChartIcon,
  Donut,
  Radar,
  MoreVertical,
  Settings2,
  Copy,
  Trash2,
  Plus,
  Home,
  Zap,
  Fan,
  Lock,
  Unlock,
  DoorOpen,
  DoorClosed,
  Volume2,
  Sun,
  Moon,
  CloudRain,
  Wind,
  MapPin,
  Calendar,
  List,
  Table,
}

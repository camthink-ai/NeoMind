import { type ClassValue, clsx } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// ============================================================================
// Re-export design system utilities for convenience
// ============================================================================

// Format utilities
export {
  formatNumber,
  formatPercentage,
  formatBytes,
  formatDuration,
  formatRelativeTime,
  toDisplayValue,
  formatValue,
  getColorForValue,
  getStatusColorClass,
  getStatusBgClass,
  toNumberArray,
  getLastN,
  getArrayStats,
  isEmpty,
  isValidNumber,
  clamp,
  normalize,
} from '@/design-system/utils/format'

// Icon utilities
export { getIconForEntity } from '@/design-system/icons'
export { EntityIcon } from '@/design-system/icons'

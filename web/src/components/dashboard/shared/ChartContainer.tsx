/**
 * Chart Container Component
 *
 * Pure CSS approach: the container fills available space via flex,
 * and children use absolute positioning to get explicit dimensions.
 */

import { type ReactNode } from 'react'

export function ChartContainer({ children }: { children: ReactNode }) {
  return (
    <div className="relative w-full flex-1 min-h-0">
      <div className="absolute inset-0">
        {children}
      </div>
    </div>
  )
}

/**
 * Hook to get chart container dimensions.
 *
 * Staggers initial measurement across animation frames to prevent
 * all charts from rendering SVG in the same frame (which causes
 * 6000ms+ frame spikes in WKWebView/Tauri).
 *
 * Each chart instance gets a turn: 0, 1, 2, ...
 * Chart 0 measures in the next RAF, chart 1 in the one after, etc.
 * This spreads SVG rendering across multiple frames.
 */

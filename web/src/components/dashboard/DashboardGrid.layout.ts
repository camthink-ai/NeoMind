// Mobile layout computation — split from DashboardGrid.tsx (react-refresh
// requires component files to export only components). Pure, unit-tested.

import { type Layout, type LayoutItem } from 'react-grid-layout'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'

export const MOBILE_COLS = 1

/**
 * Per-type minimum mobile height (in mobile row units, where 1 unit = 60px).
 * Prevents content from being squished: charts get h≥3 for axis room,
 * value-cards get h≥2 for title+value+label, etc.
 */
const MOBILE_MIN_H: Record<string, number> = {
  'value-card': 2,
  'led-indicator': 2,
  'sparkline': 2,
  'progress-bar': 2,
  'toggle-switch': 1,
  'image-display': 2,
  'markdown-display': 3,
  'ai-analyst': 3,
  'line-chart': 3, 'area-chart': 3, 'bar-chart': 3, 'pie-chart': 3,
  'map-display': 3, 'video-display': 3, 'web-display': 3, 'custom-layer': 3,
  'image-history': 3, 'agent-monitor-widget': 4,
}

/**
 * Build the mobile (xs) layout from the desktop component list.
 *
 * Every card becomes a full-width (w=1) row, stacked vertically. Authored
 * height is kept but floored by the per-type minimum.
 *
 * Exported for unit testing — do not call directly; DashboardGrid wires it
 * into the per-breakpoint `layouts` object.
 */
export function buildMobileLayout(
  components: Array<{ id: string; type?: string; position: { w?: number; h?: number } }>,
): Layout {
  return components.map((c) => {
    const constraints = c.type ? COMPONENT_SIZE_CONSTRAINTS[c.type as keyof typeof COMPONENT_SIZE_CONSTRAINTS] : undefined
    const authoredH = c.position.h ?? constraints?.defaultH ?? 2
    const typeMinH = (c.type ? MOBILE_MIN_H[c.type] : undefined) ?? 1
    const h = Math.max(authoredH, typeMinH, 1)

    const item: LayoutItem = {
      i: c.id,
      x: 0,
      y: 0,
      w: MOBILE_COLS,
      h,
      minW: 1,
      minH: 1,
      static: false,
    }
    return item
  })
}

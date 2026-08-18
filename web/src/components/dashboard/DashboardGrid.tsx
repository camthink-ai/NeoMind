/**
 * Dashboard Grid Component
 *
 * Provides a drag-and-drop grid layout using react-grid-layout v2.
 *
 * Layout Strategy:
 * - `layouts` always reflects component positions from props
 * - react-grid-layout is re-driven via `layouts` whenever container width changes
 * - This ensures sidebar toggle / resize always produces correct layout
 * - Positions synced to parent ONLY on drag/resize stop
 */

import { useRef, useState, useEffect, useCallback, useMemo } from 'react'
import { ReactElement } from 'react'
import { ResponsiveGridLayout, type Layout, type LayoutItem } from 'react-grid-layout'
import { cn } from '@/lib/utils'
import { useIsMobile } from '@/hooks/useMobile'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'

import 'react-grid-layout/css/styles.css'
import 'react-resizable/css/styles.css'

const isTouchDevice = () => {
  if (typeof window === 'undefined') return false
  return 'ontouchstart' in window || navigator.maxTouchPoints > 0
}

// ── Mobile responsive layout ──────────────────────────────────────────────
// Phones (<768px) render as a single full-width column (masonry-style). This
// is the Home Assistant pattern and the most predictable mobile UX: no matter
// how the user authored the desktop grid, every card stacks vertically at
// full viewport width.
//
// We tried a 2-column "small vs large" classification first but it was
// unreliable — RGL's compactor didn't reliably pair w=1 cards side-by-side,
// and the right column ended up empty. Single-column sidesteps the whole
// classification problem.
//
// The authored height is kept, floored by a per-type MOBILE_MIN_H so a chart
// the user pinned to h=2 on desktop still gets enough vertical room for its
// axes when it becomes full-width on mobile.
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

export interface DashboardGridProps {
  components: Array<{
    id: string
    position: { x: number; y: number; w: number; h: number; minW?: number; minH?: number; maxW?: number; maxH?: number }
    children?: ReactElement
    render?: () => ReactElement
  }>
  rowHeight?: number
  margin?: [number, number]
  containerPadding?: [number, number]
  breakpoints?: Record<string, number>
  cols?: Record<string, number>
  editMode?: boolean
  onLayoutChange?: (layout: readonly any[]) => void
  className?: string
}

export function DashboardGrid({
  components,
  rowHeight = 60,
  margin = [4, 4],
  containerPadding = [4, 4],
  // xs: 0 so every width below the sm (768) floor lands in xs and gets the
  // 2-column mobile layout. Without this, very narrow phones (360–480px)
  // had no matching breakpoint and RGL fell back to the largest one (lg),
  // dropping them onto the 12-col desktop layout.
  breakpoints = { lg: 1200, md: 996, sm: 768, xs: 0 },
  // xs: 2 — the "双列 + 智能重排" mobile grid. lg/md/sm keep their previous
  // column counts (12/10/6) so desktop authoring is unchanged.
  cols = { lg: 12, md: 10, sm: 6, xs: MOBILE_COLS },
  editMode = false,
  onLayoutChange,
  className,
}: DashboardGridProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const isMobile = useIsMobile()
  const touchEnabled = isTouchDevice()

  // Track active drag/resize to prevent layout reset during interaction
  const isInteractingRef = useRef(false)

  // Component ID string — changes when dashboard switches (different widgets)
  const componentIdKey = useMemo(() => components.map(c => c.id).join(','), [components])

  // Track container width via ResizeObserver
  // Skip updates during drag/resize to prevent layout reset ("jump" bug)
  const widthRef = useRef(0)
  const updateWidth = useCallback(() => {
    if (containerRef.current && !isInteractingRef.current) {
      const w = containerRef.current.offsetWidth
      widthRef.current = w
      setWidth(w)
    }
  }, [])

  // Re-observe when componentIdKey changes (dashboard switch) so width is
  // re-measured immediately, preventing the "grid disappears" bug caused by
  // stale width=0 after ResponsiveGridLayout remounts via key={componentIdKey}.
  useEffect(() => {
    updateWidth()
    const resizeObserver = new ResizeObserver(() => {
      requestAnimationFrame(updateWidth)
    })
    if (containerRef.current) resizeObserver.observe(containerRef.current)
    return () => {
      resizeObserver.disconnect()
    }
  }, [updateWidth, componentIdKey])

  // Build layouts — include `width` in deps so the object reference changes
  // whenever the container resizes. This forces react-grid-layout to fully
  // recalculate positions from the original component.position values, preventing
  // stale compacted positions from sidebar toggle or window resize.
  //
  // xs is generated separately by buildMobileLayout (2-col smart rearrange).
  // The same desktop `layout` array is shared across lg/md/sm — RGL treats
  // layouts as immutable and clones internally, so sharing the reference is
  // safe and avoids a triple allocation on every width change.
  const layouts = useMemo(() => {
    const layout: Layout = components.map((c) => ({
      i: c.id,
      x: c.position.x ?? 0,
      y: c.position.y ?? 0,
      w: c.position.w ?? 4,
      h: c.position.h ?? 3,
      minW: c.position.minW ?? 1,
      minH: c.position.minH ?? 1,
      maxW: c.position.maxW,
      maxH: c.position.maxH,
      static: false,
    }))
    const mobile = buildMobileLayout(components)
    return { lg: layout, md: layout, sm: layout, xs: mobile }
    // width in deps: forces react-grid-layout to recalculate on container resize
  }, [componentIdKey, width, components])

  // When dashboard switches (componentIdKey changes), synchronously re-measure
  // container width to avoid the grid being hidden by the `width > 0` guard.
  // ResizeObserver fires asynchronously which can leave width=0 for one render.
  useEffect(() => {
    if (containerRef.current) {
      const w = containerRef.current.offsetWidth
      if (w > 0) {
        widthRef.current = w
        setWidth(w)
      }
    }
  }, [componentIdKey])

  // Drag/resize handlers — freeze width updates during interaction, persist on stop
  const handleDragStart = useCallback(() => {
    isInteractingRef.current = true
  }, [])

  const handleDragStop = useCallback((layout: any) => {
    isInteractingRef.current = false
    if (onLayoutChange && editMode) {
      onLayoutChange(layout as readonly any[])
    }
    // Re-measure width after drag (container may have changed)
    requestAnimationFrame(updateWidth)
  }, [onLayoutChange, editMode, updateWidth])

  const handleResizeStart = useCallback(() => {
    isInteractingRef.current = true
  }, [])

  const handleResizeStop = useCallback((layout: any) => {
    isInteractingRef.current = false
    if (onLayoutChange && editMode) {
      onLayoutChange(layout as readonly any[])
    }
    requestAnimationFrame(updateWidth)
  }, [onLayoutChange, editMode, updateWidth])

  // Transitions
  const [transitionsEnabled, setTransitionsEnabled] = useState(false)
  useEffect(() => {
    if (width > 0 && !transitionsEnabled) {
      const timer = setTimeout(() => setTransitionsEnabled(true), 300)
      return () => clearTimeout(timer)
    }
  }, [width, transitionsEnabled])

  return (
    <div ref={containerRef} className={cn('w-full', className)}>
      <style>{`
        .react-grid-layout { display: block !important; }
        .react-grid-item {
          ${transitionsEnabled ? 'transition: transform 200ms ease;' : 'transition: none;'}
        }
        ${touchEnabled ? `
        .react-grid-item { touch-action: none; }
        .react-grid-item > * { touch-action: pan-y pinch-zoom; }
        .react-grid-layout.edit-mode .react-grid-item { touch-action: none; }
        .react-grid-layout:not(.edit-mode) .react-grid-item { touch-action: pan-y pan-x pinch-zoom; }
        .react-grid-item button[style*="touch-action: manipulation"] {
          touch-action: manipulation !important;
          pointer-events: auto !important;
        }
        ` : ''}
        .dashboard-item {
          width: 100%; height: 100%;
          display: flex; flex-direction: column; overflow: hidden;
        }
        .dashboard-item > * {
          height: 100%; min-height: 0;
          display: flex; flex-direction: column;
        }
        .react-grid-placeholder {
          background: rgba(148, 163, 184, 0.15) !important;
          border: 1px dashed rgba(148, 163, 184, 0.3) !important;
          border-radius: 0.5rem; margin: 0;
          transition: all 150ms ease;
        }
        .react-grid-layout.edit-mode > .react-grid-item.react-draggable-dragging {
          z-index: 100;
          box-shadow: var(--shadow-xl);
          transition: none; opacity: 0.9;
        }
        .react-grid-layout.edit-mode > .react-grid-item.resizing {
          z-index: 100; transition: none;
        }
        .react-resizable-handle {
          position: absolute;
          width: ${isMobile ? '32px' : '14px'}; height: ${isMobile ? '32px' : '14px'};
          bottom: 0; right: 0; background: transparent; cursor: se-resize; z-index: 10;
        }
        ${isMobile ? `
        .react-resizable-handle { width: 44px; height: 44px; }
        .react-resizable-handle::after {
          width: 20px; height: 20px; right: 8px; bottom: 8px;
          border-right-width: 3px; border-bottom-width: 3px;
        }
        ` : ''}
        .react-resizable-handle::after {
          content: ''; position: absolute; right: 2px; bottom: 2px;
          width: 6px; height: 6px;
          border-right: 2px solid color-mix(in oklch, var(--muted-foreground) 50%, transparent);
          border-bottom: 2px solid color-mix(in oklch, var(--muted-foreground) 50%, transparent);
          border-radius: 0 0 1px 0;
        }
        .react-grid-layout.edit-mode > .react-grid-item:hover .react-resizable-handle::after {
          border-right-color: var(--primary);
          border-bottom-color: var(--primary);
          width: 8px; height: 8px;
        }
        .react-grid-layout:not(.edit-mode) .react-resizable-handle { display: none; }
        .react-grid-layout:not(.edit-mode) > .react-grid-item {
          transition: none !important;
        }
      `}</style>
      {width > 0 && (
        <ResponsiveGridLayout
          key={componentIdKey}
          className={cn('dashboard-grid', editMode && 'edit-mode')}
          layouts={layouts}
          breakpoints={breakpoints}
          cols={cols}
          width={width}
          rowHeight={rowHeight}
          margin={margin}
          containerPadding={containerPadding}
          maxRows={Infinity}
          dragConfig={{ enabled: editMode && !isMobile, bounded: false }}
          resizeConfig={{ enabled: editMode && !isMobile, handles: ['se'] as const }}
          onDragStart={handleDragStart}
          onDragStop={handleDragStop}
          onResizeStart={handleResizeStart}
          onResizeStop={handleResizeStop}
        >
          {components.map((component) => (
            <div
              key={component.id}
              className="dashboard-item h-full"
            >
              {component.children ?? component.render?.()}
            </div>
          ))}
        </ResponsiveGridLayout>
      )}
      {editMode && <div className="h-48" />}
    </div>
  )
}

/**
 * Dashboard Grid Component
 *
 * Provides a drag-and-drop grid layout using react-grid-layout v2.
 * Supports responsive layouts and edit mode with visual feedback.
 * Now with touch support for mobile devices.
 *
 * Layout Tracking Strategy:
 * - During drag/resize: track positions in ref without re-rendering
 * - After drag/resize: update state to sync with parent
 * - External changes: only update if not in middle of drag/resize
 */

import { useRef, useState, useEffect, useCallback, useMemo } from 'react'
import { ReactElement } from 'react'
import { ResponsiveGridLayout } from 'react-grid-layout'
import { cn } from '@/lib/utils'
import { responsiveCols } from '@/design-system/tokens/size'
import { useIsMobile } from '@/hooks/useMobile'

// Import styles
import 'react-grid-layout/css/styles.css'
import 'react-resizable/css/styles.css'

// Check if device supports touch
const isTouchDevice = () => {
  if (typeof window === 'undefined') return false
  return 'ontouchstart' in window || navigator.maxTouchPoints > 0
}

export interface DashboardGridProps {
  // Components to render
  components: Array<{
    id: string
    position: { x: number; y: number; w: number; h: number; minW?: number; minH?: number; maxW?: number; maxH?: number }
    children: ReactElement
  }>

  // Layout configuration
  rowHeight?: number
  margin?: [number, number]
  containerPadding?: [number, number]
  breakpoints?: Record<string, number>
  cols?: Record<string, number>

  // Edit mode
  editMode?: boolean
  onLayoutChange?: (layout: readonly any[]) => void

  // Styling
  className?: string
}

export function DashboardGrid({
  components,
  rowHeight = 60,  // Grid row height in pixels - each h:1 = 60px
  margin = [4, 4],  // Gap between grid cells
  containerPadding = [4, 4],  // Padding around grid
  breakpoints = { lg: 1200, md: 996, sm: 768, xs: 480 },
  cols = responsiveCols,
  editMode = false,
  onLayoutChange,
  className,
}: DashboardGridProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const isMobile = useIsMobile()
  const touchEnabled = isTouchDevice()

  // Track layout positions from parent (props) - this is our "source of truth"
  // We ONLY update this from props, never from drag/resize
  const [parentLayoutPositions, setParentLayoutPositions] = useState<Record<string, { x: number; y: number; w: number; h: number }>>(() => {
    const initial: Record<string, { x: number; y: number; w: number; h: number }> = {}
    components.forEach(c => {
      initial[c.id] = { x: c.position.x, y: c.position.y, w: c.position.w, h: c.position.h }
    })
    return initial
  })

  // Track positions during drag/resize - use ref to avoid re-renders
  const dragLayoutRef = useRef<Record<string, { x: number; y: number; w: number; h: number }>>({})

  // Track if we're in the middle of a drag/resize operation
  const isDraggingRef = useRef(false)

  // Track if the current layout change is from user interaction (drag/resize) vs responsive resize
  const isUserInteractionRef = useRef(false)

  // Track the last layout we sent to parent to avoid echo effect
  const lastSentLayoutRef = useRef<string>('')

  // Sync parent layout positions when components prop changes
  useEffect(() => {
    if (!isDraggingRef.current) {
      const newPositions: Record<string, { x: number; y: number; w: number; h: number }> = {}
      let hasChanges = false

      components.forEach(c => {
        const existing = parentLayoutPositions[c.id]
        if (!existing || existing.x !== c.position.x || existing.y !== c.position.y ||
            existing.w !== c.position.w || existing.h !== c.position.h) {
          newPositions[c.id] = { x: c.position.x, y: c.position.y, w: c.position.w, h: c.position.h }
          hasChanges = true
        } else {
          newPositions[c.id] = existing
        }
      })

      // Check for removed components
      const currentIds = new Set(Object.keys(newPositions))
      Object.keys(parentLayoutPositions).forEach(id => {
        if (!currentIds.has(id)) {
          hasChanges = true
        }
      })

      if (hasChanges) {
        setParentLayoutPositions(newPositions)
      }
    }
  }, [components, parentLayoutPositions])

  // Debounced update for container width
  const updateWidth = useCallback(() => {
    if (containerRef.current) {
      setWidth(containerRef.current.offsetWidth)
    }
  }, [])

  // Debounce timeout ref
  const resizeTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  // Update container width on resize (with debounce to prevent rapid re-renders)
  useEffect(() => {
    updateWidth()

    const resizeObserver = new ResizeObserver(() => {
      // Clear existing timeout
      if (resizeTimeoutRef.current) {
        clearTimeout(resizeTimeoutRef.current)
      }
      // Debounce the width update
      resizeTimeoutRef.current = setTimeout(() => {
        requestAnimationFrame(updateWidth)
      }, 100) // 100ms debounce
    })
    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    const handleWindowResize = () => {
      // Clear existing timeout
      if (resizeTimeoutRef.current) {
        clearTimeout(resizeTimeoutRef.current)
      }
      // Debounce the width update
      resizeTimeoutRef.current = setTimeout(() => {
        requestAnimationFrame(updateWidth)
      }, 100) // 100ms debounce
    }
    window.addEventListener('resize', handleWindowResize)

    return () => {
      resizeObserver.disconnect()
      window.removeEventListener('resize', handleWindowResize)
      if (resizeTimeoutRef.current) {
        clearTimeout(resizeTimeoutRef.current)
      }
    }
  }, [updateWidth])

  // Get current layout positions - use drag positions if dragging, otherwise parent positions
  const getCurrentPositions = useCallback(() => {
    if (isDraggingRef.current && Object.keys(dragLayoutRef.current).length > 0) {
      return dragLayoutRef.current
    }
    return parentLayoutPositions
  }, [parentLayoutPositions])

  // Create base layout from current positions
  // We use a state-derived key to force recalculation during drag
  const [dragKey, setDragKey] = useState(0)

  const baseLayout = useMemo(() => {
    const positions = getCurrentPositions()
    return components.map((c) => {
      const pos = positions[c.id] || c.position
      return {
        i: c.id,
        x: pos.x,
        y: pos.y,
        w: pos.w,
        h: pos.h,
        minW: c.position.minW ?? 1,
        minH: c.position.minH ?? 1,
        maxW: c.position.maxW,
        maxH: c.position.maxH,
        static: false,
      }
    })
  }, [components, parentLayoutPositions, dragKey, getCurrentPositions])

  const layout = baseLayout

  // Handle layout changes during drag/resize
  // Update drag ref immediately to keep layouts in sync, notify parent on user interaction
  const handleLayoutChange = useCallback((currentLayout: any, allLayouts?: any) => {
    // Update drag ref IMMEDIATELY - this happens before any re-render
    const newPositions: Record<string, { x: number; y: number; w: number; h: number }> = {}
    currentLayout.forEach((item: any) => {
      newPositions[item.i] = { x: item.x, y: item.y, w: item.w, h: item.h }
    })
    dragLayoutRef.current = newPositions

    // Only force re-render and notify parent if this is a user-initiated change (drag/resize)
    // Ignore automatic layout changes from responsive width changes
    if (isUserInteractionRef.current) {
      // Force re-render to update layouts prop with new drag positions
      setDragKey(k => k + 1)

      // Only notify parent during edit mode (user drag/resize)
      if (onLayoutChange && editMode) {
        onLayoutChange(currentLayout as readonly any[])
      }
    }
    // Note: We still update dragLayoutRef even for responsive changes so components don't jump
    // But we don't trigger re-renders or parent updates for responsive changes
  }, [onLayoutChange, editMode])

  // Track drag start
  const handleDragStart = useCallback(() => {
    isDraggingRef.current = true
    isUserInteractionRef.current = true
  }, [])

  // Track drag end - notify parent with final layout
  const handleDragStop = useCallback((layout: any) => {
    // Notify parent with final layout position
    if (onLayoutChange && editMode) {
      onLayoutChange(layout as readonly any[])
    }
    // Clear drag state - effect will sync from components prop on next render
    isDraggingRef.current = false
    isUserInteractionRef.current = false
    dragLayoutRef.current = {}
  }, [onLayoutChange, editMode])

  // Track resize start
  const handleResizeStart = useCallback(() => {
    isDraggingRef.current = true
    isUserInteractionRef.current = true
  }, [])

  // Track resize end - notify parent with final layout
  const handleResizeStop = useCallback((layout: any) => {
    // Notify parent with final layout position
    if (onLayoutChange && editMode) {
      onLayoutChange(layout as readonly any[])
    }
    // Clear drag state - effect will sync from components prop on next render
    isDraggingRef.current = false
    isUserInteractionRef.current = false
    dragLayoutRef.current = {}
  }, [onLayoutChange, editMode])

  return (
    <div ref={containerRef} className={cn('w-full', className)}>
      <style>{`
        /* Grid layout base */
        .react-grid-layout {
          display: block !important;
        }

        /* Grid items - controlled by react-grid-layout, don't override */
        .react-grid-item {
          /* Only add transition, let react-grid-layout handle position/size */
          transition: transform 200ms ease;
        }

        /* Touch-friendly touch-action */
        ${touchEnabled ? `
        .react-grid-item {
          touch-action: none;
        }

        .react-grid-item > * {
          touch-action: pan-y pinch-zoom;
        }

        .react-grid-layout.edit-mode .react-grid-item {
          touch-action: none;
        }

        .react-grid-layout:not(.edit-mode) .react-grid-item {
          touch-action: pan-y pan-x pinch-zoom;
        }

        /* Mobile edit button - ensure it's clickable */
        .react-grid-item button[style*="touch-action: manipulation"] {
          touch-action: manipulation !important;
          pointer-events: auto !important;
        }
        ` : ''}

        /* Dashboard item fills the grid cell allocated by react-grid-layout */
        .dashboard-item {
          width: 100%;
          height: 100%;
          display: flex;
          flex-direction: column;
          overflow: hidden;
        }

        /* Drag placeholder - use !important to override library defaults */
        .react-grid-placeholder {
          background: rgba(148, 163, 184, 0.15) !important;
          border: 1px dashed rgba(148, 163, 184, 0.3) !important;
          border-radius: 0.5rem;
          margin: 0;
          transition: all 150ms ease;
        }

        /* Edit mode styles - no additional outline/border, components handle their own styling */
        .react-grid-layout.edit-mode > .react-grid-item {
          /* No outline or border-radius - let components style themselves */
        }

        .react-grid-layout.edit-mode > .react-grid-item:hover {
          /* Hover effect handled by component's shadow transition */
        }

        .react-grid-layout.edit-mode > .react-grid-item.react-draggable-dragging {
          z-index: 100;
          box-shadow: 0 12px 32px rgba(0, 0, 0, 0.15);
          transition: none;
          opacity: 0.9;
        }

        .react-grid-layout.edit-mode > .react-grid-item.resizing {
          z-index: 100;
          transition: none;
        }

        /* Resize handle - visible only in edit mode */
        .react-resizable-handle {
          position: absolute;
          width: ${isMobile ? '32px' : '14px'};
          height: ${isMobile ? '32px' : '14px'};
          bottom: 0;
          right: 0;
          background: transparent;
          cursor: se-resize;
          z-index: 10;
        }

        ${isMobile ? `
        .react-resizable-handle {
          width: 44px;
          height: 44px;
        }

        .react-resizable-handle::after {
          width: 20px;
          height: 20px;
          right: 8px;
          bottom: 8px;
          border-right-width: 3px;
          border-bottom-width: 3px;
        }
        ` : ''}

        .react-resizable-handle::after {
          content: '';
          position: absolute;
          right: 2px;
          bottom: 2px;
          width: 6px;
          height: 6px;
          border-right: 2px solid hsl(var(--muted-foreground) / 0.5);
          border-bottom: 2px solid hsl(var(--muted-foreground) / 0.5);
          border-radius: 0 0 1px 0;
        }

        .react-grid-layout.edit-mode > .react-grid-item:hover .react-resizable-handle::after {
          border-right-color: hsl(var(--primary));
          border-bottom-color: hsl(var(--primary));
          width: 8px;
          height: 8px;
        }

        /* Hide resize handles when not in edit mode */
        .react-grid-layout:not(.edit-mode) .react-resizable-handle {
          display: none;
        }

        /* Smooth transitions when not editing */
        .react-grid-layout:not(.edit-mode) > .react-grid-item {
          transition: transform 200ms cubic-bezier(0.4, 0, 0.2, 1), width 200ms cubic-bezier(0.4, 0, 0.2, 1), height 200ms cubic-bezier(0.4, 0, 0.2, 1);
        }
      `}</style>
      {width > 0 && (
        <ResponsiveGridLayout
          className={cn('dashboard-grid', editMode && 'edit-mode')}
          layouts={{
            lg: layout,
            md: layout,
            sm: layout,
            xs: layout,
          }}
          breakpoints={breakpoints}
          cols={cols}
          width={width}
          rowHeight={rowHeight}
          margin={margin}
          containerPadding={containerPadding}
          maxRows={Infinity}
          dragConfig={{ enabled: editMode, bounded: false }}
          resizeConfig={{ enabled: editMode, handles: ['se'] as const }}
          onLayoutChange={handleLayoutChange}
          onDragStart={handleDragStart}
          onDragStop={handleDragStop}
          onResizeStart={handleResizeStart}
          onResizeStop={handleResizeStop}
          isDraggable={editMode}
          isResizable={editMode}
        >
          {components.map((component) => (
            <div key={component.id} className="dashboard-item h-full">
              {component.children}
            </div>
          ))}
        </ResponsiveGridLayout>
      )}
    </div>
  )
}

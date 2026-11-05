/**
 * UnderlineTabs — the single desktop underline-tab primitive.
 *
 * Every desktop page-tab strip renders through this component so the
 * geometry (h-9 / px-3 / gap-1), the active treatment (text-foreground +
 * one sliding 2px indicator), the disabled style and the APG keyboard
 * behaviour can't drift between call sites again.
 *
 * The indicator is one absolutely-positioned bar measured against the
 * active button (not a per-tab ::after), so switching tabs SLIDES the
 * underline instead of jumping it. It hides until the first measurement
 * lands so it never animates from (0,0) on mount.
 */
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import type { TabConfig } from './PageTabs'

export interface UnderlineTabsProps {
  tabs: TabConfig[]
  activeTab: string
  onTabChange: (value: string) => void
  className?: string
  /**
   * id prefix for APG wiring with the tabpanels. Keep the default:
   * `page-tab-*` pairs with PageTabsContent's `page-tabpanel-*`.
   */
  idPrefix?: string
}

export function UnderlineTabs({
  tabs,
  activeTab,
  onTabChange,
  className,
  idPrefix = 'page-tab',
}: UnderlineTabsProps) {
  const scrollerRef = useRef<HTMLDivElement>(null)
  const innerRef = useRef<HTMLDivElement>(null)
  const btnRefs = useRef(new Map<string, HTMLButtonElement>())
  // null = not yet measured → don't render the bar (no slide-in from 0).
  const [indicator, setIndicator] = useState<{ left: number; width: number } | null>(null)
  const [fades, setFades] = useState({ left: false, right: false })

  const measure = useCallback(() => {
    const btn = btnRefs.current.get(activeTab)
    if (!btn || !tabs.some((t) => t.value === activeTab)) {
      setIndicator(null)
      return
    }
    // offsetParent is the relative inner wrapper → offsets are strip-local
    // and the bar scrolls with the tabs when the strip overflows.
    setIndicator({ left: btn.offsetLeft, width: btn.offsetWidth })
  }, [activeTab, tabs])

  const updateFades = useCallback(() => {
    const el = scrollerRef.current
    if (!el) return
    setFades({
      left: el.scrollLeft > 1,
      right: el.scrollLeft + el.clientWidth < el.scrollWidth - 1,
    })
  }, [])

  // Pre-paint measurement on tab change so the bar slides from its old spot.
  useLayoutEffect(() => {
    measure()
  }, [measure])

  useEffect(() => {
    const inner = innerRef.current
    const scroller = scrollerRef.current
    // jsdom (tests) has no ResizeObserver — the layout-effect measure above
    // already covers the mounted state there.
    if (!inner || !scroller || typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => {
      measure()
      updateFades()
    })
    ro.observe(inner)
    ro.observe(scroller)
    window.addEventListener('resize', updateFades)
    let cancelled = false
    // Web-font swap changes label widths — re-measure once fonts settle.
    document.fonts?.ready.then(() => {
      if (!cancelled) measure()
    })
    return () => {
      cancelled = true
      ro.disconnect()
      window.removeEventListener('resize', updateFades)
    }
  }, [measure, updateFades])

  // ARIA APG Tabs — arrow keys move between tabs (automatic activation),
  // Home/End jump to the ends. Roving tabindex keeps a single tab stop in
  // the page Tab order.
  const handleKeyDown = (e: React.KeyboardEvent) => {
    const keys = ['ArrowRight', 'ArrowLeft', 'Home', 'End']
    if (!keys.includes(e.key)) return
    const selectable = tabs.filter((t) => !t.disabled)
    if (selectable.length === 0) return
    const current = selectable.findIndex((t) => t.value === activeTab)
    let next: number
    if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = selectable.length - 1
    else {
      const delta = e.key === 'ArrowRight' ? 1 : -1
      next = current === -1 ? 0 : (current + delta + selectable.length) % selectable.length
    }
    e.preventDefault()
    const target = selectable[next]
    onTabChange(target.value)
    // Move focus too — the chip's tabIndex follows activeTab, so query the
    // fresh node after activation.
    requestAnimationFrame(() => {
      document.getElementById(`${idPrefix}-${target.value}`)?.focus()
    })
  }

  return (
    <div className={cn('relative min-w-0 max-w-full', className)}>
      <div
        ref={scrollerRef}
        role="tablist"
        onScroll={updateFades}
        onKeyDown={handleKeyDown}
        className="flex items-center gap-1 overflow-x-auto scrollbar-none"
      >
        <div ref={innerRef} className="relative inline-flex items-center gap-1">
          {tabs.map((tab) => {
            const isActive = activeTab === tab.value
            return (
              <button
                key={tab.value}
                ref={(el) => {
                  if (el) btnRefs.current.set(tab.value, el)
                  else btnRefs.current.delete(tab.value)
                }}
                id={`${idPrefix}-${tab.value}`}
                role="tab"
                aria-selected={isActive}
                aria-controls={`${idPrefix}panel-${tab.value}`}
                tabIndex={isActive ? 0 : -1}
                disabled={tab.disabled}
                onClick={() => onTabChange(tab.value)}
                className={cn(
                  /* Hover: text brightens + a quiet muted wash covers the
                     whole item (icon included) — color-only hover read as a
                     flicker. The underline itself is the shared sliding bar
                     below, not a per-tab pseudo-element.
                     min-w floor: 2-char CJK labels (+px-3) land ~52px and
                     read cramped next to longer siblings — 64px keeps every
                     chip present without equalizing widths. */
                  'relative inline-flex min-w-[4rem] shrink-0 items-center justify-start gap-2 px-3 h-9 text-sm font-medium whitespace-nowrap rounded-md transition-colors',
                  'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                  'disabled:pointer-events-none disabled:opacity-50',
                  isActive
                    ? 'text-foreground'
                    : 'text-muted-foreground hover:text-foreground hover:bg-muted-50',
                )}
              >
                {tab.icon && <span className="shrink-0 h-4 w-4">{tab.icon}</span>}
                <span>{tab.label}</span>
              </button>
            )
          })}
          {indicator && (
            <span
              data-tab-indicator
              aria-hidden="true"
              className="pointer-events-none absolute bottom-0 h-0.5 rounded-full bg-foreground transition-[left,width] duration-normal ease-out"
              style={{ left: indicator.left, width: indicator.width }}
            />
          )}
        </div>
      </div>
      {/* Overflow affordance: fade the strip's edges while content hides
          beyond them, so a scrolled tab strip doesn't end in a hard cut. */}
      <span
        aria-hidden="true"
        className={cn(
          'pointer-events-none absolute inset-y-0 left-0 w-6 bg-gradient-to-r from-background to-transparent transition-opacity duration-normal',
          fades.left ? 'opacity-100' : 'opacity-0',
        )}
      />
      <span
        aria-hidden="true"
        className={cn(
          'pointer-events-none absolute inset-y-0 right-0 w-6 bg-gradient-to-l from-background to-transparent transition-opacity duration-normal',
          fades.right ? 'opacity-100' : 'opacity-0',
        )}
      />
    </div>
  )
}

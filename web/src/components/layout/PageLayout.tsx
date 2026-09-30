import { ReactNode, Fragment, useEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import { PageHeader } from '@/components/layout/PageHeader'
import { MobilePageHeader } from '@/components/layout/MobilePageHeader'
import {
  MobileHeaderActionsContext,
  useMobileHeaderActionsRegistry,
} from '@/components/layout/MobileHeaderActionsContext'
import { useIsMobile } from '@/hooks/useMobile'

/**
 * Optional mobile-header overrides. Only applied on mobile; ignored on desktop.
 * Useful for sub-pages that need a back chevron in the header slot or want to
 * hide the hamburger (e.g. full-screen drill-downs managed via in-page state).
 */
export interface PageLayoutMobileHeaderProps {
  /** Rendered after the hamburger (e.g. back chevron). */
  leftExtra?: ReactNode
  /** Hide the hamburger button. */
  hideMenu?: boolean
  /**
   * Override the title shown in MobilePageHeader. When omitted, the page's
   * `title` prop is used. Useful when the mobile view drills into a sub-section
   * whose label differs from the page title.
   */
  titleOverride?: ReactNode
}

export interface PageLayoutProps {
  children: ReactNode
  /** Optional page title, rendered via PageHeader */
  title?: string
  /** Optional secondary description text below the title */
  subtitle?: string
  /** Optional actions area rendered on the right of the header (buttons, filters, etc.) */
  actions?: ReactNode
  /** Optional footer content (e.g., pagination bar fixed at bottom) */
  footer?: ReactNode
  /** Optional fixed header content (e.g., tabs) - rendered between title and scrollable content */
  headerContent?: ReactNode
  maxWidth?: 'md' | 'lg' | 'xl' | '2xl' | 'full'
  className?: string
  /** Whether to render a subtle bottom border under the header */
  borderedHeader?: boolean
  /** Whether to hide footer on mobile (for infinite scroll) */
  hideFooterOnMobile?: boolean
  /** Whether to fix actions bar on mobile (don't scroll with content) */
  fixedActionsOnMobile?: boolean
  /** Whether to remove scroll container padding (for full-bleed children like detail views) */
  noPadding?: boolean
  /** Whether page has a bottom tab navigation bar (mobile) - adds extra bottom padding */
  hasBottomNav?: boolean
  /** Mobile header overrides (ignored on desktop). */
  mobileHeader?: PageLayoutMobileHeaderProps
}

const maxWidthClass = {
  md: 'max-w-4xl',
  lg: 'max-w-6xl',
  xl: 'max-w-7xl',
  '2xl': 'max-w-7xl',
  full: 'max-w-full',
}

/**
 * Scrolled-header elevation: a two-layer contact+ambient shadow that fades
 * downward from the header's bottom edge. Deliberately shadow-ONLY — a
 * hairline border sits flush against the tab underline and the toolbar
 * buttons/inputs that bottom-align at this edge (they'd read as touching
 * the line), while a shadow starts at ~4% opacity and immediately falls
 * off, reading as depth instead of a divider.
 */
const ELEVATED_SHADOW =
  'shadow-[0_1px_2px_rgba(0,0,0,0.04),0_4px_12px_-2px_rgba(0,0,0,0.07)]'

/**
 * Standard page layout container
 *
 * Provides consistent padding, max-width, and optional header across all pages.
 *
 * @example
 * <PageLayout
 *   title="Devices"
 *   subtitle="Manage all connected devices"
 *   actions={<Button size="sm">Refresh</Button>}
 *   maxWidth="xl"
 *   footer={<Pagination />}
 *   hideFooterOnMobile
 * >
 *   <div>Content here</div>
 * </PageLayout>
 */
export function PageLayout({
  children,
  title,
  subtitle,
  actions,
  footer,
  headerContent,
  maxWidth = 'full',
  className,
  borderedHeader = false,
  hideFooterOnMobile = false,
  fixedActionsOnMobile: _fixedActionsOnMobile = false,
  noPadding = false,
  hasBottomNav = false,
  mobileHeader,
}: PageLayoutProps) {
  const isMobile = useIsMobile()
  // Scroll-aware header elevation. The scroll container sits BELOW the
  // title/tabs stack and shares its white bg, so rows scrolling up get
  // hard-clipped at an invisible boundary (reads as tearing). Once the
  // container is scrolled, the header stack's bottom edge gains a soft
  // downward shadow — the Linear/Notion affordance that says "content
  // continues beneath this line". At rest nothing shows, preserving the
  // tabs' underline-only design.
  //
  // Detection is a 1px sentinel + IntersectionObserver, NOT onScroll:
  // IO is computed by the rendering engine on every scroll cause
  // (wheel, programmatic, keyboard, anchor) with no dependency on
  // scroll-event dispatch, and it clears itself when content swaps
  // shrink the container back to unscrolled.
  const [contentScrolled, setContentScrolled] = useState(false)
  const sentinelRef = useRef<HTMLDivElement>(null)
  const scrollerRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const sentinel = sentinelRef.current
    const root = scrollerRef.current
    if (!sentinel || !root) return
    const io = new IntersectionObserver(
      ([entry]) => setContentScrolled(!entry.isIntersecting),
      { root },
    )
    io.observe(sentinel)
    return () => io.disconnect()
  }, [])
  // Only meaningful on desktop: the mobile header is the fixed
  // MobilePageHeader with its own chrome, and headerContent's wrapper can
  // be empty there (PageTabsBar lifts its actions away on mobile).
  const headerElevated = !isMobile && contentScrolled
  // Registry that lets children (e.g. PageTabsBar on mobile) "lift" their
  // action buttons into the MobilePageHeader above the content, and push
  // wide controls (search/filter) into a sticky toolbar inside the content.
  const {
    value: actionsCtxValue,
    collectedHeader: collectedMobileActions,
    collectedContent: collectedMobileContentActions,
  } = useMobileHeaderActionsRegistry()

  // Determine if footer should be shown
  const showFooter = footer && !(isMobile && hideFooterOnMobile)

  // Bottom spacer height: uses inline style to avoid CSS specificity issues
  // with safe-bottom/pb-* classes overriding each other.
  const bottomSpacerHeight = hasBottomNav && isMobile
    ? 'calc(8rem + env(safe-area-inset-bottom, 0px))'  // per-page bottom nav + safe area
    : showFooter
      ? '14rem'                                          // footer clearance (224px)
      : isMobile
        ? 'calc(1.5rem + env(safe-area-inset-bottom, 0px))'
        : '2rem'

  return (
    <MobileHeaderActionsContext.Provider value={actionsCtxValue}>
    <div className="flex flex-col h-full">
      {/* Mobile: per-page header is always rendered so the hamburger menu
          stays accessible even on pages that pass an empty title (e.g.
          detail views). Without this, drilling into a detail screen
          removes the only way to open the nav drawer. */}
      {isMobile && (
        <MobilePageHeader
          title={mobileHeader?.titleOverride ?? title}
          actions={
            <>
              {actions}
              {collectedMobileActions.map((node, i) => (
                <Fragment key={i}>{node}</Fragment>
              ))}
            </>
          }
          leftExtra={mobileHeader?.leftExtra}
          hideMenu={mobileHeader?.hideMenu}
        />
      )}
      {/* Desktop: PageHeader with title + description + actions.
          mx-auto + maxWidth mirror the scroll container so the title and
          content share the same left edge. Pages without headerContent have
          the scroll edge directly under this strip — it takes the elevation
          instead. */}
      {title && !isMobile && (
        <div
          className={cn(
            'shrink-0 bg-background transition-shadow duration-normal',
            !headerContent && headerElevated && ELEVATED_SHADOW,
          )}
        >
          <div className={cn('mx-auto w-full px-4 pt-4 pb-2 sm:px-6 sm:pt-5 sm:pb-3 md:px-8 md:pt-6 md:pb-3', maxWidthClass[maxWidth], className)}>
            <PageHeader
              title={title}
              description={subtitle}
              actions={actions}
              variant={borderedHeader ? 'bordered' : 'default'}
            />
          </div>
        </div>
      )}
      {/* Fixed header content (e.g., tabs) - outside scroll container.
          bg-background matches the title strip above and the scroll
          container below for visual continuity. When content scrolls
          beneath, a SOFT downward shadow fades in (no hairline border — a
          crisp line hugs the tab/button bottoms sitting flush at this
          edge; a shadow reads as the header floating above the content). */}
      {headerContent && (
        <div
          className={cn(
            'shrink-0 bg-background transition-shadow duration-normal',
            headerElevated && ELEVATED_SHADOW,
          )}
        >
          {headerContent}
        </div>
      )}
      {/* Content area */}
      <div className={cn('flex-1 flex flex-col min-h-0', className)}>
        {/* Mobile sticky content toolbar — search/filter controls lifted by
            PageTabsBar's actionsExtra. Sits between the header and the scroll
            container so it stays visible while content scrolls. Desktop is
            unaffected (those actions render in the desktop tab bar instead). */}
        {isMobile && collectedMobileContentActions.length > 0 && (
          <div className="shrink-0  bg-background px-3 py-2">
            <div className="flex items-center gap-2 overflow-x-auto scrollbar-none">
              {collectedMobileContentActions.map((node, i) => (
                <Fragment key={i}>{node}</Fragment>
              ))}
            </div>
          </div>
        )}
        {/* Scrollable content. bg-background + overscroll-none so the
            rubber-band / pull-to-refresh bounce on mobile never exposes a
            transparent strip above the first (often sticky) child. The
            1px sentinel at the very top drives the header elevation —
            scrolled ⇔ it has left the viewport. */}
        <div
          ref={scrollerRef}
          className={cn(
            '@container flex-1 flex flex-col overflow-auto bg-background overscroll-none',
            !noPadding && 'px-4 sm:px-6 md:px-8',
            !noPadding && isMobile && 'pt-2',
          )}
          data-page-scroll-container
        >
          <div ref={sentinelRef} aria-hidden="true" className="h-px shrink-0" />
          <div className={cn('mx-auto w-full flex flex-col min-h-full animate-fade-in', maxWidthClass[maxWidth])}>
            {children}
            {/* Mount node for infinite-scroll sentinels — Pagination portals its
                loading / "no more items" hint into this div. It must sit BEFORE
                the bottom spacer: appended after the spacer, the hint lands
                underneath the fixed bottom nav. */}
            <div data-infinite-scroll-mount className="shrink-0" />
            {/* Bottom spacer: ensures content isn't hidden behind fixed footer/nav */}
            {!noPadding && (
              <div className="shrink-0" style={{ height: bottomSpacerHeight }} />
            )}
          </div>
        </div>
      </div>
      {/* Fixed footer with glass morphism effect. left offsets past the
          desktop AppSidebar (0 on mobile — sidebar unmounted, var is 0px). */}
      {showFooter ? (
        <div
          className="fixed bottom-[var(--keyboard-offset,0px)] bg-surface-glass backdrop-blur-xl border-t border-glass-border safe-bottom z-10 transition-[margin-right] duration-normal ease-out"
          style={{ left: 'var(--app-sidebar-width, 0px)', right: 0, marginRight: 'var(--dock-chat-width, 0px)' }}
        >
          <div className={cn('w-full px-4 py-4 sm:px-6 sm:py-5 md:px-8', maxWidthClass[maxWidth], className)}>
            {footer}
          </div>
        </div>
      ) : isMobile && hideFooterOnMobile && footer ? (
        /* Hidden mount point for footer content (e.g., Pagination) so hooks like
           useWindowScrollLoad still run for mobile infinite scroll */
        <div className="hidden">{footer}</div>
      ) : null}
    </div>
    </MobileHeaderActionsContext.Provider>
  )
}

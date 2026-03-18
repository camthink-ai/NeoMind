import { ReactNode } from 'react'
import { cn } from '@/lib/utils'
import { PageHeader } from '@/components/layout/PageHeader'
import { useIsMobile } from '@/hooks/useMobile'

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
}

const maxWidthClass = {
  md: 'max-w-4xl',
  lg: 'max-w-6xl',
  xl: 'max-w-7xl',
  '2xl': 'max-w-7xl',
  full: 'max-w-full',
}

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
  fixedActionsOnMobile = false,
}: PageLayoutProps) {
  const isMobile = useIsMobile()

  // Determine if footer should be shown
  const showFooter = footer && !(isMobile && hideFooterOnMobile)

  return (
    <div className="flex flex-col h-full">
      {title && (
        <div className="shrink-0 bg-background">
          <div className={cn('w-full px-4 pt-4 pb-2 sm:px-6 sm:pt-5 sm:pb-3 md:px-8 md:pt-6 md:pb-3', maxWidthClass[maxWidth], className)}>
            <PageHeader
              title={title}
              description={subtitle}
              actions={actions}
              variant={borderedHeader ? 'bordered' : 'default'}
            />
          </div>
        </div>
      )}
      {/* Fixed header content (e.g., tabs) - outside scroll container */}
      {headerContent && (
        <div className="shrink-0 bg-background">
          {headerContent}
        </div>
      )}
      {/* Content area - uses flex-col to push sticky elements to bottom when content is short */}
      <div className={cn('flex-1 flex flex-col min-h-0', className)}>
        {/* Scrollable content - adjust padding based on footer visibility */}
        <div
          className={cn(
            'flex-1 overflow-auto px-4 sm:px-6 md:px-8',
            showFooter ? 'pb-20' : 'pb-4 sm:pb-6'
          )}
          data-page-scroll-container
        >
          <div className={cn('mx-auto w-full', maxWidthClass[maxWidth])}>
            {children}
          </div>
        </div>
      </div>
      {/* Fixed footer with glass morphism effect */}
      {showFooter && (
        <div className="fixed bottom-0 left-0 right-0 bg-gradient-to-t from-background via-background/95 to-background/80 backdrop-blur-md border-t border-border/30">
          <div className={cn('w-full px-4 py-3 sm:px-6 sm:py-4 md:px-8', maxWidthClass[maxWidth], className)}>
            {footer}
          </div>
        </div>
      )}
    </div>
  )
}

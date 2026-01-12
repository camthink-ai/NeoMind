import { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export interface PageLayoutProps {
  children: ReactNode
  maxWidth?: 'md' | 'lg' | 'xl' | '2xl' | 'full'
  className?: string
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
 * Provides consistent padding and max-width across all pages.
 *
 * @example
 * <PageLayout maxWidth="xl">
 *   <PageHeader title="My Page" />
 *   <div>Content here</div>
 * </PageLayout>
 */
export function PageLayout({ children, maxWidth = 'full', className }: PageLayoutProps) {
  return (
    <div className={cn('mx-auto p-4 md:p-6 pb-20 space-y-6', maxWidthClass[maxWidth], className)}>
      {children}
    </div>
  )
}

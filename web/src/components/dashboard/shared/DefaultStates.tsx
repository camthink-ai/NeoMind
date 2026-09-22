/**
 * Unified Default States for Dashboard Components
 *
 * Provides consistent loading, empty, and error states across all dashboard components.
 * These states replace the entire card container.
 */

import { useLayoutEffect, useRef, useState } from 'react'
import { cn } from '@/lib/utils'
import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { dashboardComponentSize, dashboardCardBase, type DashboardComponentSize } from '@/design-system/tokens/size'

export interface StateProps {
  size?: DashboardComponentSize
  className?: string
}

// Icon sizes based on component size
const ICON_SIZE: Record<DashboardComponentSize, string> = {
  xs: 'h-6 w-6',
  sm: 'h-8 w-8',
  md: 'h-12 w-12',
  lg: 'h-16 w-16',
}

/** Below this card height the stacked icon + two-line states cram. */
const COMPACT_THRESHOLD_PX = 132

/**
 * Dashboard grid cells vary per widget — the same state must collapse to a
 * single quiet line on short cards instead of cramming icon + two lines.
 * Exported for hand-rolled card states outside this module (e.g. the
 * registry's unknown-component / error fallbacks).
 */
export function useCompactCard<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [compact, setCompact] = useState(false)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => {
      setCompact(entry.contentRect.height < COMPACT_THRESHOLD_PX)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  return { ref, compact }
}

/**
 * Empty state with message (optional icon)
 * Replaces the entire card container.
 */
export interface EmptyStateProps extends StateProps {
  icon?: React.ReactNode
  message?: string
  subMessage?: string
  action?: React.ReactNode
}

export function EmptyState({
  size = 'md',
  className,
  icon,
  message = 'No Data Available',
  subMessage,
  action,
}: EmptyStateProps) {
  const sizeConfig = dashboardComponentSize[size]
  const { ref, compact } = useCompactCard<HTMLDivElement>()

  if (compact) {
    // Short card: one quiet line — forced-small icon + message; the
    // subMessage survives as the hover title, the action stays inline.
    return (
      <div
        ref={ref}
        title={subMessage}
        className={cn(
          dashboardCardBase,
          'flex-row items-center justify-center gap-2 px-3 min-h-full w-full',
          className
        )}
      >
        {icon && (
          <span className="flex h-4 w-4 shrink-0 items-center justify-center text-muted-foreground [&>svg]:h-4 [&>svg]:w-4">
            {icon}
          </span>
        )}
        <p className="truncate text-xs font-medium text-muted-foreground">{message}</p>
        {action}
      </div>
    )
  }

  return (
    <div ref={ref} className={cn(
      dashboardCardBase,
      'flex flex-col items-center justify-center gap-3 min-h-full w-full',
      sizeConfig.padding,
      className
    )}>
      {icon && (
        <div className={cn('flex items-center justify-center text-muted-foreground', ICON_SIZE[size])}>
          {icon}
        </div>
      )}
      <div className="text-center">
        <p className="text-muted-foreground text-sm font-medium">{message}</p>
        {subMessage && (
          <p className="text-muted-foreground text-xs mt-1">{subMessage}</p>
        )}
        {action && <div className="mt-3">{action}</div>}
      </div>
    </div>
  )
}

/**
 * Error state with message and optional retry (no icon)
 * Replaces the entire card container.
 */
export interface ErrorStateProps extends StateProps {
  message?: string
  subMessage?: string
  onRetry?: () => void
  retryLabel?: string
}

export function ErrorState({
  size = 'md',
  className,
  message = 'Failed to Load Data',
  subMessage,
  onRetry,
  retryLabel = 'Retry',
}: ErrorStateProps) {
  const sizeConfig = dashboardComponentSize[size]
  const { ref, compact } = useCompactCard<HTMLDivElement>()

  if (compact) {
    // Short card: error line + inline retry icon-button; the subMessage
    // survives as the hover title.
    return (
      <div
        ref={ref}
        title={subMessage}
        className={cn(
          dashboardCardBase,
          'flex-row items-center justify-center gap-2 px-3 min-h-full w-full',
          className
        )}
      >
        <p className="truncate text-xs font-medium text-error">{message}</p>
        {onRetry && (
          <Button
            variant="outline"
            size="icon-sm"
            className="h-7 w-7 shrink-0"
            onClick={onRetry}
            aria-label={retryLabel}
          >
            <RefreshCw className="h-3.5 w-3.5" />
          </Button>
        )}
      </div>
    )
  }

  return (
    <div ref={ref} className={cn(
      dashboardCardBase,
      'flex flex-col items-center justify-center gap-2 min-h-full w-full',
      sizeConfig.padding,
      className
    )}>
      <div className="text-center">
        <p className="text-error text-sm font-medium">{message}</p>
        {subMessage && (
          <p className="text-muted-foreground text-xs mt-1">{subMessage}</p>
        )}
        {onRetry && (
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 mt-3"
            onClick={onRetry}
          >
            <RefreshCw className="h-4 w-4" />
            {retryLabel}
          </Button>
        )}
      </div>
    </div>
  )
}

/**
 * Loading state with skeleton
 * Replaces the entire card container.
 */
export function LoadingState({ size = 'md', className }: StateProps) {
  const sizeConfig = dashboardComponentSize[size]

  return (
    <div className={cn(dashboardCardBase, 'h-full flex flex-col', sizeConfig.padding, className)}>
      <div className={cn('flex-1 w-full rounded-md bg-muted animate-pulse min-h-8')} />
    </div>
  )
}

/**
 * Combined state renderer that handles all states
 */
export interface StateConfigProps {
  loading?: boolean
  error?: boolean
  empty?: boolean
  size?: DashboardComponentSize
  className?: string
  // Empty state props
  emptyIcon?: React.ReactNode
  emptyMessage?: string
  emptySubMessage?: string
  emptyAction?: React.ReactNode
  // Error state props
  errorMessage?: string
  errorSubMessage?: string
  onRetry?: () => void
  retryLabel?: string
  // Children to render when all states are false
  children: React.ReactNode
}

export function StateContainer({
  loading,
  error,
  empty,
  size = 'md',
  className,
  emptyIcon,
  emptyMessage,
  emptySubMessage,
  emptyAction,
  errorMessage,
  errorSubMessage,
  onRetry,
  retryLabel,
  children,
}: StateConfigProps) {
  if (loading) {
    return <LoadingState size={size} className={className} />
  }

  if (error) {
    return (
      <ErrorState
        size={size}
        className={className}
        message={errorMessage}
        subMessage={errorSubMessage}
        onRetry={onRetry}
        retryLabel={retryLabel}
      />
    )
  }

  if (empty) {
    return (
      <EmptyState
        size={size}
        className={className}
        icon={emptyIcon}
        message={emptyMessage}
        subMessage={emptySubMessage}
        action={emptyAction}
      />
    )
  }

  return <>{children}</>
}

// Re-export all states
export const DefaultStates = {
  Loading: LoadingState,
  Empty: EmptyState,
  Error: ErrorState,
  Container: StateContainer,
}

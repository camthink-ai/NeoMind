/**
 * Unified Default States for Dashboard Components
 *
 * Provides consistent loading, empty, and error states across all dashboard components.
 * These states replace the entire card container.
 */

import { cn } from '@/lib/utils'
import { Skeleton } from '@/components/ui/skeleton'
import { RefreshCw, AlertCircle } from 'lucide-react'
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

  return (
    <div className={cn(
      dashboardCardBase,
      'flex flex-col items-center justify-center gap-3 bg-muted/30 min-h-full',
      sizeConfig.padding,
      className
    )}>
      {icon && (
        <div className={cn('flex items-center justify-center text-muted-foreground/60', ICON_SIZE[size])}>
          {icon}
        </div>
      )}
      <div className="text-center">
        <p className="text-muted-foreground text-sm font-medium">{message}</p>
        {subMessage && (
          <p className="text-muted-foreground/50 text-xs mt-1">{subMessage}</p>
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

  return (
    <div className={cn(
      dashboardCardBase,
      'flex flex-col items-center justify-center gap-2 bg-muted/30 min-h-full',
      sizeConfig.padding,
      className
    )}>
      <div className="text-center">
        <p className="text-destructive/80 text-sm font-medium">{message}</p>
        {subMessage && (
          <p className="text-muted-foreground/50 text-xs mt-1">{subMessage}</p>
        )}
        {onRetry && (
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 mt-3"
            onClick={onRetry}
          >
            <RefreshCw className="h-3.5 w-3.5" />
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
  const height = size === 'sm' ? 60 : size === 'md' ? 80 : 120

  return (
    <div className={cn(dashboardCardBase, sizeConfig.padding, className)}>
      <Skeleton className={cn('w-full rounded-md')} style={{ height }} />
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

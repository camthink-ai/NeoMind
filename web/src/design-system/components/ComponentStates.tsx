/**
 * Component States - Loading, Error, Empty
 *
 * Reusable state components for dashboard widgets.
 */

import { AlertCircle, RefreshCw, Inbox, FileQuestion } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

// ============================================================================
// Loading State
// ============================================================================

export interface ComponentLoadingProps {
  variant?: 'default' | 'skeleton' | 'spinner'
  size?: 'sm' | 'md' | 'lg'
  className?: string
  showCard?: boolean
}

export function ComponentLoading({
  variant = 'skeleton',
  size = 'md',
  className,
  showCard = true,
}: ComponentLoadingProps) {
  const sizeClasses = {
    sm: 'h-24',
    md: 'h-32',
    lg: 'h-48',
  }

  const content = (
    <div className={cn('flex items-center justify-center', sizeClasses[size], className)}>
      {variant === 'spinner' && (
        <RefreshCw className="w-6 h-6 text-muted-foreground animate-spin" />
      )}
      {variant === 'skeleton' && (
        <div className="w-full h-full flex flex-col gap-2 p-4">
          <Skeleton className="h-4 w-1/3" />
          <Skeleton className="flex-1" />
        </div>
      )}
      {variant === 'default' && (
        <div className="flex flex-col items-center gap-2 text-muted-foreground">
          <RefreshCw className="w-5 h-5 animate-spin" />
          <span className="text-sm">Loading...</span>
        </div>
      )}
    </div>
  )

  if (showCard) {
    return (
      <Card className={className}>
        <CardContent className="p-0">{content}</CardContent>
      </Card>
    )
  }

  return content
}

// ============================================================================
// Error State
// ============================================================================

export interface ComponentErrorProps {
  title?: string
  message?: string
  onRetry?: () => void
  retrying?: boolean
  className?: string
  showCard?: boolean
  compact?: boolean
}

export function ComponentError({
  title = 'Error loading data',
  message = 'Please try again later',
  onRetry,
  retrying = false,
  className,
  showCard = true,
  compact = false,
}: ComponentErrorProps) {
  const content = (
    <div className={cn(
      'flex flex-col items-center justify-center gap-3 text-center p-6',
      compact && 'p-4',
      className
    )}>
      <div className="w-10 h-10 rounded-full bg-destructive/10 flex items-center justify-center">
        <AlertCircle className="w-5 h-5 text-destructive" />
      </div>

      <div className="space-y-1">
        <h3 className="font-medium text-sm text-foreground">{title}</h3>
        {!compact && (
          <p className="text-xs text-muted-foreground max-w-[200px]">{message}</p>
        )}
      </div>

      {onRetry && (
        <Button
          variant="outline"
          size="sm"
          onClick={onRetry}
          disabled={retrying}
          className="gap-2"
        >
          <RefreshCw className={cn('w-3 h-3', retrying && 'animate-spin')} />
          Retry
        </Button>
      )}
    </div>
  )

  if (showCard) {
    return (
      <Card className={className}>
        <CardContent className="p-0">{content}</CardContent>
      </Card>
    )
  }

  return content
}

// ============================================================================
// Empty State
// ============================================================================

export interface ComponentEmptyProps {
  title?: string
  message?: string
  icon?: 'inbox' | 'file' | 'none'
  action?: {
    label: string
    onClick: () => void
  }
  className?: string
  showCard?: boolean
  compact?: boolean
}

export function ComponentEmpty({
  title = 'No data',
  message = 'There is no data to display',
  icon = 'inbox',
  action,
  className,
  showCard = true,
  compact = false,
}: ComponentEmptyProps) {
  const icons = {
    inbox: Inbox,
    file: FileQuestion,
    none: null,
  }

  const IconComponent = icons[icon]

  const content = (
    <div className={cn(
      'flex flex-col items-center justify-center gap-3 text-center p-6',
      compact && 'p-4',
      className
    )}>
      {IconComponent && (
        <div className="w-10 h-10 rounded-full bg-muted flex items-center justify-center">
          <IconComponent className="w-5 h-5 text-muted-foreground" />
        </div>
      )}

      <div className="space-y-1">
        <h3 className="font-medium text-sm text-foreground">{title}</h3>
        {!compact && (
          <p className="text-xs text-muted-foreground max-w-[200px]">{message}</p>
        )}
      </div>

      {action && (
        <Button variant="outline" size="sm" onClick={action.onClick}>
          {action.label}
        </Button>
      )}
    </div>
  )

  if (showCard) {
    return (
      <Card className={className}>
        <CardContent className="p-0">{content}</CardContent>
      </Card>
    )
  }

  return content
}

// ============================================================================
// Combined State Component
// ============================================================================

export interface ComponentStateWrapperProps {
  loading?: boolean
  error?: Error | string | null
  empty?: boolean
  emptyMessage?: string
  onRetry?: () => void
  retrying?: boolean
  children: React.ReactNode
  className?: string
  showCard?: boolean
  compact?: boolean
}

/**
 * Renders appropriate state based on loading/error/empty conditions
 */
export function ComponentStateWrapper({
  loading = false,
  error = null,
  empty = false,
  emptyMessage,
  onRetry,
  retrying = false,
  children,
  className,
  showCard = true,
  compact = false,
}: ComponentStateWrapperProps) {
  if (loading) {
    return <ComponentLoading showCard={showCard} className={className} />
  }

  if (error) {
    return (
      <ComponentError
        title="Error"
        message={typeof error === 'string' ? error : error.message}
        onRetry={onRetry}
        retrying={retrying}
        showCard={showCard}
        compact={compact}
        className={className}
      />
    )
  }

  if (empty) {
    return (
      <ComponentEmpty
        title="No data"
        message={emptyMessage}
        showCard={showCard}
        compact={compact}
        className={className}
      />
    )
  }

  return <>{children}</>
}

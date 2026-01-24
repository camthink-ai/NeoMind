/**
 * Dashboard Card Wrapper
 *
 * Consistent card wrapper for all dashboard components.
 * Handles header, content, footer, and loading/error states.
 */

import { forwardRef, useState } from 'react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { ComponentLoading, ComponentError, ComponentEmpty } from './ComponentStates'
import { Settings2, X, MoreVertical } from 'lucide-react'
import { Button } from '@/components/ui/button'

// ============================================================================
// Types
// ============================================================================

export interface DashboardCardProps {
  // Content
  title?: string
  description?: string
  children: React.ReactNode

  // Loading/Error/Empty states
  loading?: boolean
  error?: Error | string | null
  empty?: boolean
  emptyMessage?: string
  onRetry?: () => void

  // Display options
  showCard?: boolean
  showHeader?: boolean
  showFooter?: boolean
  headerAction?: React.ReactNode
  footer?: React.ReactNode

  // Styling
  variant?: 'default' | 'outlined' | 'elevated' | 'flat'
  size?: 'sm' | 'md' | 'lg'
  className?: string
  contentClassName?: string
  headerClassName?: string

  // Interaction
  clickable?: boolean
  onClick?: () => void

  // Edit mode
  editMode?: boolean
  onEdit?: () => void
  onDelete?: () => void
  onConfigure?: () => void
}

// ============================================================================
// Component
// ============================================================================

export const DashboardCard = forwardRef<HTMLDivElement, DashboardCardProps>(
  (props, ref) => {
    const {
      title,
      description,
      children,
      loading = false,
      error,
      empty,
      emptyMessage,
      onRetry,
      showCard = true,
      showHeader = true,
      showFooter = false,
      headerAction,
      variant = 'default',
      size = 'md',
      className,
      contentClassName,
      headerClassName,
      clickable = false,
      onClick,
      editMode = false,
      onEdit,
      onDelete,
      onConfigure,
    } = props

    const [showMenu, setShowMenu] = useState(false)

    // Size classes
    const sizeClasses = {
      sm: 'p-3',
      md: 'p-4',
      lg: 'p-6',
    }

    // Variant classes
    const variantClasses = {
      default: 'bg-card/50 backdrop-blur border-0 shadow-sm',
      outlined: 'bg-card border shadow-none',
      elevated: 'bg-card border-0 shadow-md',
      flat: 'bg-transparent border-0 shadow-none',
    }

    // Handle loading/error/empty states
    if (loading) {
      return (
        <Card
          ref={ref}
          className={cn(
            'overflow-hidden',
            variantClasses[variant],
            sizeClasses[size],
            clickable && 'cursor-pointer hover:shadow-md transition-shadow',
            className
          )}
          onClick={clickable ? onClick : undefined}
        >
          {showHeader && title && (
            <CardHeader className={cn('pb-2', headerClassName)}>
              <Skeleton className="h-4 w-24" />
            </CardHeader>
          )}
          <CardContent className={contentClassName}>
            <ComponentLoading variant="default" size={size} showCard={false} />
          </CardContent>
        </Card>
      )
    }

    if (error) {
      return (
        <Card
          ref={ref}
          className={cn(
            'overflow-hidden',
            variantClasses[variant],
            sizeClasses[size],
            className
          )}
        >
          {showHeader && title && (
            <CardHeader className={cn('pb-2', headerClassName)}>
              <CardTitle className="text-sm">{title}</CardTitle>
            </CardHeader>
          )}
          <CardContent className={contentClassName}>
            <ComponentError
              title="Error"
              message={typeof error === 'string' ? error : error.message}
              onRetry={onRetry}
              showCard={false}
            />
          </CardContent>
        </Card>
      )
    }

    if (empty) {
      return (
        <Card
          ref={ref}
          className={cn(
            'overflow-hidden',
            variantClasses[variant],
            sizeClasses[size],
            clickable && 'cursor-pointer hover:shadow-md transition-shadow',
            className
          )}
          onClick={clickable ? onClick : undefined}
        >
          {showHeader && title && (
            <CardHeader className={cn('pb-2', headerClassName)}>
              <CardTitle className="text-sm">{title}</CardTitle>
            </CardHeader>
          )}
          <CardContent className={contentClassName}>
            <ComponentEmpty
              title="No data"
              message={emptyMessage}
              showCard={false}
            />
          </CardContent>
        </Card>
      )
    }

    // Render content
    const content = (
      <>
        {showHeader && (title || headerAction || editMode) && (
          <CardHeader className={cn('pb-2 flex flex-row items-center justify-between space-y-0', headerClassName)}>
            <div className="flex-1 min-w-0">
              {title && (
                <CardTitle className="text-sm font-medium truncate">{title}</CardTitle>
              )}
              {description && (
                <p className="text-xs text-muted-foreground truncate">{description}</p>
              )}
            </div>

            <div className="flex items-center gap-1">
              {headerAction}
              {editMode && (
                <div className="flex items-center gap-1">
                  {onConfigure && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={(e) => {
                        e.stopPropagation()
                        onConfigure()
                      }}
                    >
                      <Settings2 className="h-3.5 w-3.5" />
                    </Button>
                  )}
                  {onDelete && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={(e) => {
                        e.stopPropagation()
                        onDelete()
                      }}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </div>
              )}
            </div>
          </CardHeader>
        )}

        <CardContent className={cn(sizeClasses[size], contentClassName)}>
          {children}
        </CardContent>

        {showFooter && props.footer}
      </>
    )

    if (!showCard) {
      return (
        <div
          ref={ref}
          className={cn('relative', clickable && 'cursor-pointer', className)}
          onClick={clickable ? onClick : undefined}
        >
          {content}
        </div>
      )
    }

    return (
      <Card
        ref={ref}
        className={cn(
          'overflow-hidden transition-all hover:shadow-md',
          variantClasses[variant],
          clickable && 'cursor-pointer',
          className
        )}
        onClick={clickable ? onClick : undefined}
      >
        {content}
      </Card>
    )
  }
)

DashboardCard.displayName = 'DashboardCard'

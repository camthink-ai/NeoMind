import { Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface LoadingStateProps {
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'page'
  text?: string
  className?: string
}

/**
 * Loading state component with spinner
 *
 * @example
 * <LoadingState /> // Default size, no text
 * <LoadingState size="lg" text="加载中..." />
 * <LoadingState variant="page" text="加载中..." /> // Full page centered loading
 */
export function LoadingState({ size = 'md', variant = 'default', text, className }: LoadingStateProps) {
  const sizeClasses = {
    sm: 'w-4 h-4',
    md: 'w-6 h-6',
    lg: 'w-8 h-8',
  }

  const textSizeClasses = {
    sm: 'text-xs',
    md: 'text-sm',
    lg: 'text-base',
  }

  if (variant === 'page') {
    return (
      <div className={cn('flex flex-col items-center justify-center gap-4 py-16 min-h-[200px]', className)}>
        <Loader2 className={cn('animate-spin text-muted-foreground', 'w-10 h-10')} />
        {text && (
          <p className="text-sm text-muted-foreground">{text}</p>
        )}
      </div>
    )
  }

  return (
    <div className={cn('flex flex-col items-center justify-center gap-3 py-8', className)}>
      <Loader2 className={cn('animate-spin text-muted-foreground', sizeClasses[size])} />
      {text && (
        <p className={cn('text-muted-foreground', textSizeClasses[size])}>{text}</p>
      )}
    </div>
  )
}

/**
 * Inline loading spinner for buttons
 */
export function LoadingSpinner({ className }: { className?: string }) {
  return <Loader2 className={cn('h-4 w-4 animate-spin', className)} />
}

/**
 * Skeleton loader for cards
 */
export function CardSkeleton({ count = 1 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="animate-pulse">
          <div className="h-24 rounded-lg bg-muted/50" />
        </div>
      ))}
    </>
  )
}

/**
 * Table row skeleton loader
 */
export function TableRowSkeleton({ cells = 4, rows = 3 }: { cells?: number; rows?: number }) {
  return (
    <>
      {Array.from({ length: rows }).map((_, i) => (
        <tr key={i}>
          {Array.from({ length: cells }).map((_, j) => (
            <td key={j} className="p-4">
              <div className="h-4 animate-pulse rounded bg-muted/50" />
            </td>
          ))}
        </tr>
      ))}
    </>
  )
}

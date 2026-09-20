/**
 * Component Skeleton Card
 *
 * Shows a loading skeleton while component data is being fetched.
 * Prevents empty card flicker and provides immediate visual feedback.
 */

import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'

export interface ComponentSkeletonProps {
  className?: string
  title?: string
  height?: number
  style?: React.CSSProperties
}

export function ComponentSkeleton({
  className,
  title: _title = 'Loading...',
  height: _height = 200,
  style
}: ComponentSkeletonProps) {
  return (
    <Card className={cn('w-full h-full flex flex-col p-4', className)} style={style}>
      {/* Title skeleton */}
      <div className="flex items-center justify-between mb-4">
        <Skeleton className="h-5 w-32" />
        <Skeleton className="h-4 w-4 rounded" />
      </div>

      {/* Content skeleton */}
      <div className="flex-1 flex flex-col justify-center space-y-3">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-3/4" />
        <Skeleton className="h-4 w-1/2" />

        {/* Chart/Data skeleton */}
        <div className="flex items-end justify-between h-32 mt-4 space-x-2">
          {[...Array(8)].map((_, i) => (
            <Skeleton key={i} className="flex-1" style={{ height: `${30 + Math.random() * 50}%` }} />
          ))}
        </div>
      </div>
    </Card>
  )
}

/**
 * Mini skeleton for smaller components
 */
export function MiniComponentSkeleton({ className, style }: { className?: string; style?: React.CSSProperties }) {
  return (
    <Card className={cn('w-full h-full flex flex-col p-3', className)} style={style}>
      <div className="flex items-center justify-between mb-2">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-3 w-3 rounded" />
      </div>
      <Skeleton className="h-8 w-16 mb-2" />
      <Skeleton className="h-2 w-full" />
    </Card>
  )
}

/**
 * Chart skeleton for chart components
 */
export function ChartSkeleton({ className, style }: { className?: string; style?: React.CSSProperties }) {
  return (
    <Card className={cn('w-full h-full flex flex-col p-4', className)} style={style}>
      <div className="flex items-center justify-between mb-4">
        <Skeleton className="h-5 w-32" />
        <Skeleton className="h-4 w-4 rounded" />
      </div>

      {/* Chart area skeleton */}
      <div className="flex-1 flex items-end justify-between h-40 space-x-1 px-2">
        {[...Array(12)].map((_, i) => (
          <Skeleton
            key={i}
            className="flex-1"
            style={{ height: `${20 + Math.random() * 60}%` }}
          />
        ))}
      </div>

      {/* X-axis labels skeleton */}
      <div className="flex justify-between mt-2 px-2">
        {[...Array(6)].map((_, i) => (
          <Skeleton key={i} className="h-3 w-8" />
        ))}
      </div>
    </Card>
  )
}
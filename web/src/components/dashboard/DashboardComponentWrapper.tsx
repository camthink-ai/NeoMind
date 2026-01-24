/**
 * Dashboard Component Wrapper
 *
 * Provides consistent styling and layout for all dashboard components.
 * Uses unified design tokens for size, padding, and spacing.
 */

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import { dashboardComponentSize, dashboardCardBase, dashboardCardContent, dashboardCardHeader } from '@/design-system/tokens/size'

export type ComponentPadding = 'none' | 'sm' | 'md' | 'lg'

export interface DashboardComponentWrapperProps {
  title?: string
  description?: string
  children: React.ReactNode
  className?: string
  contentClassName?: string
  showCard?: boolean
  headerClassName?: string
  padding?: ComponentPadding
  actions?: React.ReactNode
  size?: keyof typeof dashboardComponentSize
}

const paddingMap = {
  none: '',
  sm: dashboardComponentSize.sm.padding,
  md: dashboardComponentSize.md.padding,
  lg: dashboardComponentSize.lg.padding,
}

/**
 * Unified wrapper for all dashboard components.
 * Ensures consistent:
 * - Card appearance (border, shadow, background)
 * - Padding and spacing based on size prop
 * - Title/header positioning
 * - Flex layout for proper container filling
 */
export function DashboardComponentWrapper({
  title,
  description,
  children,
  className,
  contentClassName,
  showCard = true,
  headerClassName,
  padding = 'md',
  actions,
  size = 'md',
}: DashboardComponentWrapperProps) {
  const sizeConfig = dashboardComponentSize[size]
  const hasHeader = title || description || actions

  const content = (
    <>
      {hasHeader && (
        <CardHeader className={cn(
          dashboardCardHeader,
          sizeConfig.headerPadding,
          paddingMap[padding === 'none' ? 'md' : padding],
          headerClassName
        )}>
          <div className="flex flex-col gap-0.5 min-w-0 flex-1">
            {title && (
              <CardTitle className={cn(sizeConfig.titleText, 'font-medium leading-none truncate')}>
                {title}
              </CardTitle>
            )}
            {description && (
              <p className={cn(sizeConfig.labelText, 'text-muted-foreground truncate')}>
                {description}
              </p>
            )}
          </div>
          {actions && (
            <div className={cn('flex items-center gap-1', sizeConfig.itemGap)}>
              {actions}
            </div>
          )}
        </CardHeader>
      )}
      <CardContent className={cn(
        hasHeader ? 'pt-0' : paddingMap[padding === 'none' ? 'md' : padding],
        !hasHeader && paddingMap[padding === 'none' ? 'md' : padding],
        'flex-1 min-h-0',
        contentClassName
      )}>
        {children}
      </CardContent>
    </>
  )

  if (!showCard) {
    return (
      <div className={cn(dashboardCardContent, className)}>
        {content}
      </div>
    )
  }

  return (
    <Card className={cn(dashboardCardBase, className)}>
      {content}
    </Card>
  )
}

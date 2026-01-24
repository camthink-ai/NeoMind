/**
 * Alert Banner Component
 *
 * Prominent alert message banner.
 * Supports different severity levels with icons.
 */

import { useState } from 'react'
import { AlertCircle, AlertTriangle, Info, CheckCircle, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type AlertSeverity = 'info' | 'success' | 'warning' | 'error'

export interface AlertBannerProps {
  dataSource?: DataSource
  title?: string
  message?: string
  severity?: AlertSeverity
  dismissible?: boolean
  icon?: boolean
  variant?: 'default' | 'filled' | 'outlined' | 'subtle'
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const sizeClasses = {
  sm: { padding: 'p-3', text: 'text-sm', icon: 'w-4 h-4' },
  md: { padding: 'p-4', text: 'text-base', icon: 'w-5 h-5' },
  lg: { padding: 'p-5', text: 'text-lg', icon: 'w-6 h-6' },
}

const severityConfig = {
  info: { icon: Info, color: 'text-blue-500', bgColor: 'bg-blue-50 dark:bg-blue-950', borderColor: 'border-blue-500' },
  success: { icon: CheckCircle, color: 'text-green-500', bgColor: 'bg-green-50 dark:bg-green-950', borderColor: 'border-green-500' },
  warning: { icon: AlertTriangle, color: 'text-amber-500', bgColor: 'bg-amber-50 dark:bg-amber-950', borderColor: 'border-amber-500' },
  error: { icon: AlertCircle, color: 'text-red-500', bgColor: 'bg-red-50 dark:bg-red-950', borderColor: 'border-red-500' },
}

export function AlertBanner({
  dataSource,
  title: propTitle,
  message: propMessage,
  severity = 'info',
  dismissible = false,
  icon = true,
  variant = 'default',
  size = 'md',
  showCard = false,
  className,
}: AlertBannerProps) {
  const { data: titleData } = useDataSource<string>(dataSource, { fallback: propTitle })
  const { data: messageData } = useDataSource<string>(undefined, { fallback: propMessage })

  const title = titleData ?? propTitle
  const message = messageData ?? propMessage
  const sizes = sizeClasses[size]
  const config = severityConfig[severity]
  const IconComponent = config.icon

  const [dismissed, setDismissed] = useState(false)

  if (dismissed) return null

  const variantClasses = {
    default: cn(config.bgColor, `border-l-4 ${config.borderColor}`),
    filled: cn(config.bgColor, `border ${config.borderColor}`),
    outlined: cn('bg-background', `border-2 ${config.borderColor}`),
    subtle: 'bg-muted border border-border',
  }

  const content = (
    <div
      className={cn(
        'flex items-start gap-3 rounded-lg',
        variantClasses[variant],
        sizes.padding,
        className
      )}
    >
      {icon && <IconComponent className={cn(config.color, sizes.icon, 'flex-shrink-0')} />}

      <div className="flex-1 min-w-0">
        {title && (
          <div className={cn('font-semibold', config.color, sizes.text)}>
            {title}
          </div>
        )}
        {message && (
          <div className={cn('text-foreground', sizes.text)}>
            {message}
          </div>
        )}
      </div>

      {dismissible && (
        <button
          onClick={() => setDismissed(true)}
          className={cn('flex-shrink-0 opacity-70 hover:opacity-100', sizes.icon)}
        >
          <X className="w-full h-full" />
        </button>
      )}
    </div>
  )

  if (showCard) {
    return (
      <DashboardComponentWrapper
        title={title || 'Alert'}
        showCard={true}
        padding="none"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}


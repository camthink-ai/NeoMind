/**
 * ValidationBanner Component
 *
 * Unified validation banner for automation dialogs.
 * Displays errors and warnings with dismiss option.
 */

import { ReactNode } from 'react'
import { Button } from '@/components/ui/button'
import { AlertCircle, AlertTriangle, Info, X } from 'lucide-react'
import { cn } from '@/lib/utils'

export type ValidationBannerType = 'error' | 'warning' | 'info'

export interface ValidationBannerProps {
  type?: ValidationBannerType
  errors?: string[]
  warnings?: string[]
  info?: string[]
  onDismiss?: () => void
  className?: string
  showIcon?: boolean
}

const bannerStyles = {
  error: 'bg-destructive/10 border-destructive/20 text-destructive',
  warning: 'bg-amber-500/10 border-amber-500/20 text-amber-700 dark:text-amber-400',
  info: 'bg-blue-500/10 border-blue-500/20 text-blue-700 dark:text-blue-400',
}

const icons = {
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
}

const titles = {
  error: '请完善以下配置',
  warning: '注意事项',
  info: '提示',
}

export function ValidationBanner({
  type = 'error',
  errors = [],
  warnings = [],
  info = [],
  onDismiss,
  className,
  showIcon = true,
}: ValidationBannerProps) {
  const messages = type === 'error' ? errors : type === 'warning' ? warnings : info

  if (messages.length === 0) return null

  const Icon = icons[type]
  const title = titles[type]

  return (
    <div
      className={cn(
        'mx-4 md:mx-8 mt-4 md:mt-6 px-4 py-3 border rounded-lg flex items-start gap-3',
        bannerStyles[type],
        className
      )}
    >
      {showIcon && (
        <Icon className="h-5 w-5 shrink-0 mt-0.5" />
      )}
      <div className="flex-1 min-w-0">
        <p className="font-medium text-sm">
          {title}
        </p>
        <ul className="mt-2 space-y-1 text-sm">
          {messages.map((msg, i) => (
            <li key={i} className="flex items-start gap-2">
              <span className="shrink-0">•</span>
              <span className="break-words">{msg}</span>
            </li>
          ))}
        </ul>
      </div>
      {onDismiss && (
        <Button
          variant="ghost"
          size="icon"
          className="shrink-0 h-6 w-6"
          onClick={onDismiss}
        >
          <X className="h-4 w-4" />
        </Button>
      )}
    </div>
  )
}

/**
 * Compact version for inline validation display
 */
export interface ValidationBadgeProps {
  count: number
  type?: ValidationBannerType
  onClick?: () => void
}

export function ValidationBadge({ count, type = 'error', onClick }: ValidationBadgeProps) {
  if (count === 0) return null

  const styles = {
    error: 'bg-destructive text-destructive-foreground',
    warning: 'bg-amber-500 text-white',
    info: 'bg-blue-500 text-white',
  }

  return (
    <button
      onClick={onClick}
      className={cn(
        'px-2 py-0.5 rounded-full text-xs font-medium flex items-center gap-1',
        styles[type],
        onClick && 'cursor-pointer hover:opacity-80'
      )}
    >
      {count} {type === 'error' ? '错误' : type === 'warning' ? '警告' : '提示'}
    </button>
  )
}

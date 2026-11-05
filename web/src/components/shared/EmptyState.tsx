import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import { cn } from '@/lib/utils'
import {
  Server,
  FileJson,
  Zap,
  Workflow,
  Bell,
  Terminal,
  Home,
  Package,
  Users,
  Settings,
  Inbox,
  Search,
} from 'lucide-react'

export type EmptyStateIconType =
  | 'device'
  | 'deviceType'
  | 'rule'
  | 'workflow'
  | 'alert'
  | 'command'
  | 'scenario'
  | 'plugin'
  | 'user'
  | 'settings'
  | 'search'
  | 'inbox'
  | 'custom'

const iconMap: Record<Exclude<EmptyStateIconType, 'custom'>, React.ReactNode> = {
  device: <Server />,
  deviceType: <FileJson />,
  rule: <Zap />,
  workflow: <Workflow />,
  alert: <Bell />,
  command: <Terminal />,
  scenario: <Home />,
  plugin: <Package />,
  user: <Users />,
  settings: <Settings />,
  search: <Search />,
  inbox: <Inbox />,
}

interface EmptyStateBaseProps {
  title: string
  description?: string
  className?: string
}

interface EmptyStateAction {
  label: string
  onClick: () => void
  variant?: 'default' | 'outline' | 'secondary' | 'ghost' | 'destructive'
  icon?: React.ReactNode
}

/** A visual is reserved for states with a concrete recovery or creation action. */
export type EmptyStateProps = EmptyStateBaseProps & (
  | { icon?: undefined; action?: EmptyStateAction }
  | { icon: React.ReactNode | EmptyStateIconType; action: EmptyStateAction }
)

/**
 * Empty state component for when there's no data to display
 *
 * @example
 * <EmptyState
 *   icon="device"
 *   title="暂无设备"
 *   description="点击下方按钮添加您的第一个设备"
 *   action={{ label: '添加设备', onClick: handleAdd }}
 * />
 */
export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  const iconContent = typeof icon === 'string' ? iconMap[icon as keyof typeof iconMap] : icon

  return (
    <div className={cn('flex w-full flex-1 flex-col items-center justify-center px-4 py-10 text-center', className)}>
      {iconContent && (
        <div className="mb-3 flex h-8 w-8 items-center justify-center text-muted-foreground [&>svg]:h-7 [&>svg]:w-7">
          {iconContent}
        </div>
      )}
      <h3 className="text-base font-medium text-foreground">{title}</h3>
      {description && (
        <p className="mt-1.5 max-w-sm text-sm leading-relaxed text-muted-foreground">{description}</p>
      )}
      {action && (
        <Button
          onClick={action.onClick}
          variant={action.variant || 'default'}
          className="mt-4"
        >
          {action.icon && <span className="mr-2">{action.icon}</span>}
          {action.label}
        </Button>
      )}
    </div>
  )
}

/**
 * Card-framed empty state — the page-level convention. An <EmptyState> left
 * floating in whitespace breaks the page rhythm next to the carded table /
 * list chrome (ResponsiveTable wraps its mobile empty in exactly this Card,
 * and agents.tsx made the same call), so custom mobile list branches should
 * render their empties through this wrapper. ResponsiveTable passes its
 * emptyState prop ALREADY framed on mobile — do not double-wrap there.
 */
export function EmptyStateCard({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <Card className={cn('overflow-hidden border-border', className)}>
      <div className="p-8">{children}</div>
    </Card>
  )
}

/**
 * Compact empty state for smaller spaces
 */
export function EmptyStateCompact({
  title,
  description,
  icon,
}: {
  title: string
  description?: string
  icon?: React.ReactNode
}) {
  return (
    <div className="flex w-full flex-1 flex-col items-center justify-center py-6 text-center">
      {icon && (
        <div className="mb-2 flex h-6 w-6 items-center justify-center text-muted-foreground [&>svg]:h-5 [&>svg]:w-5">
          {icon}
        </div>
      )}
      <p className="text-sm font-medium text-muted-foreground">{title}</p>
      {description && (
        <p className="mt-1 text-sm text-muted-foreground">{description}</p>
      )}
    </div>
  )
}

/**
 * Inline empty state for tables and lists
 */
export function EmptyStateInline({
  title,
  colSpan = 1,
  className,
}: {
  title: string
  colSpan?: number
  className?: string
}) {
  return (
    <tr>
      <td colSpan={colSpan} className={cn('py-8', className)}>
        <div className="flex items-center justify-center text-center">
          <p className="text-sm text-muted-foreground">{title}</p>
        </div>
      </td>
    </tr>
  )
}

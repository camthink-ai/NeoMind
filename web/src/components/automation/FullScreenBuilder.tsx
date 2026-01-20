/**
 * FullScreenBuilder - A consistent full-screen container for automation builders
 *
 * Provides a unified layout pattern for Rules, Workflows, and Transforms builders.
 *
 * Layout Structure:
 * ┌─────────────────────────────────────────────────────────────────┐
 * │ Header: Title | Description | Status | Actions (Close)           │
 * ├─────────────────────────────────────────────────────────────────┤
 * │ ┌─────────────────────┬───────────────────────────────────────┐ │
 * │ │                     │                                       │ │
 * │ │   Main Content      │   Side Panel (Preview/Help/Tips)     │ │
 * │ │   (Scrollable)      │   (Scrollable)                       │ │
 * │ │                     │                                       │ │
 * │ │                     │                                       │ │
 * └─────────────────────┴───────────────────────────────────────┘ │
 * ├─────────────────────────────────────────────────────────────────┤
 * │ Footer: Validation Status | Actions (Cancel/Save)               │
 * └─────────────────────────────────────────────────────────────────┘
 */

import { ReactNode, useState } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { X, ChevronDown, ChevronRight, Info, AlertTriangle, CheckCircle } from 'lucide-react'

export interface BuilderHeaderProps {
  title: string
  description?: string
  icon?: ReactNode
  status?: 'draft' | 'valid' | 'invalid' | 'saving'
  onClose?: () => void
  actions?: ReactNode
  badge?: ReactNode
}

export interface BuilderFooterProps {
  isValid: boolean
  isDirty: boolean
  isSaving: boolean
  saveLabel?: string
  onCancel: () => void
  onSave: () => void | Promise<void>
  validationMessage?: string
  leftActions?: ReactNode
}

export interface FullScreenBuilderProps {
  open: boolean
  onClose: () => void

  // Header
  title: string
  description?: string
  icon?: ReactNode
  headerActions?: ReactNode
  badge?: ReactNode

  // Main content
  children: ReactNode

  // Side panel (optional)
  sidePanel?: {
    content: ReactNode
    title?: string
    collapsible?: boolean
  }

  // Footer
  isValid: boolean
  isDirty: boolean
  isSaving: boolean
  saveLabel?: string
  onSave: () => void | Promise<void>
  validationMessage?: string
  footerLeftActions?: ReactNode
}

export function FullScreenBuilder({
  open,
  onClose,
  title,
  description,
  icon,
  headerActions,
  badge,
  children,
  sidePanel,
  isValid,
  isDirty,
  isSaving,
  saveLabel,
  onSave,
  validationMessage,
  footerLeftActions,
}: FullScreenBuilderProps) {
  if (!open) return null

  const content = (
    <div className="fixed inset-0 z-[100] bg-background">
      <div className="flex h-full w-full max-w-[1920px] mx-auto flex-col">
        {/* Header */}
        <BuilderHeader
          title={title}
          description={description}
          icon={icon}
          badge={badge}
          onClose={onClose}
          actions={headerActions}
        />

        {/* Main Content Area */}
        <div className="flex flex-1 min-h-0 overflow-hidden">
          {/* Main Content */}
          <ScrollArea className={cn('flex-1', sidePanel && 'border-r')}>
            <div className="p-6 max-w-5xl mx-auto">
              {children}
            </div>
          </ScrollArea>

          {/* Side Panel */}
          {sidePanel && (
            <div
              className={cn(
                'w-80 border-l bg-muted/30 flex flex-col',
                'transition-all duration-300'
              )}
            >
              {sidePanel.title && (
                <div className="px-4 py-3 border-b font-medium text-sm">
                  {sidePanel.title}
                </div>
              )}
              <ScrollArea className="flex-1">
                <div className="p-4">{sidePanel.content}</div>
              </ScrollArea>
            </div>
          )}
        </div>

        {/* Footer */}
        <BuilderFooter
          isValid={isValid}
          isDirty={isDirty}
          isSaving={isSaving}
          saveLabel={saveLabel}
          onCancel={onClose}
          onSave={onSave}
          validationMessage={validationMessage}
          leftActions={footerLeftActions}
        />
      </div>
    </div>
  )

  return createPortal(content, document.body)
}

/**
 * Builder Header Component
 */
function BuilderHeader({
  title,
  description,
  icon,
  badge,
  onClose,
  actions,
}: BuilderHeaderProps) {
  return (
    <div className="flex items-center justify-between px-6 py-4 border-b bg-background">
      <div className="flex items-center gap-4 flex-1 min-w-0">
        {icon && <div className="flex-shrink-0 text-muted-foreground">{icon}</div>}
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h1 className="text-lg font-semibold truncate">{title}</h1>
            {badge}
          </div>
          {description && (
            <p className="text-sm text-muted-foreground truncate">{description}</p>
          )}
        </div>
      </div>

      <div className="flex items-center gap-2">
        {actions}
        <Button variant="ghost" size="icon" onClick={onClose} className="flex-shrink-0">
          <X className="h-5 w-5" />
        </Button>
      </div>
    </div>
  )
}

/**
 * Builder Footer Component
 */
function BuilderFooter({
  isValid,
  isDirty,
  isSaving,
  saveLabel = 'common:save',
  onCancel,
  onSave,
  validationMessage,
  leftActions,
}: BuilderFooterProps) {
  return (
    <div className="flex items-center justify-between px-6 py-4 border-t bg-background">
      <div className="flex items-center gap-4 flex-1">
        {leftActions}

        {/* Validation Status */}
        <div className="flex items-center gap-2 text-sm">
          {isDirty && (
            <span className="text-muted-foreground">
              • {typeof saveLabel === 'string' ? saveLabel : '保存'}{' '}
            </span>
          )}
          {validationMessage && (
            <span className={cn('flex items-center gap-1', isValid ? 'text-muted-foreground' : 'text-destructive')}>
              {validationMessage}
            </span>
          )}
        </div>
      </div>

      {/* Action Buttons */}
      <div className="flex items-center gap-2">
        <Button variant="outline" onClick={onCancel} disabled={isSaving}>
          Cancel
        </Button>
        <Button onClick={onSave} disabled={!isValid || isSaving}>
          {isSaving ? (
            <>
              <span className="animate-spin mr-2">⏳</span>
              Saving...
            </>
          ) : (
            saveLabel
          )}
        </Button>
      </div>
    </div>
  )
}

/**
 * Section Component - For organizing content into sections
 */
export interface BuilderSectionProps {
  title: string
  description?: string
  children: ReactNode
  icon?: ReactNode
  actions?: ReactNode
  className?: string
  collapsible?: boolean
  defaultCollapsed?: boolean
}

export function BuilderSection({
  title,
  description,
  children,
  icon,
  actions,
  className,
  collapsible,
  defaultCollapsed = false,
}: BuilderSectionProps) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed)

  return (
    <div className={cn('border rounded-lg overflow-hidden', className)}>
      <div
        className={cn(
          'flex items-center justify-between px-4 py-3 bg-muted/30',
          collapsible && 'cursor-pointer hover:bg-muted/40'
        )}
        onClick={() => collapsible && setCollapsed(!collapsed)}
      >
        <div className="flex items-center gap-2">
          {icon}
          <div>
            <h3 className="font-medium text-sm">{title}</h3>
            {description && <p className="text-xs text-muted-foreground">{description}</p>}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {actions}
          {collapsible && (
            <Button variant="ghost" size="icon" className="h-6 w-6">
              {collapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
            </Button>
          )}
        </div>
      </div>
      {!collapsed && <div className="p-4 space-y-4">{children}</div>}
    </div>
  )
}

/**
 * Form Grid Component - For consistent form layouts
 */
export interface FormGridProps {
  children: ReactNode
  columns?: 1 | 2 | 3 | 4
  className?: string
}

export function FormGrid({ children, columns = 2, className }: FormGridProps) {
  const gridCols = {
    1: 'grid-cols-1',
    2: 'grid-cols-2',
    3: 'grid-cols-3',
    4: 'grid-cols-4',
  }

  return (
    <div className={cn('grid gap-4', gridCols[columns], className)}>{children}</div>
  )
}

/**
 * Tip Card Component - For showing tips and hints in the side panel
 */
export interface TipCardProps {
  title: string
  children: ReactNode
  icon?: ReactNode
  variant?: 'info' | 'warning' | 'success'
}

export function TipCard({ title, children, icon, variant = 'info' }: TipCardProps) {
  const variantStyles = {
    info: 'bg-blue-50 border-blue-200 text-blue-900 dark:bg-blue-950 dark:border-blue-800 dark:text-blue-100',
    warning: 'bg-yellow-50 border-yellow-200 text-yellow-900 dark:bg-yellow-950 dark:border-yellow-800 dark:text-yellow-100',
    success: 'bg-green-50 border-green-200 text-green-900 dark:bg-green-950 dark:border-green-800 dark:text-green-100',
  }

  const defaultIcons = {
    info: <Info className="h-4 w-4" />,
    warning: <AlertTriangle className="h-4 w-4" />,
    success: <CheckCircle className="h-4 w-4" />,
  }

  return (
    <div className={cn('border rounded-lg p-3 text-sm', variantStyles[variant])}>
      <div className="flex items-start gap-2">
        {icon || defaultIcons[variant]}
        <div className="flex-1 min-w-0">
          <p className="font-medium mb-1">{title}</p>
          <p className="text-xs opacity-80">{children}</p>
        </div>
      </div>
    </div>
  )
}

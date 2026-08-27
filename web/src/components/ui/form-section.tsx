/**
 * FormSection Component
 *
 * A collapsible section component for organizing form fields.
 * Lightweight design - uses title and spacing for visual hierarchy.
 */

import { ReactNode, useState, useCallback, forwardRef } from 'react'
import { ChevronDown } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useIsMobile } from '@/hooks/useMobile'

export interface FormSectionProps {
  /** Section title */
  title?: string
  /** Optional description below title */
  description?: string
  /** Whether the section is collapsible */
  collapsible?: boolean
  /** Whether the section starts expanded */
  defaultExpanded?: boolean
  /** Whether the section is currently expanded (controlled) */
  expanded?: boolean
  /** Callback when expansion state changes */
  onExpandedChange?: (expanded: boolean) => void
  /** Section content */
  children: ReactNode
  /** Additional class name */
  className?: string
  /** Additional class name for content area */
  contentClassName?: string
  /** Test ID for testing */
  testId?: string
}

/**
 * FormSection - A lightweight collapsible form section
 */
export const FormSection = forwardRef<HTMLDivElement, FormSectionProps>(({
  title,
  description,
  collapsible = false,
  defaultExpanded = true,
  expanded: controlledExpanded,
  onExpandedChange,
  children,
  className,
  contentClassName,
  testId,
}, ref) => {
  const isMobile = useIsMobile()
  const [internalExpanded, setInternalExpanded] = useState(defaultExpanded)

  const isExpanded = controlledExpanded !== undefined ? controlledExpanded : internalExpanded

  const handleToggle = useCallback(() => {
    if (!collapsible) return
    const newState = !isExpanded
    if (controlledExpanded === undefined) {
      setInternalExpanded(newState)
    }
    onExpandedChange?.(newState)
  }, [collapsible, isExpanded, controlledExpanded, onExpandedChange])

  return (
    <div
      ref={ref}
      className={cn('flex flex-col', className)}
      data-testid={testId}
    >
      {/* Header */}
      <div
        className={cn(
          'flex items-center gap-2 py-2',
          collapsible && 'cursor-pointer select-none'
        )}
        onClick={collapsible ? handleToggle : undefined}
        role={collapsible ? 'button' : undefined}
        tabIndex={collapsible ? 0 : undefined}
        onKeyDown={collapsible ? (e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            handleToggle()
          }
        } : undefined}
        aria-expanded={collapsible ? isExpanded : undefined}
      >
        {(title || description) && (
          <div className="flex-1 min-w-0">
            {title && (
              <h3 className={cn(
                'text-sm font-medium text-muted-foreground',
                isMobile && 'text-base'
              )}>
                {title}
              </h3>
            )}
            {description && (
              <p className="text-xs text-muted-foreground mt-0.5">
                {description}
              </p>
            )}
          </div>
        )}

        {collapsible && (
          <ChevronDown className={cn(
            'h-4 w-4 text-muted-foreground transition-transform duration-normal shrink-0',
            isExpanded && 'rotate-180'
          )} />
        )}
      </div>

      {/* Content */}
      <div
        className={cn(
          'overflow-hidden transition-all duration-normal ease-out',
          isExpanded ? 'max-h-[2000px] opacity-100' : 'max-h-0 opacity-0'
        )}
      >
        <div className={cn(
          'space-y-4',
          isMobile ? 'pt-3' : 'pt-2',
          contentClassName
        )}>
          {children}
        </div>
      </div>
    </div>
  )
})

FormSection.displayName = 'FormSection'

/**
 * FormSectionGroup - Container for multiple FormSections
 */
export interface FormSectionGroupProps {
  children: ReactNode
  className?: string
  /** Compact mode with reduced spacing */
  compact?: boolean
}

export function FormSectionGroup({
  children,
  className,
  compact = false,
}: FormSectionGroupProps) {
  const isMobile = useIsMobile()

  return (
    <div className={cn(
      'flex flex-col space-y-4',
      compact && 'space-y-3',
      className
    )}>
      {children}
    </div>
  )
}

export default FormSection

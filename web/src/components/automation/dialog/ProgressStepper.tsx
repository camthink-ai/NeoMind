/**
 * ProgressStepper Component
 *
 * Unified step progress indicator for automation wizards.
 * Glassmorphism design with clean visual hierarchy.
 */

import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useIsMobile } from '@/hooks/useMobile'

export type StepStatus = 'pending' | 'active' | 'completed'

export interface Step {
  id: string
  label: string
  shortLabel?: string
  icon?: ReactNode
  optional?: boolean
}

export interface ProgressStepperProps {
  steps: Step[]
  currentStep: string
  completedSteps: string[]
  onStepClick?: (stepId: string) => void
  className?: string
  /** Layout mode */
  mode?: 'horizontal' | 'vertical'
}

// ============================================================================
// Step Dot Component
// ============================================================================

function StepDot({
  status,
  icon,
  size = 'default',
}: {
  status: StepStatus
  icon?: ReactNode
  size?: 'default' | 'large'
}) {
  const sizeClasses = size === 'large'
    ? 'w-10 h-10 text-sm'
    : 'w-8 h-8 text-xs'

  return (
    <div
      className={cn(
        'rounded-full flex items-center justify-center font-medium transition-all shrink-0',
        sizeClasses,
        status === 'pending' && 'bg-muted text-muted-foreground border border-border',
        status === 'active' && [
          'bg-primary text-primary-foreground border border-primary',
          size === 'large' ? 'ring-4 ring-primary' : 'ring-2 ring-primary',
        ],
        status === 'completed' && 'bg-muted text-primary border border-border'
      )}
    >
      {status === 'completed' ? (
        <Check className={size === 'large' ? 'h-4 w-4' : 'h-4 w-4'} />
      ) : icon ? (
        icon
      ) : null}
    </div>
  )
}

// ============================================================================
// Vertical Stepper (Sidebar)
// ============================================================================

export interface VerticalStepperProps {
  steps: Step[]
  currentStep: string
  completedSteps: string[]
  onStepClick?: (stepId: string) => void
  className?: string
}

export function VerticalStepper({
  steps,
  currentStep,
  completedSteps,
  onStepClick,
  className,
}: VerticalStepperProps) {
  const getStepStatus = (step: Step): StepStatus => {
    if (completedSteps.includes(step.id)) return 'completed'
    if (currentStep === step.id) return 'active'
    return 'pending'
  }

  const currentIndex = steps.findIndex(s => s.id === currentStep)

  // Dot center offset: btn py-3(12px) + w-8/2(16px) = 28px from button top
  const dotCenterOffset = '1.75rem'

  return (
    <nav className={cn('p-3', className)}>
      <div className="relative">
        {/* Continuous color track — starts at first dot center, ends at last dot center */}
        <div
          className="absolute w-0.5 rounded-full"
          style={{
            left: 'calc(0.5rem + 16px)', // btn px-2 + w-8/2
            top: dotCenterOffset,
            bottom: dotCenterOffset,
          }}
        >
          {/* Background track */}
          <div className="absolute inset-0 bg-border rounded-full" />
          {/* Filled portion (completed steps) */}
          {currentIndex > 0 && (
            <div
              className="absolute top-0 left-0 right-0 bg-primary rounded-full transition-all"
              style={{
                height: `${(currentIndex / (steps.length - 1)) * 100}%`
              }}
            />
          )}
        </div>

        {/* Step items */}
        {steps.map((step, _index) => {
          const status = getStepStatus(step)
          const isClickable = onStepClick && (completedSteps.includes(step.id) || status === 'active')

          return (
            <button
              key={step.id}
              onClick={() => isClickable && onStepClick?.(step.id)}
              disabled={!isClickable}
              className={cn(
                'relative w-full text-left transition-all',
                'flex items-center gap-2.5 px-2 py-3 rounded-lg',
                // Active: muted background
                status === 'active' && 'bg-muted-30',
                // Completed: subtle hover
                status === 'completed' && isClickable && 'hover:bg-muted-20',
                !isClickable && status === 'pending' && 'cursor-default'
              )}
            >
              <StepDot status={status} icon={step.icon} size="default" />
              <div className="flex-1 min-w-0">
                <div className={cn(
                  'text-xs font-medium truncate',
                  status === 'active' && 'text-foreground',
                  status !== 'active' && 'text-muted-foreground'
                )}>
                  {step.shortLabel || step.label}
                </div>
              </div>
            </button>
          )
        })}
      </div>
    </nav>
  )
}

// ============================================================================
// Horizontal Stepper (Top Bar)
// ============================================================================

export interface HorizontalStepperProps {
  steps: Step[]
  currentStep: string
  completedSteps: string[]
  onStepClick?: (stepId: string) => void
  className?: string
}

export function HorizontalStepper({
  steps,
  currentStep,
  completedSteps,
  onStepClick,
  className,
}: HorizontalStepperProps) {
  const { t } = useTranslation(['common'])
  const isMobile = useIsMobile()

  const getStepStatus = (step: Step): StepStatus => {
    if (completedSteps.includes(step.id)) return 'completed'
    if (currentStep === step.id) return 'active'
    return 'pending'
  }

  const currentIndex = steps.findIndex(s => s.id === currentStep)

  return (
    <div
      className={cn(
        'shrink-0 px-5 md:px-6 py-4',
        'border-b border-border',
        'bg-black/[0.02] dark:bg-white/[0.02]',
        className
      )}
    >
      <div className="flex items-center justify-center gap-2 md:gap-3 overflow-x-auto">
        {steps.map((step, i) => {
          const status = getStepStatus(step)
          const isClickable = onStepClick && (completedSteps.includes(step.id) || i === currentIndex + 1)
          const isPast = i < currentIndex

          return (
            <div key={step.id} className="flex items-center">
              <button
                onClick={() => isClickable && onStepClick?.(step.id)}
                disabled={!isClickable}
                className={cn(
                  'flex flex-col items-center gap-2 transition-all',
                  isClickable && 'hover:opacity-80 cursor-pointer',
                  !isClickable && 'cursor-default'
                )}
              >
                <StepDot status={status} icon={step.icon} />
                <span
                  className={cn(
                    'text-xs whitespace-nowrap',
                    status === 'active' && 'font-semibold text-foreground',
                    status === 'completed' && 'text-foreground',
                    status === 'pending' && 'text-muted-foreground'
                  )}
                >
                  {isMobile && step.shortLabel ? step.shortLabel : step.label}
                  {step.optional && (
                    <span className="text-muted-foreground ml-1">
                      ({t('common:optional')})
                    </span>
                  )}
                </span>
              </button>

              {/* Connector */}
              {i < steps.length - 1 && (
                <div
                  className={cn(
                    'h-0.5 w-8 md:w-12 mx-2 rounded-full transition-colors shrink-0',
                    (status === 'completed' || isPast) ? 'bg-primary' : 'bg-border'
                  )}
                />
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

// ============================================================================
// Main ProgressStepper Component
// ============================================================================

export function ProgressStepper({
  steps,
  currentStep,
  completedSteps,
  onStepClick,
  className,
  mode = 'horizontal',
}: ProgressStepperProps) {
  if (mode === 'vertical') {
    return (
      <VerticalStepper
        steps={steps}
        currentStep={currentStep}
        completedSteps={completedSteps}
        onStepClick={onStepClick}
        className={className}
      />
    )
  }

  return (
    <HorizontalStepper
      steps={steps}
      currentStep={currentStep}
      completedSteps={completedSteps}
      onStepClick={onStepClick}
      className={className}
    />
  )
}

/**
 * ProgressStepper Component
 *
 * Unified step progress indicator for automation wizards.
 * Shows visual progress through configuration steps.
 */

import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Check } from 'lucide-react'
import { cn } from '@/lib/utils'

export type StepStatus = 'pending' | 'active' | 'completed'

export interface Step {
  id: string
  label: string
  icon?: ReactNode
  optional?: boolean
}

export interface ProgressStepperProps {
  steps: Step[]
  currentStep: string
  completedSteps: string[]
  onStepClick?: (stepId: string) => void
  className?: string
}

function StepDot({
  status,
  icon,
  optional,
}: {
  status: StepStatus
  icon?: ReactNode
  optional?: boolean
}) {
  return (
    <div
      className={cn(
        'w-8 h-8 rounded-full flex items-center justify-center text-sm font-medium transition-all shrink-0',
        status === 'pending' && 'bg-muted text-muted-foreground',
        status === 'active' && 'bg-primary text-primary-foreground ring-4 ring-primary/20',
        status === 'completed' && 'bg-green-500 text-white'
      )}
    >
      {status === 'completed' ? (
        <Check className="h-4 w-4" />
      ) : icon ? (
        icon
      ) : (
        '?'
      )}
    </div>
  )
}

function StepConnector({ active }: { active: boolean }) {
  return (
    <div
      className={cn(
        'h-0.5 w-8 md:w-16 transition-colors shrink-0',
        active ? 'bg-green-500' : 'bg-muted'
      )}
    />
  )
}

export function ProgressStepper({
  steps,
  currentStep,
  completedSteps,
  onStepClick,
  className,
}: ProgressStepperProps) {
  const { t } = useTranslation(['common'])
  const getStepStatus = (step: Step): StepStatus => {
    if (completedSteps.includes(step.id)) return 'completed'
    if (currentStep === step.id) return 'active'
    return 'pending'
  }

  const currentIndex = steps.findIndex(s => s.id === currentStep)

  return (
    <div className={cn('px-4 md:px-6 py-4 border-b bg-muted/30 shrink-0', className)}>
      <div className="flex items-center justify-center gap-0 md:gap-1 overflow-x-auto">
        {steps.map((step, i) => {
          const status = getStepStatus(step)
          const isActive = status === 'completed' || i <= currentIndex
          const isClickable = onStepClick && (completedSteps.includes(step.id) || i === currentIndex + 1)

          return (
            <div key={step.id} className="flex items-center">
              <div className="flex flex-col items-center gap-1">
                <button
                  onClick={() => isClickable && onStepClick?.(step.id)}
                  disabled={!isClickable}
                  className={cn(
                    'flex flex-col items-center gap-1 transition-all',
                    isClickable && 'hover:opacity-80',
                    !isClickable && 'cursor-default'
                  )}
                >
                  <StepDot status={status} icon={step.icon} optional={step.optional} />
                  <span
                    className={cn(
                      'text-xs whitespace-nowrap hidden md:block',
                      status === 'active' && 'font-semibold text-foreground',
                      status === 'completed' && 'text-foreground',
                      status === 'pending' && 'text-muted-foreground'
                    )}
                  >
                    {step.label}
                    {step.optional && (
                      <span className="text-muted-foreground/60 ml-1">({t('common:optional')})</span>
                    )}
                  </span>
                </button>
              </div>
              {i < steps.length - 1 && (
                <StepConnector active={isActive && completedSteps.includes(step.id)} />
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}

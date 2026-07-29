// TaskProgress component - displays multi-turn task progress
import { CheckCircle2, Circle, Loader2, AlertCircle } from "lucide-react"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { cn } from "@/lib/utils"
import { useTranslation } from "react-i18next"

export type TaskStepType = "gather_info" | "confirm" | "execute"
export type TaskStatus = "in_progress" | "completed" | "cancelled" | "failed"

export interface TaskStep {
  step_type: TaskStepType
  description: string
  prompt?: string
  details?: string
  action?: string
}

export interface TaskProgressData {
  task_id: string
  current_step: number
  total_steps: number
  steps: TaskStep[]
  status: TaskStatus
}

interface TaskProgressProps {
  task: TaskProgressData
  className?: string
}

// Helper function to get step type config
function getStepTypeConfig(stepType: TaskStepType, t: (key: string) => string) {
  const configs: Record<TaskStepType, { label: string; color: string }> = {
    gather_info: {
      label: t("taskProgress.steps.collecting"),
      color: "text-info"
    },
    confirm: {
      label: t("taskProgress.steps.confirming"),
      color: "text-warning"
    },
    execute: {
      label: t("taskProgress.steps.executing"),
      color: "text-success"
    }
  }
  return configs[stepType]
}

export function TaskProgress({ task, className }: TaskProgressProps) {
  const { t } = useTranslation("chat")
  const progressPercent = task.total_steps > 0
    ? (task.current_step / task.total_steps) * 100
    : 0

  const getStepStatus = (index: number): "pending" | "active" | "completed" => {
    if (index < task.current_step) return "completed"
    if (index === task.current_step && task.status === "in_progress") return "active"
    return "pending"
  }

  const getStepIcon = (status: "pending" | "active" | "completed") => {
    switch (status) {
      case "completed":
        return <CheckCircle2 className="h-4 w-4 text-success" />
      case "active":
        return <Loader2 className="h-4 w-4 text-info animate-spin" />
      case "pending":
        return <Circle className="h-4 w-4 text-muted-foreground" />
    }
  }

  return (
    <div className={cn("task-progress space-y-3", className)}>
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{t("taskProgress.title")}</span>
          <Badge variant="outline" className="text-xs">
            {task.current_step}/{task.total_steps}
          </Badge>
        </div>
        {task.status === "completed" && (
          <Badge className="bg-success hover:bg-success text-xs">
            {t("taskProgress.status.completed")}
          </Badge>
        )}
        {task.status === "failed" && (
          <Badge variant="destructive" className="text-xs">
            {t("taskProgress.status.failed")}
          </Badge>
        )}
        {task.status === "cancelled" && (
          <Badge variant="secondary" className="text-xs">
            {t("taskProgress.status.cancelled")}
          </Badge>
        )}
      </div>

      {/* Progress Bar */}
      <div className="space-y-1">
        <Progress value={progressPercent} className="h-2" />
        <div className="text-xs text-muted-foreground text-right">
          {t("taskProgress.percentComplete", { percent: Math.round(progressPercent) })}
        </div>
      </div>

      {/* Steps List */}
      <div className="space-y-2">
        {task.steps.map((step, index) => {
          const status = getStepStatus(index)
          const config = getStepTypeConfig(step.step_type, t)

          return (
            <div
              key={index}
              className={cn(
                "flex items-start gap-3 p-3 rounded-md border transition-colors",
                status === "active" && "bg-info-light border-info",
                status === "completed" && "bg-success-light border-success-light",
                status === "pending" && "bg-muted-30 border-border"
              )}
            >
              {/* Step Icon */}
              <div className={cn(
                "flex-shrink-0 mt-0.5",
                config.color
              )}>
                {getStepIcon(status)}
              </div>

              {/* Step Content */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{step.description}</span>
                  <Badge variant="outline" className="text-xs">
                    {config.label}
                  </Badge>
                </div>

                {step.details && (
                  <p className="text-xs text-muted-foreground mt-1">
                    {step.details}
                  </p>
                )}

                {status === "active" && step.prompt && (
                  <div className="mt-2 p-2 rounded bg-background border border-info-light">
                    <p className="text-xs text-info">{step.prompt}</p>
                  </div>
                )}
              </div>

              {/* Step Number Badge */}
              <div className={cn(
                "flex-shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium",
                status === "completed" && "bg-success text-primary-foreground",
                status === "active" && "bg-info text-primary-foreground",
                status === "pending" && "bg-muted text-muted-foreground"
              )}>
                {index + 1}
              </div>
            </div>
          )
        })}
      </div>

      {/* Status Messages */}
      {task.status === "in_progress" && (
        <div className="flex items-center gap-2 p-3 rounded-md bg-info-light border border-info">
          <Loader2 className="h-4 w-4 text-info animate-spin" />
          <p className="text-sm text-info">
            {t("taskProgress.currentStep", { step: task.current_step + 1 })}
          </p>
        </div>
      )}

      {task.status === "completed" && (
        <div className="flex items-center gap-2 p-3 rounded-md bg-success-light border border-success-light">
          <CheckCircle2 className="h-4 w-4 text-success" />
          <p className="text-sm text-success">
            {t("taskProgress.success")}
          </p>
        </div>
      )}

      {task.status === "failed" && (
        <div className="flex items-center gap-2 p-3 rounded-md bg-error-light border border-error">
          <AlertCircle className="h-4 w-4 text-error" />
          <p className="text-sm text-error">
            {t("taskProgress.failed")}
          </p>
        </div>
      )}
    </div>
  )
}

// Compact horizontal progress indicator
export function CompactTaskProgress({ task, className }: TaskProgressProps) {
  const progressPercent = task.total_steps > 0
    ? (task.current_step / task.total_steps) * 100
    : 0

  return (
    <div className={cn("flex items-center gap-3", className)}>
      {/* Progress indicators */}
      <div className="flex items-center gap-1">
        {task.steps.map((_, index) => {
          const isCompleted = index < task.current_step
          const isCurrent = index === task.current_step && task.status === "in_progress"

          return (
            <div
              key={index}
              className={cn(
                "w-2 h-2 rounded-full transition-colors",
                isCompleted && "bg-success",
                isCurrent && "bg-info animate-pulse",
                !isCompleted && !isCurrent && "bg-muted-30"
              )}
            />
          )
        })}
      </div>

      {/* Text */}
      <span className="text-xs text-muted-foreground">
        {task.current_step}/{task.total_steps}
      </span>

      {/* Progress bar */}
      <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden">
        <div
          className={cn(
            "h-full transition-all duration-300",
            task.status === "completed" && "bg-success",
            task.status === "failed" && "bg-error",
            task.status !== "completed" && task.status !== "failed" && "bg-info"
          )}
          style={{ width: `${progressPercent}%` }}
        />
      </div>
    </div>
  )
}

// Step-by-step wizard view
export function TaskWizard({ task, className }: TaskProgressProps) {
  const { t } = useTranslation("chat")
  const currentStepData = task.steps[task.current_step]

  return (
    <div className={cn("task-wizard space-y-4", className)}>
      {/* Progress Header */}
      <div className="flex items-center justify-between">
        <h3 className="font-medium">{t("taskProgress.createAutomation")}</h3>
        <CompactTaskProgress task={task} />
      </div>

      {/* Current Step */}
      {task.status === "in_progress" && currentStepData && (
        <div className="p-4 rounded-lg border bg-card">
          <div className="flex items-center gap-2 mb-2">
            <span className="text-xs text-muted-foreground">
              {t("taskProgress.stepLabel", { step: task.current_step + 1 })}
            </span>
            <Badge variant="outline" className="text-xs">
              {getStepTypeConfig(currentStepData.step_type, t).label}
            </Badge>
          </div>

          <h4 className="font-medium mb-2">{currentStepData.description}</h4>

          {currentStepData.prompt && (
            <p className="text-sm text-muted-foreground mb-4">
              {currentStepData.prompt}
            </p>
          )}

          {currentStepData.details && (
            <div className="p-3 rounded-md text-sm">
              {currentStepData.details}
            </div>
          )}
        </div>
      )}

      {/* Completed state */}
      {task.status === "completed" && (
        <div className="p-6 rounded-md border bg-success-light text-center">
          <CheckCircle2 className="h-12 w-12 text-success mx-auto mb-3" />
          <h3 className="font-medium text-success mb-1">{t("taskProgress.status.completed")}</h3>
          <p className="text-sm text-success">
            {t("taskProgress.automationCreated")}
          </p>
        </div>
      )}
    </div>
  )
}

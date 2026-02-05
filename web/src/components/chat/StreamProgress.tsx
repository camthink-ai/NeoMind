import { Progress } from "@/components/ui/progress"
import { Activity, AlertTriangle, Loader2, Sparkles } from "lucide-react"
import { cn } from "@/lib/utils"
import { useTranslation } from "react-i18next"

interface StreamProgressProps {
  elapsed: number
  totalDuration: number
  stage?: string  // Optional stage from backend (collecting, analyzing, executing, etc.)
  warning?: string
  currentStep?: string  // Current execution plan step from Plan events
  className?: string
}

// Map backend stage values to UI labels and icons
function getStageConfig(stage?: string, t?: (key: string) => string) {
  // Normalize stage to lowercase for comparison
  const normalized = stage?.toLowerCase() || ''

  // If no stage provided, default to thinking
  if (!normalized) {
    return {
      label: t ? t('stage.thinking') : '思考中',
      icon: <Activity className="h-4 w-4 animate-pulse" />
    }
  }

  // Map backend stage values to UI
  if (normalized.includes('collect') || normalized.includes('收集')) {
    return {
      label: t ? t('stage.collecting') : '收集中',
      icon: <Activity className="h-4 w-4 animate-pulse" />
    }
  }
  if (normalized.includes('analyz') || normalized.includes('分析') || normalized.includes('thinking') || normalized.includes('思考')) {
    return {
      label: t ? t('stage.analyzing') : '分析中',
      icon: <Sparkles className="h-4 w-4 animate-pulse" />
    }
  }
  if (normalized.includes('execut') || normalized.includes('tool') || normalized.includes('执行') || normalized.includes('工具')) {
    return {
      label: t ? t('stage.executing') : '执行中',
      icon: <Loader2 className="h-4 w-4 animate-spin" />
    }
  }
  if (normalized.includes('generat') || normalized.includes('生成')) {
    return {
      label: t ? t('stage.generating') : '生成中',
      icon: <Loader2 className="h-4 w-4 animate-spin" />
    }
  }

  // Default: use the stage value as-is
  return {
    label: stage,
    icon: <Loader2 className="h-4 w-4 animate-spin" />
  }
}

export function StreamProgress({
  elapsed,
  totalDuration,
  stage,
  warning,
  currentStep,
  className
}: StreamProgressProps) {
  const { t } = useTranslation("chat")
  const progress = Math.min((elapsed / totalDuration) * 100, 100)

  const stageConfig = getStageConfig(stage, t)
  const isNearTimeout = progress > 80
  const barColor = isNearTimeout ? 'bg-yellow-500' : 'bg-blue-500'

  return (
    <div className={cn(
      "flex items-center gap-3 text-sm text-muted-foreground px-4 py-2 bg-muted/30 rounded-lg",
      className
    )}>
      {stageConfig.icon}
      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between mb-1">
          <span className="font-medium text-foreground">{stageConfig.label}</span>
          <span className="text-xs">{elapsed}s / {totalDuration}s</span>
        </div>
        {currentStep && (
          <p className="text-xs text-muted-foreground truncate mb-1">
            {currentStep}
          </p>
        )}
        <div className="h-1.5 bg-muted rounded-full overflow-hidden">
          <div
            className={cn("h-full transition-all duration-300", barColor)}
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
      {warning && (
        <span className="text-yellow-600 text-xs flex items-center gap-1 shrink-0">
          <AlertTriangle className="h-3 w-3" />
          {warning}
        </span>
      )}
    </div>
  )
}

/**
 * Agent Thinking Panel - Real-time display of agent reasoning steps
 *
 * Shows thinking steps and decisions as they happen during execution.
 */

import { useEffect, useState, useRef } from "react"
import { useTranslation } from "react-i18next"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Brain,
  ChevronRight,
  ChevronDown,
  ChevronUp,
  Loader2,
  Play,
  Zap,
  CheckCircle2,
  Clock,
  FileText,
  X,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { useAgentEvents, type AgentThinkingStep } from "@/hooks/useAgentEvents"
import { formatTimestamp } from "@/lib/utils/format"
import { api } from "@/lib/api"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import type { AgentExecutionDetail } from "@/types"

interface AgentThinkingPanelProps {
  agentId: string
  isExecuting: boolean
}

export function AgentThinkingPanel({ agentId, isExecuting }: AgentThinkingPanelProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const { currentExecution, thinkingSteps, decisions } = useAgentEvents(agentId, {
    enabled: true,
    eventTypes: ['AgentExecutionStarted', 'AgentThinking', 'AgentDecision', 'AgentExecutionCompleted'],
  })

  const [autoScroll, setAutoScroll] = useState(true)

  // Track when we should show the panel
  const [showPanel, setShowPanel] = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const [collapsed, setCollapsed] = useState(false)

  // Fetch execution detail on completion to show conclusion
  const [executionDetail, setExecutionDetail] = useState<AgentExecutionDetail | null>(null)
  const fetchedExecutionId = useRef<string | null>(null)

  useEffect(() => {
    if (
      currentExecution?.status === 'completed' &&
      currentExecution.id &&
      fetchedExecutionId.current !== currentExecution.id
    ) {
      fetchedExecutionId.current = currentExecution.id
      api.getExecution(agentId, currentExecution.id)
        .then(setExecutionDetail)
        .catch((err) => handleError(err, { operation: 'Load execution detail', showToast: false }))
    }
    // Reset when new execution starts
    if (currentExecution?.status === 'running') {
      setExecutionDetail(null)
      fetchedExecutionId.current = null
    }
  }, [currentExecution?.status, currentExecution?.id, agentId, handleError])

  useEffect(() => {
    if (isExecuting && currentExecution && !dismissed) {
      setShowPanel(true)
    }
    // Auto-hide after completion with a delay
    if (!isExecuting && currentExecution?.completed_at) {
      const timer = setTimeout(() => {
        setShowPanel(false)
      }, 15000)
      return () => clearTimeout(timer)
    }
  }, [isExecuting, currentExecution, dismissed])

  // Reset dismissed state when new execution starts
  useEffect(() => {
    if (isExecuting && currentExecution?.status === 'running') {
      setDismissed(false)
      setCollapsed(false)
    }
  }, [isExecuting, currentExecution?.status])

  if (!showPanel || !currentExecution) return null

  const hasContent = thinkingSteps.length > 0 || decisions.length > 0

  // Get a short thinking preview for the collapsed state
  const lastStep = thinkingSteps.length > 0
    ? thinkingSteps[thinkingSteps.length - 1]
    : null
  const thinkingPreview = lastStep?.description || (currentExecution.status === 'running' ? t('agents:thinking.waiting') : '')

  return (
    <div className="shrink-0 border rounded-lg bg-muted-30 overflow-hidden">
      {/* Header - always visible, clickable to toggle */}
      <button
        type="button"
        onClick={() => setCollapsed(!collapsed)}
        className="w-full flex items-center justify-between px-4 py-2 hover:bg-muted-50 transition-colors"
      >
        <div className="flex items-center gap-2 min-w-0 flex-1">
          {currentExecution.status === 'running' ? (
            <Loader2 className="h-4 w-4 animate-spin text-info shrink-0" />
          ) : (
            <CheckCircle2 className={cn(
              "h-4 w-4 shrink-0",
              currentExecution.status === 'completed' ? "text-success" : "text-error"
            )} />
          )}
          <span className="text-sm font-medium shrink-0">
            {currentExecution.status === 'running'
              ? t('agents:thinking.executing')
              : currentExecution.status === 'completed'
                ? t('agents:thinking.completed')
                : t('agents:thinking.failed')
            }
          </span>
          {currentExecution.duration_ms !== undefined && (
            <span className="text-xs text-muted-foreground flex items-center gap-1 shrink-0">
              <Clock className="h-4 w-4" />
              {currentExecution.duration_ms < 1000
                ? `${currentExecution.duration_ms}ms`
                : `${(currentExecution.duration_ms / 1000).toFixed(1)}s`
              }
            </span>
          )}
          {thinkingSteps.length > 0 && (
            <Badge variant="outline" className="text-xs h-5 shrink-0">
              {thinkingSteps.length} {t('agents:thinking.steps')}
            </Badge>
          )}
          {/* Collapsed: show thinking preview */}
          {collapsed && thinkingPreview && (
            <span className="text-xs text-muted-foreground truncate ml-1">
              — {thinkingPreview}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0 ml-2">
          {collapsed ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronUp className="h-4 w-4 text-muted-foreground" />
          )}
          <button
            type="button"
            aria-label={t('common:close')}
            onClick={(e) => {
              e.stopPropagation()
              setShowPanel(false)
              setDismissed(true)
            }}
            className="inline-flex items-center justify-center h-7 w-7 rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </button>

      {/* Content - collapsible */}
      {!collapsed && (
        <ScrollArea className="h-[200px]">
          <div className="p-3 pt-0 space-y-3">
            {!hasContent ? (
              <div className="flex items-center justify-center py-6 text-muted-foreground text-sm">
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                {t('agents:thinking.waiting')}
              </div>
            ) : (
              <>
                {/* Thinking Steps */}
                {thinkingSteps.length > 0 && (
                  <div>
                    <h4 className="text-xs font-semibold text-muted-foreground mb-2 flex items-center gap-1.5">
                      <Brain className="h-4 w-4 text-accent-purple" />
                      {t('agents:thinking.reasoningSteps')}
                    </h4>
                    <div className="space-y-2">
                      {thinkingSteps.map((step, idx) => (
                        <ThinkingStep key={`${step.step_number}-${idx}`} step={step} />
                      ))}
                    </div>
                  </div>
                )}

                {/* Decisions */}
                {decisions.length > 0 && (
                  <div>
                    <h4 className="text-xs font-semibold text-muted-foreground mb-2 flex items-center gap-1.5">
                      <Play className="h-4 w-4 text-success" />
                      {t('agents:thinking.decisions')}
                    </h4>
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-2">
                      {decisions.map((decision, idx) => (
                        <Decision key={idx} decision={decision} />
                      ))}
                    </div>
                  </div>
                )}

                {/* Conclusion (fetched after execution completes) */}
                {executionDetail && (() => {
                  const dp = executionDetail.decision_process
                  const conclusion = dp?.conclusion
                  const confidence = dp?.confidence
                  if (!conclusion && confidence === undefined) return null
                  return (
                    <div>
                      <h4 className="text-xs font-semibold text-muted-foreground mb-2 flex items-center gap-1.5">
                        <FileText className="h-4 w-4 text-success" />
                        {t('agents:memory.conclusion')}
                      </h4>
                      <div className="space-y-2">
                        {conclusion && (
                          <Card className="p-2.5 bg-muted-50">
                            <p className="text-sm">{conclusion}</p>
                          </Card>
                        )}
                        {confidence !== undefined && (
                          <div className="flex items-center justify-between text-sm p-2 bg-muted-50 rounded-lg">
                            <span className="text-xs text-muted-foreground">{t('agents:memory.confidence')}</span>
                            <Badge variant={confidence > 0.7 ? "default" : "secondary"} className="h-5">
                              {(confidence * 100).toFixed(0)}%
                            </Badge>
                          </div>
                        )}
                      </div>
                    </div>
                  )
                })()}
              </>
            )}
          </div>
        </ScrollArea>
      )}
    </div>
  )
}

// ============================================================================
// Sub Components
// ============================================================================

interface ThinkingStepProps {
  step: AgentThinkingStep
}

function ThinkingStep({ step }: ThinkingStepProps) {
  const { t } = useTranslation(['common', 'agents'])

  const getStepTypeColor = (stepType: string) => {
    switch (stepType.toLowerCase()) {
      case 'analysis':
      case 'analyze':
        return 'text-info bg-info-light border-info'
      case 'evaluation':
      case 'evaluate':
        return 'text-accent-orange bg-accent-orange-light border-accent-orange-light'
      case 'planning':
      case 'plan':
        return 'text-accent-purple bg-accent-purple-light border-accent-purple-light'
      case 'execution':
      case 'execute':
        return 'text-success bg-success-light border-success-light'
      default:
        return 'text-muted-foreground bg-muted-50'
    }
  }

  return (
    <div className="flex gap-2">
      <div className="flex flex-col items-center shrink-0">
        <div className="w-6 h-6 rounded-full bg-muted text-primary text-xs flex items-center justify-center font-medium">
          {step.step_number}
        </div>
        {step.step_number < 10 && <div className="w-0.5 flex-1 bg-border min-h-[16px]" />}
      </div>
      <Card className="flex-1 p-2.5">
        <div className="flex items-start justify-between gap-2 mb-1">
          <p className="text-sm flex-1">{step.description}</p>
          <Badge
            variant="outline"
            className={cn("text-xs h-5 shrink-0", getStepTypeColor(step.step_type))}
          >
            {step.step_type}
          </Badge>
        </div>
        {step.details != null && (
          <div className="mt-2 pt-2 border-t">
            <pre className="text-xs bg-muted p-2 rounded overflow-x-auto max-h-20">
              {String(
                typeof step.details === 'string'
                  ? step.details
                  : JSON.stringify(step.details, null, 2)
              )}
            </pre>
          </div>
        )}
        <div className="flex items-center gap-2 mt-2 text-xs text-muted-foreground">
          <span className="flex items-center gap-1">
            <Clock className="h-4 w-4" />
            {formatTimestamp(step.timestamp, false)}
          </span>
        </div>
      </Card>
    </div>
  )
}

interface DecisionProps {
  decision: {
    description: string
    rationale: string
    action: string
    confidence: number
    timestamp: number
  }
}

function Decision({ decision }: DecisionProps) {
  const { t } = useTranslation(['common', 'agents'])

  return (
    <Card className="p-2.5">
      <div className="text-sm font-medium mb-1">{decision.description}</div>
      {decision.rationale && (
        <div className="text-xs text-muted-foreground mb-2">{decision.rationale}</div>
      )}
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5">
          <Zap className="h-4 w-4 text-success" />
          <Badge variant="secondary" className="h-5">
            {decision.action}
          </Badge>
        </div>
        <Badge
          variant={decision.confidence > 0.7 ? "default" : "secondary"}
          className="h-5"
        >
          {(decision.confidence * 100).toFixed(0)}%
        </Badge>
      </div>
    </Card>
  )
}

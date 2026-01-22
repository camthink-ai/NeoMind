import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import { Separator } from "@/components/ui/separator"
import {
  Clock,
  CheckCircle2,
  XCircle,
  AlertCircle,
  Brain,
  Database,
  Play,
  FileText,
  ChevronRight,
} from "lucide-react"
import { api } from "@/lib/api"
import type { AgentExecutionDetail, DecisionProcess, DataCollected, ReasoningStep, Decision, GeneratedReport } from "@/types"

interface ExecutionDetailDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agentId: string
  executionId: string
}

export function ExecutionDetailDialog({
  open,
  onOpenChange,
  agentId,
  executionId,
}: ExecutionDetailDialogProps) {
  const { t } = useTranslation(['common', 'agents'])
  const [execution, setExecution] = useState<AgentExecutionDetail | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (open && agentId && executionId) {
      loadExecution()
    }
  }, [open, agentId, executionId])

  const loadExecution = async () => {
    setLoading(true)
    try {
      const data = await api.getExecution(agentId, executionId)
      setExecution(data)
    } catch (error) {
      console.error('Failed to load execution:', error)
    } finally {
      setLoading(false)
    }
  }

  const formatDate = (timestamp: number) => {
    return new Date(timestamp * 1000).toLocaleString()
  }

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'Completed':
        return <CheckCircle2 className="h-4 w-4 text-green-500" />
      case 'Failed':
        return <XCircle className="h-4 w-4 text-red-500" />
      case 'Running':
        return <Clock className="h-4 w-4 text-blue-500" />
      default:
        return <AlertCircle className="h-4 w-4 text-gray-500" />
    }
  }

  const getStatusBadgeClass = (status: string) => {
    switch (status) {
      case 'Completed': return 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
      case 'Failed': return 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'
      case 'Running': return 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400'
      default: return 'bg-gray-100 text-gray-700 dark:bg-gray-800 dark:text-gray-400'
    }
  }

  if (!execution) {
    return null
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[90vh]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Play className="h-5 w-5 text-primary" />
            {t('agents:execution.title')}
          </DialogTitle>
          <DialogDescription>
            {t('agents:execution.description')}
          </DialogDescription>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-8">
            <Clock className="h-6 w-6 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <ScrollArea className="max-h-[70vh] pr-4">
            <div className="space-y-6">
              {/* Status */}
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  {getStatusIcon(execution.status)}
                  <span className="text-sm font-medium">{t(`agents:executionStatus.${execution.status.toLowerCase()}`)}</span>
                </div>
                <div className="flex items-center gap-4 text-sm text-muted-foreground">
                  <span className="flex items-center gap-1">
                    <Clock className="h-3 w-3" />
                    {formatDate(execution.timestamp)}
                  </span>
                  <span>{execution.duration_ms}ms</span>
                </div>
              </div>

              {/* Trigger Type */}
              <div>
                <span className="text-xs text-muted-foreground">{t('agents:triggerType')}</span>
                <Badge variant="outline" className="ml-2">
                  {execution.trigger_type}
                </Badge>
              </div>

              {execution.error && (
                <Card className="p-4 border-destructive">
                  <div className="flex items-start gap-2 text-destructive">
                    <AlertCircle className="h-4 w-4 mt-0.5" />
                    <div className="text-sm">{execution.error}</div>
                  </div>
                </Card>
              )}

              {execution.decision_process && (
                <>
                  {/* Situation Analysis */}
                  <DecisionProcessSection
                    title={t('agents:memory.situationAnalysis')}
                    icon={<Brain className="h-4 w-4" />}
                  >
                    <p className="text-sm">{execution.decision_process.situation_analysis}</p>
                  </DecisionProcessSection>

                  {/* Data Collected */}
                  {execution.decision_process.data_collected.length > 0 && (
                    <DecisionProcessSection
                      title={t('agents:memory.dataCollected')}
                      subtitle={`${execution.decision_process.data_collected.length} ${t('common:sources')}`}
                      icon={<Database className="h-4 w-4" />}
                    >
                      <div className="space-y-2">
                        {execution.decision_process.data_collected.map((data, idx) => (
                          <DataCollectedItem key={idx} data={data} />
                        ))}
                      </div>
                    </DecisionProcessSection>
                  )}

                  {/* Reasoning Steps */}
                  {execution.decision_process.reasoning_steps.length > 0 && (
                    <DecisionProcessSection
                      title={t('agents:memory.reasoningSteps')}
                      icon={<ChevronRight className="h-4 w-4" />}
                    >
                      <div className="space-y-3">
                        {execution.decision_process.reasoning_steps.map((step, idx) => (
                          <ReasoningStepItem key={idx} step={step} />
                        ))}
                      </div>
                    </DecisionProcessSection>
                  )}

                  {/* Decisions */}
                  {execution.decision_process.decisions.length > 0 && (
                    <DecisionProcessSection
                      title={t('agents:memory.decisions')}
                      icon={<Play className="h-4 w-4" />}
                    >
                      <div className="space-y-2">
                        {execution.decision_process.decisions.map((decision, idx) => (
                          <DecisionItem key={idx} decision={decision} />
                        ))}
                      </div>
                    </DecisionProcessSection>
                  )}

                  {/* Confidence */}
                  <div className="flex items-center justify-between text-sm">
                    <span className="text-muted-foreground">{t('agents:memory.confidence')}</span>
                    <Badge variant={execution.decision_process.confidence > 0.7 ? "default" : "secondary"}>
                      {(execution.decision_process.confidence * 100).toFixed(0)}%
                    </Badge>
                  </div>

                  {/* Conclusion */}
                  <Card className="p-4 bg-muted/50">
                    <div className="text-sm">
                      <span className="font-medium">{t('agents:memory.conclusion')}:</span>
                      <span className="ml-2">{execution.decision_process.conclusion}</span>
                    </div>
                  </Card>
                </>
              )}

              {/* Report */}
              {execution.result?.report && (
                <DecisionProcessSection
                  title={t('agents:memory.generatedReport')}
                  icon={<FileText className="h-4 w-4" />}
                >
                  <Card className="p-4">
                    <div className="text-sm whitespace-pre-wrap font-mono">
                      {execution.result.report.content}
                    </div>
                  </Card>
                </DecisionProcessSection>
              )}

              {/* Actions Executed */}
              {execution.result?.actions_executed && execution.result.actions_executed.length > 0 && (
                <DecisionProcessSection
                  title={t('agents:memory.actionsExecuted')}
                  icon={<Play className="h-4 w-4" />}
                >
                  <div className="space-y-2">
                    {execution.result.actions_executed.map((action, idx) => (
                      <div key={idx} className="flex items-center justify-between p-2 border rounded">
                        <div className="text-sm">
                          <div className="font-medium">{action.description}</div>
                          <div className="text-xs text-muted-foreground">{action.target}</div>
                        </div>
                        <Badge variant={action.success ? "default" : "destructive"}>
                          {action.success ? t('common:success') : t('common:failed')}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </DecisionProcessSection>
              )}
            </div>
          </ScrollArea>
        )}

        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

interface DecisionProcessSectionProps {
  title: string
  subtitle?: string
  icon: React.ReactNode
  children: React.ReactNode
}

function DecisionProcessSection({ title, subtitle, icon, children }: DecisionProcessSectionProps) {
  return (
    <div>
      <div className="flex items-center gap-2 mb-3">
        {icon}
        <h4 className="text-sm font-semibold">{title}</h4>
        {subtitle && (
          <span className="text-xs text-muted-foreground">({subtitle})</span>
        )}
      </div>
      {children}
    </div>
  )
}

function DataCollectedItem({ data }: { data: DataCollected }) {
  const formatJson = (value: any) => {
    try {
      return JSON.stringify(value, null, 2)
    } catch {
      return String(value)
    }
  }

  return (
    <Card className="p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-medium">{data.source}</span>
        <Badge variant="outline" className="text-xs">{data.data_type}</Badge>
      </div>
      <pre className="text-xs bg-muted p-2 rounded overflow-x-auto">
        {formatJson(data.values)}
      </pre>
    </Card>
  )
}

function ReasoningStepItem({ step }: { step: ReasoningStep }) {
  return (
    <div className="flex gap-3">
      <div className="flex flex-col items-center">
        <div className="w-6 h-6 rounded-full bg-primary text-primary-foreground text-xs flex items-center justify-center">
          {step.step_number}
        </div>
        {step.step_number < 10 && <div className="w-0.5 flex-1 bg-border" />}
      </div>
      <div className="flex-1 pb-4">
        <div className="text-sm font-medium">{step.description}</div>
        {step.input && (
          <div className="text-xs text-muted-foreground mt-1">
            {t('agents:memory.input')}: {step.input}
          </div>
        )}
        {step.output && (
          <div className="text-xs bg-muted p-2 rounded mt-2">
            {t('agents:memory.output')}: {step.output}
          </div>
        )}
        <div className="flex items-center gap-2 mt-2">
          <Badge variant="outline" className="text-xs">{step.step_type}</Badge>
          <span className="text-xs text-muted-foreground">
            {t('agents:memory.confidence')}: {(step.confidence * 100).toFixed(0)}%
          </span>
        </div>
      </div>
    </div>
  )
}

function DecisionItem({ decision }: { decision: Decision }) {
  return (
    <Card className="p-3">
      <div className="text-sm font-medium mb-1">{decision.description}</div>
      <div className="text-xs text-muted-foreground mb-2">{decision.rationale}</div>
      <Separator className="my-2" />
      <div className="flex items-center justify-between text-xs">
        <span className="text-muted-foreground">{t('agents:memory.action')}</span>
        <Badge variant="secondary">{decision.action}</Badge>
      </div>
    </Card>
  )
}

// Type definitions for execution details
export type { AgentExecutionDetail, DecisionProcess, DataCollected, ReasoningStep, Decision, GeneratedReport }

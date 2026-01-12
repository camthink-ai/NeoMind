// WorkflowMonitor Component
//
// Real-time workflow monitoring interface with live execution tracking,
// step-by-step progress, and workflow statistics.

import { useState, useCallback, useMemo } from "react"
import { useWorkflowEvents } from "@/hooks/useEvents"
import type { NeoTalkEvent } from "@/lib/events"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Progress } from "@/components/ui/progress"
import {
  Workflow,
  Play,
  Pause,
  CheckCircle,
  XCircle,
  RefreshCw,
  ChevronDown,
  ChevronUp,
  TrendingUp,
} from "lucide-react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

export interface WorkflowExecution {
  executionId: string
  workflowId: string
  workflowName: string
  status: "running" | "completed" | "failed" | "cancelled"
  startTime: number
  endTime?: number
  currentStep?: string
  completedSteps: string[]
  totalSteps: number
  input?: Record<string, unknown>
  output?: Record<string, unknown>
  error?: string
}

export interface WorkflowStep {
  stepId: string
  name: string
  type: "action" | "condition" | "loop" | "parallel" | "delay" | "notification"
  status: "pending" | "running" | "completed" | "failed" | "skipped"
  startTime?: number
  endTime?: number
  output?: unknown
  error?: string
}

export interface WorkflowStats {
  totalExecutions: number
  runningCount: number
  completedCount: number
  failedCount: number
  avgDuration: number
  successRate: number
}

export interface WorkflowMonitorProps {
  /**
   * Filter by workflow ID
   */
  workflowId?: string

  /**
   * Maximum number of executions to display
   */
  maxExecutions?: number

  /**
   * Whether to show step details
   */
  showSteps?: boolean

  /**
   * Whether to enable auto-refresh
   */
  autoRefresh?: boolean

  /**
   * Callback when workflow starts
   */
  onWorkflowStart?: (executionId: string, workflowId: string) => void

  /**
   * Callback when workflow completes
   */
  onWorkflowComplete?: (executionId: string, success: boolean) => void

  /**
   * Callback when workflow step fails
   */
  onStepFailed?: (executionId: string, stepId: string, error: string) => void
}

/**
 * WorkflowMonitor - Real-time workflow monitoring component
 *
 * @example
 * ```tsx
 * <WorkflowMonitor
 *   showSteps={true}
 *   onWorkflowComplete={(id, success) => console.log(`Workflow ${id} ${success ? 'succeeded' : 'failed'}`)}
 * />
 * ```
 */
export function WorkflowMonitor({
  workflowId,
  maxExecutions = 50,
  autoRefresh = true,
  onWorkflowStart,
  onWorkflowComplete,
}: WorkflowMonitorProps) {
  const [selectedWorkflow, setSelectedWorkflow] = useState<string | null>(workflowId || null)
  const [statusFilter, setStatusFilter] = useState<"all" | "running" | "completed" | "failed">("all")
  const [expandedExecutions, setExpandedExecutions] = useState<Set<string>>(new Set())
  const [isPaused, setIsPaused] = useState(false)

  // Subscribe to workflow events
  const { isConnected, clearEvents, reconnect } = useWorkflowEvents({
    enabled: autoRefresh && !isPaused,
    onEvent: handleWorkflowEvent,
    onConnected: (connected) => {
      console.log(`[WorkflowMonitor] Event stream ${connected ? "connected" : "disconnected"}`)
    },
    onError: (error) => {
      console.error("[WorkflowMonitor] Event stream error:", error)
    },
  })

  // Track workflow executions
  const [executions, setExecutions] = useState<Map<string, WorkflowExecution>>(new Map())

  // Calculate statistics
  const stats = useMemo((): WorkflowStats => {
    const execList = Array.from(executions.values())
    const running = execList.filter((e) => e.status === "running")
    const completed = execList.filter((e) => e.status === "completed")
    const failed = execList.filter((e) => e.status === "failed")

    // Calculate average duration for completed executions
    const completedWithDuration = completed.filter((e) => e.endTime)
    const avgDuration =
      completedWithDuration.length > 0
        ? completedWithDuration.reduce((sum, e) => sum + (e.endTime! - e.startTime), 0) /
          completedWithDuration.length
        : 0

    return {
      totalExecutions: execList.length,
      runningCount: running.length,
      completedCount: completed.length,
      failedCount: failed.length,
      avgDuration: Math.round(avgDuration),
      successRate:
        execList.length > 0 ? Math.round((completed.length / execList.length) * 100) : 100,
    }
  }, [executions])

  // Filter executions
  const filteredExecutions = useMemo(() => {
    let filtered = Array.from(executions.values()).sort((a, b) => b.startTime - a.startTime)

    if (selectedWorkflow) {
      filtered = filtered.filter((e) => e.workflowId === selectedWorkflow)
    }

    if (statusFilter !== "all") {
      filtered = filtered.filter((e) => e.status === statusFilter)
    }

    return filtered.slice(0, maxExecutions)
  }, [executions, selectedWorkflow, statusFilter, maxExecutions])

  // Get unique workflow IDs
  const workflowIds = useMemo(() => {
    const ids = new Set<string>()
    executions.forEach((exec) => ids.add(exec.workflowId))
    return Array.from(ids)
  }, [executions])

  // Get status badge
  const getStatusBadge = useCallback((status: WorkflowExecution["status"]) => {
    const variants: Record<WorkflowExecution["status"], "default" | "secondary" | "destructive" | "outline"> = {
      running: "default",
      completed: "default",
      failed: "destructive",
      cancelled: "secondary",
    }

    const labels: Record<WorkflowExecution["status"], string> = {
      running: "运行中",
      completed: "已完成",
      failed: "失败",
      cancelled: "已取消",
    }

    const icons: Record<WorkflowExecution["status"], React.ReactNode> = {
      running: <Play className="h-3 w-3" />,
      completed: <CheckCircle className="h-3 w-3" />,
      failed: <XCircle className="h-3 w-3" />,
      cancelled: <Pause className="h-3 w-3" />,
    }

    return (
      <Badge variant={variants[status]} className="text-xs">
        <span className="mr-1">{icons[status]}</span>
        {labels[status]}
      </Badge>
    )
  }, [])

  // Handle workflow events
  function handleWorkflowEvent(event: NeoTalkEvent) {
    if (event.type === "WorkflowTriggered") {
      const data = event.data as {
        workflow_id?: string
        workflow_name?: string
        execution_id?: string
        total_steps?: number
        input?: Record<string, unknown>
      }

      const executionId = data.execution_id || `exec-${Date.now()}`
      const execution: WorkflowExecution = {
        executionId,
        workflowId: data.workflow_id || "unknown",
        workflowName: data.workflow_name || data.workflow_id || "Unknown",
        status: "running",
        startTime: event.timestamp,
        totalSteps: data.total_steps || 0,
        completedSteps: [],
        input: data.input,
      }

      setExecutions((prev) => {
        const next = new Map(prev)
        next.set(executionId, execution)
        return next
      })

      onWorkflowStart?.(executionId, execution.workflowId)
    } else if (event.type === "WorkflowStepCompleted") {
      const data = event.data as {
        execution_id?: string
        step_id?: string
        step_name?: string
        output?: unknown
      }

      if (data.execution_id && data.step_id) {
        setExecutions((prev) => {
          const next = new Map(prev)
          const current = next.get(data.execution_id!)
          if (current) {
            next.set(data.execution_id!, {
              ...current,
              completedSteps: [...current.completedSteps, data.step_id!],
            })
          }
          return next
        })
      }
    } else if (event.type === "WorkflowCompleted") {
      const data = event.data as {
        execution_id?: string
        success?: boolean
        output?: Record<string, unknown>
        error?: string
      }

      if (data.execution_id) {
        setExecutions((prev) => {
          const next = new Map(prev)
          const current = next.get(data.execution_id!)
          if (current) {
            const updated: WorkflowExecution = {
              ...current,
              status: data.success === false ? "failed" : "completed",
              endTime: event.timestamp,
              output: data.output,
              error: data.error,
            }
            next.set(data.execution_id!, updated)
            onWorkflowComplete?.(data.execution_id!, data.success !== false)
          }
          return next
        })
      }
    }
  }

  // Format timestamp
  const formatTimestamp = useCallback((timestamp: number): string => {
    const date = new Date(timestamp * 1000)
    return date.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    })
  }, [])

  // Format duration
  const formatDuration = useCallback((start: number, end?: number): string => {
    const duration = (end || Date.now() / 1000) - start
    if (duration < 60) return `${Math.round(duration)}秒`
    if (duration < 3600) return `${Math.floor(duration / 60)}分${Math.round(duration % 60)}秒`
    return `${Math.floor(duration / 3600)}小时${Math.floor((duration % 3600) / 60)}分`
  }, [])

  // Toggle expansion
  const toggleExpansion = useCallback((executionId: string) => {
    setExpandedExecutions((prev) => {
      const next = new Set(prev)
      if (next.has(executionId)) {
        next.delete(executionId)
      } else {
        next.add(executionId)
      }
      return next
    })
  }, [])

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            {isConnected ? (
              <Workflow className="h-5 w-5 text-green-500" />
            ) : (
              <Workflow className="h-5 w-5 text-gray-400" />
            )}
            <h2 className="text-xl font-semibold">工作流监控</h2>
          </div>
          <Badge variant={isConnected ? "default" : "secondary"}>
            {isConnected ? "已连接" : "未连接"}
          </Badge>
          {isPaused && (
            <Badge variant="secondary" className="text-yellow-600">
              已暂停
            </Badge>
          )}
        </div>

        <div className="flex items-center gap-2">
          {/* Workflow Filter */}
          {workflowIds.length > 0 && (
            <Select
              value={selectedWorkflow || "all"}
              onValueChange={(v) => setSelectedWorkflow(v === "all" ? null : v)}
            >
              <SelectTrigger className="w-[200px]">
                <SelectValue placeholder="选择工作流" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">所有工作流</SelectItem>
                {workflowIds.map((id) => (
                  <SelectItem key={id} value={id}>
                    {id}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}

          {/* Status Filter */}
          <Select value={statusFilter} onValueChange={(v: typeof statusFilter) => setStatusFilter(v)}>
            <SelectTrigger className="w-[120px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">全部状态</SelectItem>
              <SelectItem value="running">运行中</SelectItem>
              <SelectItem value="completed">已完成</SelectItem>
              <SelectItem value="failed">失败</SelectItem>
            </SelectContent>
          </Select>

          <Button variant="outline" size="icon" onClick={reconnect} title="重新连接">
            <RefreshCw className="h-4 w-4" />
          </Button>

          <Button
            variant="outline"
            size="icon"
            onClick={() => setIsPaused((p) => !p)}
            title={isPaused ? "恢复监控" : "暂停监控"}
          >
            {isPaused ? <Play className="h-4 w-4" /> : <Pause className="h-4 w-4" />}
          </Button>

          <Button variant="outline" size="sm" onClick={clearEvents}>
            清除
          </Button>
        </div>
      </div>

      {/* Statistics Cards */}
      <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center gap-2">
              <Workflow className="h-4 w-4 text-muted-foreground" />
              <span className="text-sm text-muted-foreground">总执行数</span>
            </div>
            <p className="text-2xl font-bold mt-2">{stats.totalExecutions}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center gap-2">
              <Play className="h-4 w-4 text-blue-500" />
              <span className="text-sm text-muted-foreground">运行中</span>
            </div>
            <p className="text-2xl font-bold mt-2">{stats.runningCount}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center gap-2">
              <CheckCircle className="h-4 w-4 text-green-500" />
              <span className="text-sm text-muted-foreground">已完成</span>
            </div>
            <p className="text-2xl font-bold mt-2">{stats.completedCount}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center gap-2">
              <XCircle className="h-4 w-4 text-red-500" />
              <span className="text-sm text-muted-foreground">失败</span>
            </div>
            <p className="text-2xl font-bold mt-2">{stats.failedCount}</p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center gap-2">
              <TrendingUp className="h-4 w-4 text-purple-500" />
              <span className="text-sm text-muted-foreground">成功率</span>
            </div>
            <p className="text-2xl font-bold mt-2">{stats.successRate}%</p>
          </CardContent>
        </Card>
      </div>

      {/* Executions List */}
      <div className="flex-1 min-h-0">
        <ScrollArea className="h-full">
          <div className="space-y-3 pr-4">
            {filteredExecutions.length === 0 ? (
              <Card>
                <CardContent className="flex items-center justify-center py-12 text-muted-foreground">
                  <div className="text-center">
                    <Workflow className="h-12 w-12 mx-auto mb-4 opacity-50" />
                    <p>{isConnected ? "等待工作流事件..." : "未连接到事件流"}</p>
                  </div>
                </CardContent>
              </Card>
            ) : (
              filteredExecutions.map((execution) => {
                const isExpanded = expandedExecutions.has(execution.executionId)
                const progress = execution.totalSteps > 0
                  ? (execution.completedSteps.length / execution.totalSteps) * 100
                  : execution.status === "completed"
                  ? 100
                  : execution.status === "running"
                  ? 50
                  : 0

                return (
                  <Card
                    key={execution.executionId}
                    className={`transition-all ${
                      execution.status === "running" ? "border-blue-500/50" : ""
                    }`}
                  >
                    <CardHeader
                      className="cursor-pointer select-none"
                      onClick={() => toggleExpansion(execution.executionId)}
                    >
                      <div className="flex items-start justify-between gap-4">
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <Workflow className="h-4 w-4 text-blue-500 shrink-0" />
                            <CardTitle className="text-base truncate">{execution.workflowName}</CardTitle>
                          </div>
                          <CardDescription className="flex items-center gap-2 text-xs">
                            <span className="font-mono">{execution.executionId.slice(-12)}</span>
                            <span>•</span>
                            <span>{formatTimestamp(execution.startTime)}</span>
                            <span>•</span>
                            <span>{formatDuration(execution.startTime, execution.endTime)}</span>
                          </CardDescription>
                        </div>
                        <div className="flex items-center gap-2 shrink-0">
                          {getStatusBadge(execution.status)}
                          <Button variant="ghost" size="icon" className="h-6 w-6">
                            {isExpanded ? (
                              <ChevronUp className="h-4 w-4" />
                            ) : (
                              <ChevronDown className="h-4 w-4" />
                            )}
                          </Button>
                        </div>
                      </div>

                      {/* Progress Bar */}
                      {execution.totalSteps > 0 && (
                        <div className="mt-3 space-y-1">
                          <div className="flex items-center justify-between text-xs text-muted-foreground">
                            <span>
                              步骤: {execution.completedSteps.length} / {execution.totalSteps}
                            </span>
                            <span>{Math.round(progress)}%</span>
                          </div>
                          <Progress value={progress} className="h-2" />
                        </div>
                      )}
                    </CardHeader>

                    {isExpanded && (
                      <CardContent className="space-y-3">
                        {/* Execution Details */}
                        <div className="grid grid-cols-2 gap-4 text-sm">
                          <div>
                            <span className="text-muted-foreground">工作流 ID:</span>
                            <span className="ml-2 font-mono">{execution.workflowId}</span>
                          </div>
                          <div>
                            <span className="text-muted-foreground">执行 ID:</span>
                            <span className="ml-2 font-mono">{execution.executionId}</span>
                          </div>
                          <div>
                            <span className="text-muted-foreground">开始时间:</span>
                            <span className="ml-2">{formatTimestamp(execution.startTime)}</span>
                          </div>
                          <div>
                            <span className="text-muted-foreground">结束时间:</span>
                            <span className="ml-2">
                              {execution.endTime ? formatTimestamp(execution.endTime) : "-"}
                            </span>
                          </div>
                        </div>

                        {/* Input/Output */}
                        {execution.input && Object.keys(execution.input).length > 0 && (
                          <div className="rounded-md bg-muted p-3">
                            <div className="text-sm font-medium mb-2">输入参数</div>
                            <pre className="text-xs text-muted-foreground overflow-x-auto">
                              {JSON.stringify(execution.input, null, 2)}
                            </pre>
                          </div>
                        )}

                        {execution.output && Object.keys(execution.output).length > 0 && (
                          <div className="rounded-md bg-muted p-3">
                            <div className="text-sm font-medium mb-2">输出结果</div>
                            <pre className="text-xs text-muted-foreground overflow-x-auto">
                              {JSON.stringify(execution.output, null, 2)}
                            </pre>
                          </div>
                        )}

                        {execution.error && (
                          <div className="rounded-md bg-destructive/10 p-3">
                            <div className="text-sm font-medium mb-1 text-destructive">错误信息</div>
                            <p className="text-sm text-destructive/80">{execution.error}</p>
                          </div>
                        )}
                      </CardContent>
                    )}
                  </Card>
                )
              })
            )}
          </div>
        </ScrollArea>
      </div>
    </div>
  )
}

// RuleMonitor Component
//
// Real-time rule monitoring interface with live evaluation updates,
// trigger history, and rule statistics.

import { useState, useCallback, useMemo } from "react"
import { useRuleEvents } from "@/hooks/useEvents"
import type { NeoTalkEvent } from "@/lib/events"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Activity,
  Zap,
  Clock,
  AlertTriangle,
  CheckCircle,
  RefreshCw,
  Pause,
  Play,
  BarChart3,
  TrendingUp,
  TrendingDown,
  Minus,
} from "lucide-react"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { MonitorStatsGrid, EmptyStateInline } from "@/components/shared"

export interface RuleEvaluation {
  ruleId: string
  ruleName: string
  timestamp: number
  condition: string
  result: boolean
  triggerValue?: number
  message?: string
}

export interface RuleTriggerEvent {
  ruleId: string
  ruleName: string
  timestamp: number
  triggerValue: number
  actions: string[]
  executionSuccess?: boolean
}

export interface RuleStats {
  ruleId: string
  ruleName: string
  totalEvaluations: number
  triggerCount: number
  successRate: number
  lastTriggered: number | null
  avgTriggerValue: number | null
}

export interface RuleMonitorProps {
  /**
   * Filter events by rule ID
   */
  ruleId?: string

  /**
   * Maximum number of events to display
   */
  maxEvents?: number

  /**
   * Whether to show the event log
   */
  showEventLog?: boolean

  /**
   * Whether to show rule statistics
   */
  showStats?: boolean

  /**
   * Whether to enable auto-refresh
   */
  autoRefresh?: boolean

  /**
   * Callback when a rule is triggered
   */
  onRuleTriggered?: (ruleId: string, ruleName: string) => void

  /**
   * Callback when a rule execution fails
   */
  onRuleFailed?: (ruleId: string, ruleName: string, error: string) => void
}

interface RuleStatus {
  ruleId: string
  ruleName: string
  enabled: boolean
  lastEvaluated: number
  lastTriggered: number | null
  triggerCount: number
  evaluationCount: number
}

/**
 * RuleMonitor - Real-time rule monitoring component
 *
 * @example
 * ```tsx
 * <RuleMonitor
 *   showEventLog={true}
 *   showStats={true}
 *   onRuleTriggered={(id, name) => console.log(`Rule ${name} triggered`)}
 * />
 * ```
 */
export function RuleMonitor({
  ruleId,
  maxEvents = 100,
  showEventLog = true,
  showStats = true,
  autoRefresh = true,
  onRuleTriggered,
  onRuleFailed,
}: RuleMonitorProps) {
  const [selectedRule, setSelectedRule] = useState<string | null>(ruleId || null)
  const [eventFilter, setEventFilter] = useState<"all" | "evaluations" | "triggers" | "executions">("all")
  const [isPaused, setIsPaused] = useState(false)

  // Subscribe to rule events
  const { isConnected, events, clearEvents, reconnect } = useRuleEvents({
    enabled: autoRefresh && !isPaused,
    onEvent: handleRuleEvent,
    onConnected: (connected) => {
      console.log(`[RuleMonitor] Event stream ${connected ? "connected" : "disconnected"}`)
    },
    onError: (error) => {
      console.error("[RuleMonitor] Event stream error:", error)
    },
  })

  // Track rule statuses
  const [ruleStatuses, setRuleStatuses] = useState<Map<string, RuleStatus>>(new Map())

  // Filter events
  const filteredEvents = useMemo(() => {
    let filtered = events

    // Apply rule filter
    if (selectedRule) {
      filtered = filtered.filter((e) => {
        const data = e.data as { rule_id?: string }
        return data.rule_id === selectedRule
      })
    }

    // Apply event type filter
    if (eventFilter !== "all") {
      switch (eventFilter) {
        case "evaluations":
          filtered = filtered.filter((e) => e.type === "RuleEvaluated")
          break
        case "triggers":
          filtered = filtered.filter((e) => e.type === "RuleTriggered")
          break
        case "executions":
          filtered = filtered.filter((e) => e.type === "RuleExecuted")
          break
      }
    }

    return filtered.slice(-maxEvents)
  }, [events, selectedRule, eventFilter, maxEvents])

  // Calculate rule statistics
  const ruleStats = useMemo(() => {
    const stats = new Map<string, RuleStats>()

    events.forEach((event) => {
      const data = event.data as { rule_id?: string; rule_name?: string }

      if (!data.rule_id) return

      const current = stats.get(data.rule_id) || {
        ruleId: data.rule_id,
        ruleName: data.rule_name || data.rule_id,
        totalEvaluations: 0,
        triggerCount: 0,
        successRate: 1,
        lastTriggered: null,
        avgTriggerValue: null,
      }

      if (event.type === "RuleEvaluated") {
        current.totalEvaluations++
      } else if (event.type === "RuleTriggered") {
        current.triggerCount++
        current.lastTriggered = event.timestamp
      } else if (event.type === "RuleExecuted") {
        // Could track success/failure here
      }

      stats.set(data.rule_id, current)
    })

    return Array.from(stats.values())
  }, [events])

  // Get unique rule IDs from events
  const ruleIds = useMemo(() => {
    const ids = new Set<string>()
    events.forEach((event) => {
      const data = event.data as { rule_id?: string }
      if (data.rule_id) {
        ids.add(data.rule_id)
      }
    })
    return Array.from(ids)
  }, [events])

  // Get trigger trend for a rule
  const getTriggerTrend = useCallback((ruleId: string): "up" | "down" | "stable" => {
    const recentTriggers = events
      .filter((e) => e.type === "RuleTriggered")
      .filter((e) => (e.data as { rule_id?: string }).rule_id === ruleId)
      .slice(-10)

    if (recentTriggers.length < 2) return "stable"

    const firstHalf = recentTriggers.slice(0, Math.floor(recentTriggers.length / 2)).length
    const secondHalf = recentTriggers.slice(Math.floor(recentTriggers.length / 2)).length

    if (secondHalf > firstHalf * 1.5) return "up"
    if (secondHalf < firstHalf * 0.5) return "down"
    return "stable"
  }, [events])

  // Handle incoming rule events
  function handleRuleEvent(event: NeoTalkEvent) {
    const data = event.data as { rule_id?: string; rule_name?: string }

    if (!data.rule_id) return

    setRuleStatuses((prev) => {
      const updated = new Map(prev)
      const current = updated.get(data.rule_id!)

      if (event.type === "RuleEvaluated") {
        updated.set(data.rule_id!, {
          ruleId: data.rule_id!,
          ruleName: data.rule_name || data.rule_id!,
          enabled: current?.enabled ?? true,
          lastEvaluated: event.timestamp,
          lastTriggered: current?.lastTriggered || null,
          triggerCount: current?.triggerCount || 0,
          evaluationCount: (current?.evaluationCount || 0) + 1,
        })
      } else if (event.type === "RuleTriggered") {
        updated.set(data.rule_id!, {
          ruleId: data.rule_id!,
          ruleName: data.rule_name || data.rule_id!,
          enabled: current?.enabled ?? true,
          lastEvaluated: event.timestamp,
          lastTriggered: event.timestamp,
          triggerCount: (current?.triggerCount || 0) + 1,
          evaluationCount: current?.evaluationCount || 0,
        })
        onRuleTriggered?.(data.rule_id!, data.rule_name || data.rule_id!)
      } else if (event.type === "RuleExecuted") {
        const execData = event.data as { success?: boolean; error?: string }
        if (execData.success === false) {
          onRuleFailed?.(data.rule_id!, data.rule_name || data.rule_id!, execData.error || "Unknown error")
        }
      }

      return updated
    })
  }

  // Format timestamp for display
  const formatTimestamp = useCallback((timestamp: number): string => {
    const date = new Date(timestamp * 1000)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return "刚刚"
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`

    return date.toLocaleString("zh-CN", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    })
  }, [])

  // Get event icon and color
  const getEventIcon = useCallback((eventType: string) => {
    switch (eventType) {
      case "RuleEvaluated":
        return <Activity className="h-4 w-4 text-info" />
      case "RuleTriggered":
        return <Zap className="h-4 w-4 text-warning" />
      case "RuleExecuted":
        return <CheckCircle className="h-4 w-4 text-green-500" />
      case "AlertCreated":
        return <AlertTriangle className="h-4 w-4 text-error" />
      default:
        return <Activity className="h-4 w-4 text-gray-500" />
    }
  }, [])

  // Get event type display name
  const getEventTypeName = useCallback((eventType: string): string => {
    const names: Record<string, string> = {
      RuleEvaluated: "规则评估",
      RuleTriggered: "规则触发",
      RuleExecuted: "规则执行",
      AlertCreated: "告警创建",
    }
    return names[eventType] || eventType
  }, [])

  // Calculate trigger count in recent time window
  const getRecentTriggerCount = useCallback((ruleId: string, minutes = 5): number => {
    const cutoff = Date.now() / 1000 - minutes * 60
    return events.filter(
      (e) =>
        e.type === "RuleTriggered" &&
        (e.data as { rule_id?: string }).rule_id === ruleId &&
        e.timestamp > cutoff
    ).length
  }, [events])

  return (
    <div className="flex flex-col gap-4 h-full">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            {isConnected ? (
              <Zap className="h-5 w-5 text-green-500" />
            ) : (
              <Zap className="h-5 w-5 text-gray-400" />
            )}
            <h2 className="text-xl font-semibold">规则监控</h2>
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
          {/* Rule Filter */}
          {ruleIds.length > 0 && (
            <Select value={selectedRule || "all"} onValueChange={(v) => setSelectedRule(v === "all" ? null : v)}>
              <SelectTrigger className="w-[200px]">
                <SelectValue placeholder="选择规则" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">所有规则</SelectItem>
                {ruleIds.map((id) => {
                  const status = ruleStatuses.get(id)
                  return (
                    <SelectItem key={id} value={id}>
                      {status?.ruleName || id}
                    </SelectItem>
                  )
                })}
              </SelectContent>
            </Select>
          )}

          {/* Event Type Filter */}
          <Select value={eventFilter} onValueChange={(v: typeof eventFilter) => setEventFilter(v)}>
            <SelectTrigger className="w-[120px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">全部事件</SelectItem>
              <SelectItem value="evaluations">评估</SelectItem>
              <SelectItem value="triggers">触发</SelectItem>
              <SelectItem value="executions">执行</SelectItem>
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
            清除事件
          </Button>
        </div>
      </div>

      {/* Rule Statistics Cards */}
      {showStats && ruleStats.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
          {ruleStats.slice(0, 8).map((stats) => {
            const trend = getTriggerTrend(stats.ruleId)
            const recentTriggers = getRecentTriggerCount(stats.ruleId)
            const status = ruleStatuses.get(stats.ruleId)

            return (
              <Card
                key={stats.ruleId}
                className={`cursor-pointer transition-all ${
                  selectedRule === stats.ruleId ? "ring-2 ring-primary" : "hover:border-primary/50"
                }`}
                onClick={() => setSelectedRule(selectedRule === stats.ruleId ? null : stats.ruleId)}
              >
                <CardHeader className="pb-2">
                  <div className="flex items-center justify-between">
                    <CardTitle className="text-sm font-medium truncate" title={stats.ruleName}>
                      {stats.ruleName}
                    </CardTitle>
                    <Badge
                      variant={status?.enabled ? "default" : "secondary"}
                      className="text-xs"
                    >
                      {status?.enabled ? "启用" : "禁用"}
                    </Badge>
                  </div>
                </CardHeader>
                <CardContent>
                  <div className="space-y-2">
                    <div className="flex items-center justify-between">
                      <span className="text-xs text-muted-foreground">触发次数</span>
                      <span className="font-medium">{stats.triggerCount}</span>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-xs text-muted-foreground">评估次数</span>
                      <span className="font-medium">{stats.totalEvaluations}</span>
                    </div>
                    <div className="flex items-center justify-between">
                      <span className="text-xs text-muted-foreground">近期触发</span>
                      <div className="flex items-center gap-1">
                        <span className="font-medium">{recentTriggers}</span>
                        {trend === "up" && <TrendingUp className="h-3 w-3 text-green-500" />}
                        {trend === "down" && <TrendingDown className="h-3 w-3 text-error" />}
                        {trend === "stable" && <Minus className="h-3 w-3 text-gray-400" />}
                      </div>
                    </div>
                    {stats.lastTriggered && (
                      <div className="flex items-center gap-1 text-xs text-muted-foreground">
                        <Clock className="h-3 w-3" />
                        {formatTimestamp(stats.lastTriggered)}
                      </div>
                    )}
                  </div>
                </CardContent>
              </Card>
            )
          })}
        </div>
      )}

      {/* Statistics Summary */}
      {showStats && (
        <MonitorStatsGrid
          stats={[
            {
              label: '总评估次数',
              value: ruleStats.reduce((sum, s) => sum + s.totalEvaluations, 0),
              icon: <Activity className="h-5 w-5" />,
              color: 'default',
            },
            {
              label: '总触发次数',
              value: ruleStats.reduce((sum, s) => sum + s.triggerCount, 0),
              icon: <Zap className="h-5 w-5" />,
              color: 'warning',
            },
            {
              label: '活跃规则',
              value: ruleStats.length,
              icon: <CheckCircle className="h-5 w-5" />,
              color: 'success',
            },
            {
              label: '事件/分钟',
              value: events.length > 0
                ? (
                    events.filter((e) => e.timestamp > Date.now() / 1000 - 60).length
                  ).toFixed(1)
                : "0",
              icon: <BarChart3 className="h-5 w-5" />,
              color: 'info',
            },
          ]}
        />
      )}

      {/* Event Log Table */}
      {showEventLog && (
        <Card className="flex-1 flex flex-col min-h-0">
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Clock className="h-5 w-5" />
              事件日志
              <Badge variant="outline" className="text-xs">
                {filteredEvents.length}
              </Badge>
            </CardTitle>
          </CardHeader>
          <CardContent className="flex-1 p-0 min-h-0">
            <ScrollArea className="h-full">
              <Table>
                <TableHeader className="sticky top-0 bg-background">
                  <TableRow>
                    <TableHead className="w-[180px]">时间</TableHead>
                    <TableHead>事件类型</TableHead>
                    <TableHead>规则</TableHead>
                    <TableHead>详情</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filteredEvents.length === 0 ? (
                    <EmptyStateInline title={isConnected ? "等待事件..." : "未连接到事件流"} colSpan={4} />
                  ) : (
                    filteredEvents
                      .slice()
                      .reverse()
                      .map((event, idx) => {
                        const data = event.data as {
                          rule_id?: string
                          rule_name?: string
                          trigger_value?: number
                          actions?: string[]
                          condition?: string
                          result?: boolean
                        }

                        return (
                          <TableRow key={`${event.id}-${idx}`}>
                            <TableCell className="text-sm text-muted-foreground">
                              {formatTimestamp(event.timestamp)}
                            </TableCell>
                            <TableCell>
                              <div className="flex items-center gap-2">
                                {getEventIcon(event.type)}
                                <span>{getEventTypeName(event.type)}</span>
                              </div>
                            </TableCell>
                            <TableCell className="font-mono text-xs">
                              {data.rule_name || data.rule_id || "-"}
                            </TableCell>
                            <TableCell className="text-sm text-muted-foreground">
                              {event.type === "RuleTriggered" && (
                                <span>触发值: {data.trigger_value?.toFixed(2)}</span>
                              )}
                              {event.type === "RuleExecuted" && (
                                <span>操作: {data.actions?.join(", ") || "-"}</span>
                              )}
                              {event.type === "RuleEvaluated" && (
                                <span>
                                  {data.condition} = {data.result ? "true" : "false"}
                                </span>
                              )}
                            </TableCell>
                          </TableRow>
                        )
                      })
                  )}
                </TableBody>
              </Table>
            </ScrollArea>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

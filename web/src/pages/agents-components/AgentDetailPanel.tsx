/**
 * Agent Detail Panel - Right side / full-screen dialog content for Agents page.
 *
 * Renders the clean-slate AgentSpec contract (trigger, input requirements,
 * output fields/publish, tool policy, three-way notify, pacing guardrails,
 * controlled actions, effective plan, last-run explanation) alongside execution
 * history, memory (journal + knowledge files), and user messages.
 */

import { useState, useEffect, useRef, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { LoadingState } from "@/components/shared/LoadingState"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  AlertTriangle,
  BarChart3,
  Bell,
  Bot,
  Brain,
  ChevronDown,
  ChevronRight,
  Database,
  Eye,
  FileText,
  Gauge,
  History,
  Info,
  MessageSquare,
  Shield,
  Sparkles,
  Workflow,
  Wrench,
  Zap,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { formatRelativeTime, formatTimestamp } from "@/lib/utils/format"
import { useIsMobile } from "@/hooks/useMobile"
import { api } from "@/lib/api"
import type {
  AiAgentDetail,
  AgentExecution,
  AgentMemory,
  AgentNotify,
  ControlledAction,
  FreshnessPolicy,
  InputRequirement,
  InputSourceRef,
  JournalExecutionRecord,
  KnowledgeFileRef,
  LlmBackendInstance,
  MissingPolicy,
  NotifyPolicy,
  PacingPolicy,
  ToolPolicy,
  TriggerSpec,
} from "@/types"
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { AgentExecutionStartedEvent, AgentExecutionCompletedEvent } from "@/lib/events"
import { FullScreenDialogSidebar } from "@/components/automation/dialog/FullScreenDialog"
import { useEvents } from "@/hooks/useEvents"
import { MobileTabStrip } from "@/components/shared/MobileTabStrip"

// Import sub-components
import { AgentExecutionTimeline } from "./AgentExecutionTimeline"
import { AgentUserMessages, AgentUserMessagesComposer } from "./AgentUserMessages"

interface AgentDetailPanelProps {
  agent: AiAgentDetail | null
  onEdit: (agent: AiAgentDetail) => void
  onExecute: (agent: AiAgentDetail) => void
  onViewExecutionDetail: (agentId: string, executionId: string) => void
  onRefresh: () => void
  inlineMode?: boolean // When true, used inside dialog (no empty state)
}

// ============================================================================
// Pure contract formatting helpers
// ============================================================================

type TFn = (key: string, opts?: Record<string, unknown>) => string

function formatInterval(secs: number, t: TFn): string {
  if (secs >= 3600 && secs % 3600 === 0) {
    return t('agents:detail.everyHours', { hours: secs / 3600 })
  }
  if (secs >= 60 && secs % 60 === 0) {
    return t('agents:detail.everyMins', { mins: secs / 60 })
  }
  return t('agents:detail.everySecs', { secs })
}

function formatInputSource(source: InputSourceRef): string {
  switch (source.kind) {
    case 'device_metric':
      return `${source.device_id} / ${source.field}`
    case 'whole_device':
      return source.device_id
    case 'extension_instance':
      return `ext:${source.id}`
    case 'upstream_ai':
      return `ai:${source.agent_id}:${source.field}`
    case 'caller_payload':
      return 'caller_payload'
  }
}

function formatFreshness(freshness: FreshnessPolicy, t: TFn): string {
  switch (freshness.kind) {
    case 'same_report':
      return t('agents:detail.freshnessSameReport')
    case 'latest_within':
      return t('agents:detail.freshnessLatestWithin', { secs: freshness.max_age_secs })
    case 'lookback_window':
      return t('agents:detail.freshnessLookback', {
        secs: freshness.lookback_secs,
        points: freshness.max_points,
      })
    case 'any':
      return t('agents:detail.freshnessAny')
  }
}

function formatMissingPolicy(policy: MissingPolicy, t: TFn): string {
  switch (policy) {
    case 'undetermined':
      return t('agents:detail.missingUndetermined')
    case 'report_stale':
      return t('agents:detail.missingReportStale')
    case 'skip':
      return t('agents:detail.missingSkip')
    case 'ignore':
      return t('agents:detail.missingIgnore')
  }
}

function formatEventFilter(filter: unknown): string | null {
  if (filter === null || filter === undefined) return null
  if (typeof filter === 'string') return filter.trim() || null
  if (typeof filter === 'object') {
    const keys = Object.keys(filter as Record<string, unknown>)
    if (keys.length === 0) return null
    return JSON.stringify(filter)
  }
  return String(filter)
}

function describeToolPolicy(
  policy: ToolPolicy | undefined,
  t: TFn,
): {
  mode: 'disabled' | 'scoped' | 'unrestricted'
  label: string
  allowedTools: string[]
  allowedCommands: string[]
} {
  if (!policy || policy === 'disabled') {
    return {
      mode: 'disabled',
      label: t('agents:editorV2.toolPolicy.disabled'),
      allowedTools: [],
      allowedCommands: [],
    }
  }
  if (policy === 'unrestricted') {
    return {
      mode: 'unrestricted',
      label: t('agents:editorV2.toolPolicy.unrestricted'),
      allowedTools: [],
      allowedCommands: [],
    }
  }
  if (typeof policy === 'object' && 'scoped' in policy && policy.scoped) {
    return {
      mode: 'scoped',
      label: t('agents:editorV2.toolPolicy.scoped'),
      allowedTools: Array.isArray(policy.scoped.allowed_tools) ? policy.scoped.allowed_tools : [],
      allowedCommands: Array.isArray(policy.scoped.allowed_commands) ? policy.scoped.allowed_commands : [],
    }
  }
  return {
    mode: 'disabled',
    label: t('agents:editorV2.toolPolicy.disabled'),
    allowedTools: [],
    allowedCommands: [],
  }
}

function normalizeNotifyPolicy(notify: NotifyPolicy | AgentNotify | undefined): {
  channels: string[]
  onComplete: boolean
  onAlert: boolean
  alertDependsOn: string[]
  onFailure: boolean
  pendingConfirmation: boolean
} {
  if (!notify) {
    return {
      channels: [],
      onComplete: false,
      onAlert: false,
      alertDependsOn: [],
      onFailure: false,
      pendingConfirmation: false,
    }
  }
  if ('on' in notify) {
    return {
      channels: notify.channels || [],
      onComplete: notify.on === 'always',
      onAlert: false,
      alertDependsOn: [],
      onFailure: notify.on === 'failure' || notify.on === 'always',
      pendingConfirmation: notify.on === 'judgment',
    }
  }
  return {
    channels: notify.channels || [],
    onComplete: Boolean(notify.on_complete),
    onAlert: Boolean(notify.on_alert),
    alertDependsOn: notify.alert_depends_on || [],
    onFailure: Boolean(notify.on_failure),
    pendingConfirmation: Boolean(notify.pending_confirmation),
  }
}

function hasPacingSettings(pacing: PacingPolicy | undefined): boolean {
  if (!pacing) return false
  return (
    pacing.burst_merge_secs != null ||
    pacing.min_interval_secs != null ||
    pacing.max_runs_per_day != null ||
    pacing.failure_threshold != null
  )
}

function resolveTrigger(agent: AiAgentDetail): TriggerSpec {
  if (agent.trigger) return agent.trigger
  // Graceful fallback if a partial mock object still provides legacy schedule
  if (agent.schedule) {
    switch (agent.schedule.schedule_type) {
      case 'cron':
        return {
          kind: 'cron',
          expr: agent.schedule.cron_expression || '',
          timezone: agent.schedule.timezone || 'UTC',
        }
      case 'interval':
        return {
          kind: 'interval',
          every_secs: agent.schedule.interval_seconds || 0,
        }
      case 'event':
        return {
          kind: 'event',
          filter: agent.schedule.event_filter,
        }
      default:
        return { kind: 'manual' }
    }
  }
  return { kind: 'manual' }
}

// ============================================================================
// Main Panel
// ============================================================================

export function AgentDetailPanel({
  agent,
  onEdit: _onEdit,
  onExecute: _onExecute,
  onViewExecutionDetail,
  onRefresh,
  inlineMode = false,
}: AgentDetailPanelProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const isMobile = useIsMobile()
  const [executions, setExecutions] = useState<AgentExecution[]>([])
  const [executionsLoading, setExecutionsLoading] = useState(false)
  const [memory, setMemory] = useState<AgentMemory | null>(null)
  const [memoryLoading, setMemoryLoading] = useState(false)
  const [explainLast, setExplainLast] = useState<string | null>(null)

  // Real-time status from WebSocket events
  const [realtimeStatus, setRealtimeStatus] = useState<string | null>(null)
  // Backend id -> human-readable name/model
  const [llmBackends, setLlmBackends] = useState<LlmBackendInstance[]>([])
  const [section, setSection] = useState<'overview' | 'history' | 'memory' | 'messages'>('overview')
  // Bumped by the pinned composer so the message list refetches after a send
  const [messagesVersion, setMessagesVersion] = useState(0)

  const agentId = agent?.id

  const loadExecutions = useCallback(async (id: string) => {
    setExecutionsLoading(true)
    try {
      const data = await api.getAgentExecutions(id)
      setExecutions(data.executions || [])
    } catch (error) {
      handleError(error, { operation: 'Load agent executions', showToast: false })
    } finally {
      setExecutionsLoading(false)
    }
  }, [handleError])

  const loadMemory = useCallback(async (id: string) => {
    setMemoryLoading(true)
    try {
      const data = await api.getAgentMemory(id)
      setMemory(data)
    } catch (error) {
      handleError(error, { operation: 'Load agent memory', showToast: false })
    } finally {
      setMemoryLoading(false)
    }
  }, [handleError])

  const loadExplainLast = useCallback(async (id: string) => {
    try {
      const res = await api.explainLastAgent(id, { skipErrorToast: true })
      const text = res?.explanation?.trim() || null
      setExplainLast(text)
    } catch {
      setExplainLast(null)
    }
  }, [])

  // Preload executions, memory, and last-run explanation when agent changes
  useEffect(() => {
    if (!agentId) return
    setExplainLast(null)
    void loadExecutions(agentId)
    void loadMemory(agentId)
    void loadExplainLast(agentId)
  }, [agentId, loadExecutions, loadMemory, loadExplainLast])

  // Backends for id->name resolution
  useEffect(() => {
    let cancelled = false
    api.listLlmBackends()
      .then((res) => { if (!cancelled) setLlmBackends(res.backends) })
      .catch(() => {})
    return () => { cancelled = true }
  }, [])

  // Listen to WebSocket events for real-time agent status updates
  const agentIdRef = useRef(agentId)
  agentIdRef.current = agentId

  useEvents({
    enabled: !!agentId,
    eventTypes: ['AgentExecutionStarted', 'AgentExecutionCompleted'],
    onConnected: (connected) => {
      if (!connected) {
        setRealtimeStatus(null)
      }
    },
    onEvent: (event) => {
      const currentAgentId = agentIdRef.current
      if (!currentAgentId) return

      switch (event.type) {
        case 'AgentExecutionStarted': {
          const startedData = (event as AgentExecutionStartedEvent).data
          if (startedData.agent_id === currentAgentId) {
            setRealtimeStatus('Executing')
          }
          break
        }

        case 'AgentExecutionCompleted': {
          const completedData = (event as AgentExecutionCompletedEvent).data
          if (completedData.agent_id === currentAgentId) {
            setRealtimeStatus(null)
            void loadExecutions(currentAgentId)
            void loadMemory(currentAgentId)
            void loadExplainLast(currentAgentId)
            api.getAgent(currentAgentId).then(() => {
              onRefresh()
            }).catch(err => {
              handleError(err, { operation: 'Switch agent status', showToast: false })
            })
          }
          break
        }
      }
    },
  })

  // Empty state (only in non-inline mode)
  if (!agent && !inlineMode) {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="text-center text-muted-foreground">
          <Bot className="h-16 w-16 mx-auto mb-4 opacity-20" />
          <p className="text-lg">{t('agents:detail.selectAgent')}</p>
        </div>
      </div>
    )
  }

  if (!agent) return null

  const currentStatus = realtimeStatus || agent.status

  const formatDuration = (ms: number | undefined | null) => {
    if (ms === undefined || ms === null || Number.isNaN(ms) || ms < 0) {
      return '--'
    }
    if (ms < 1000) return `${ms}ms`
    return `${(ms / 1000).toFixed(1)}s`
  }

  const formatCount = (count: number | undefined | null) => {
    return count !== undefined && count !== null && !Number.isNaN(count) ? count : '--'
  }

  // Contract derivations
  const trigger = resolveTrigger(agent)
  const toolPolicyInfo = describeToolPolicy(agent.tool_policy, t as TFn)
  const notifyInfo = normalizeNotifyPolicy(agent.notify)
  const inputRequirements: InputRequirement[] = agent.input?.requirements || []
  const outputFields = agent.output?.fields || []
  const outputPublish = Boolean(agent.output?.publish)
  const controlledActions: ControlledAction[] = agent.controlled_actions || []
  const effectivePlan = agent.effective_plan

  // Role & stats derivations
  const hasOutputFields = outputFields.length > 0
  const role = (outputPublish || agent.execution_mode === 'structured') && hasOutputFields
    ? 'recordData'
    : (toolPolicyInfo.mode === 'unrestricted' || agent.execution_mode === 'free')
      ? 'actOrInvestigate'
      : 'answer'
  const RoleIcon = role === 'recordData' ? BarChart3 : role === 'actOrInvestigate' ? Workflow : MessageSquare

  const totalRuns = agent.stats?.total_runs ?? agent.stats?.total_executions ?? agent.execution_count
  const successfulRuns = agent.stats?.successful_runs ?? agent.stats?.successful_executions ?? agent.success_count
  const failedRuns = agent.stats?.failed_runs ?? agent.stats?.failed_executions ?? agent.error_count
  const avgDurationMs = agent.stats?.avg_duration_ms ?? agent.avg_duration_ms
  const partialRuns = agent.stats?.partial_runs ?? 0
  const runsToday = agent.stats?.runs_today ?? 0

  const eventFilterStr = trigger.kind === 'event' ? formatEventFilter(trigger.filter) : null

  const notifyTriggers: string[] = []
  if (notifyInfo.onComplete) notifyTriggers.push(t('agents:detail.notifyOnComplete'))
  if (notifyInfo.onAlert) notifyTriggers.push(t('agents:detail.notifyOnAlert'))
  if (notifyInfo.onFailure) notifyTriggers.push(t('agents:detail.notifyOnFailure'))
  if (notifyInfo.pendingConfirmation) notifyTriggers.push(t('agents:detail.notifyPendingConfirmation'))

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className={cn("flex min-h-0 flex-1", isMobile ? "flex-col" : "flex-row")}>
        <FullScreenDialogSidebar>
          <div className="p-2 space-y-1">
            {([
              { value: 'overview', Icon: Eye, label: t('agents:detail.overview'), desc: t('agents:detail.sectionsDesc.overview') },
              { value: 'history', Icon: History, label: t('agents:detail.history'), desc: t('agents:detail.sectionsDesc.history') },
              { value: 'memory', Icon: Brain, label: t('agents:detail.memory'), desc: t('agents:detail.sectionsDesc.memory') },
              { value: 'messages', Icon: MessageSquare, label: t('agents:detail.messages'), desc: t('agents:detail.sectionsDesc.messages') },
            ] as const).map(({ value, Icon, label, desc }) => {
              const isActive = section === value
              return (
                <button
                  key={value}
                  type="button"
                  onClick={() => setSection(value)}
                  className={cn(
                    "flex w-full items-center gap-3 px-3 py-2.5 rounded-lg transition-all text-left",
                    isActive
                      ? "bg-primary-light text-primary font-medium"
                      : "text-muted-foreground hover:bg-muted-50 hover:text-foreground"
                  )}
                >
                  <div className={cn(
                    "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
                    isActive ? "bg-primary-light text-primary" : "bg-muted-50"
                  )}>
                    <Icon className="h-4 w-4" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className={cn("truncate text-sm", isActive ? "text-foreground" : "")}>{label}</div>
                    <div className="truncate text-xs text-muted-foreground">{desc}</div>
                  </div>
                </button>
              )
            })}
          </div>
        </FullScreenDialogSidebar>

        {/* Mobile: horizontal section chips */}
        {isMobile && (
          <MobileTabStrip
            items={[
              { value: 'overview', label: t('agents:detail.overview') },
              { value: 'history', label: t('agents:detail.history') },
              { value: 'memory', label: t('agents:detail.memory') },
              { value: 'messages', label: t('agents:detail.messages') },
            ] as const}
            value={section}
            onValueChange={setSection}
            ariaLabel={t('agents:detailTitle')}
          />
        )}

        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        {section === 'messages' ? (
          <div className="min-h-0 flex-1 overflow-y-auto bg-muted-20">
            <div className={cn(
              "flex h-full flex-col",
              isMobile ? "px-3 py-3" : "px-5 py-4"
            )}>
              <AgentUserMessages
                agentId={agent.id}
                refreshToken={messagesVersion}
                onMessageAdded={onRefresh}
              />
            </div>
          </div>
        ) : (
        <ScrollArea className="h-0 min-h-0 flex-1 bg-muted-20">
          <div className={cn(
            "space-y-5",
            isMobile ? "px-3 py-3" : "px-5 py-4"
          )}>
            {section === 'overview' && (
              <>
                {/* Top metadata strip */}
                <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                  <span className="inline-flex items-center gap-1 rounded-md bg-muted-30 px-1.5 py-0.5">
                    <RoleIcon className="h-3.5 w-3.5" />
                    {t(`agents:card.role.${role}`)}
                  </span>
                  <span className="inline-flex items-center gap-1 rounded-md bg-muted-30 px-1.5 py-0.5">
                    <Wrench className="h-3.5 w-3.5" />
                    {toolPolicyInfo.label}
                  </span>
                  {agent.config_version != null && (
                    <span className="inline-flex items-center rounded-md bg-muted-30 px-1.5 py-0.5 font-mono">
                      {t('agents:detail.configVersion', { version: agent.config_version })}
                    </span>
                  )}
                  {currentStatus === 'Executing' && (
                    <Badge variant="secondary" className="h-5 px-1.5 text-xs text-info">
                      {t('agents:status.executing')}
                    </Badge>
                  )}
                  <span className={cn("text-xs text-muted-foreground tabular-nums", isMobile ? "w-full" : "ml-auto")}>
                    {t('common:updatedAt')} {formatTimestamp(agent.updated_at, false)}
                    {agent.last_execution_at != null && (
                      <> · {t('agents:lastExecution')} {formatTimestamp(agent.last_execution_at, false)}</>
                    )}
                  </span>
                </div>

                {/* Runtime stats */}
                <div className="rounded-2xl border border-border bg-card p-4 md:rounded-lg md:p-3">
                  <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
                    <div>
                      <div className="text-base font-semibold tabular-nums leading-tight">{formatCount(totalRuns)}</div>
                      <div className="text-xs text-muted-foreground">{t('agents:detail.executions')}</div>
                    </div>
                    <div>
                      <div className="text-base font-semibold tabular-nums leading-tight text-success">{formatCount(successfulRuns)}</div>
                      <div className="text-xs text-muted-foreground">{t('agents:detail.success')}</div>
                    </div>
                    <div>
                      <div className={cn(
                        "text-base font-semibold tabular-nums leading-tight",
                        failedRuns > 0 ? "text-error" : ""
                      )}>{formatCount(failedRuns)}</div>
                      <div className="text-xs text-muted-foreground">{t('agents:detail.failed')}</div>
                    </div>
                    <div>
                      <div className="text-base font-semibold tabular-nums leading-tight">{formatDuration(avgDurationMs)}</div>
                      <div className="text-xs text-muted-foreground">{t('agents:detail.avgDuration')}</div>
                    </div>
                  </div>
                  {(partialRuns > 0 || runsToday > 0) && (
                    <div className="mt-2.5 flex flex-wrap items-center gap-3 border-t border-border pt-2 text-xs text-muted-foreground tabular-nums">
                      {partialRuns > 0 && (
                        <span>
                          {t('agents:detail.partialRuns')}: <strong className="font-medium text-warning">{partialRuns}</strong>
                        </span>
                      )}
                      {runsToday > 0 && (
                        <span>
                          {t('agents:detail.runsToday')}:{' '}
                          <strong className="font-medium text-foreground">
                            {runsToday}
                            {agent.pacing?.max_runs_per_day != null ? ` / ${agent.pacing.max_runs_per_day}` : ''}
                          </strong>
                        </span>
                      )}
                    </div>
                  )}
                </div>

                {/* Explain Last Run */}
                {explainLast && (
                  <div className="rounded-2xl border border-border bg-card p-4 md:rounded-lg">
                    <h4 className="mb-1.5 flex items-center gap-1.5 text-sm font-medium text-foreground">
                      <Info className="h-4 w-4 text-primary" />
                      {t('agents:detail.explainLast')}
                    </h4>
                    <p className="whitespace-pre-wrap text-xs leading-relaxed text-muted-foreground">
                      {explainLast}
                    </p>
                  </div>
                )}

                {/* User Prompt */}
                <DetailSection title={t('agents:userPrompt')} icon={FileText}>
                  <div className="text-sm leading-relaxed whitespace-pre-wrap text-foreground">
                    {agent.user_prompt || t('agents:card.noDescription')}
                  </div>
                </DetailSection>

                {/* Output Contract */}
                {hasOutputFields && (
                  <DetailSection title={t('agents:detail.outputFields')} icon={Database}>
                    <div className="space-y-2">
                      <div className="flex items-center gap-2">
                        <Badge variant={outputPublish ? 'default' : 'secondary'} className="text-xs">
                          {outputPublish ? t('agents:detail.outputPublishOn') : t('agents:detail.outputPublishOff')}
                        </Badge>
                      </div>
                      <div className="space-y-1">
                        {outputFields.map((f) => {
                          const state = agent.latest_output_state?.[f.name]
                          const latest = state?.value ?? agent.latest_output?.[f.name]
                          return (
                            <div key={f.name} className="border-b border-border py-1.5 last:border-0">
                              <div className="flex items-baseline gap-2">
                                <span className="font-mono text-xs font-medium">{f.name}</span>
                                {f.description && (
                                  <span className="truncate text-xs text-muted-foreground">{f.description}</span>
                                )}
                                <span className="ml-auto shrink-0 text-sm font-medium tabular-nums">
                                  {latest !== undefined ? String(latest) : '—'}
                                </span>
                              </div>
                              {f.depends_on && f.depends_on.length > 0 && (
                                <div className="mt-0.5 text-xs text-muted-foreground">
                                  {t('agents:detail.outputDependsOn', { deps: f.depends_on.join(', ') })}
                                </div>
                              )}
                              {latest !== undefined && (
                                <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
                                  {state && <span>{formatRelativeTime(state.at)}</span>}
                                  {state?.confidence !== undefined && (
                                    <span>{Math.round(state.confidence * 100)}%</span>
                                  )}
                                  {state?.execution_id && (
                                    <button
                                      type="button"
                                      onClick={() =>
                                        onViewExecutionDetail(agent.id, state.execution_id!)
                                      }
                                      className="ml-auto inline-flex items-center gap-0.5 text-primary transition-colors hover:underline"
                                    >
                                      {t('agents:detail.viewEvidence')}
                                      <ChevronRight className="h-3 w-3" />
                                    </button>
                                  )}
                                </div>
                              )}
                            </div>
                          )
                        })}
                        {outputPublish && (
                          <p className="pt-1 text-xs text-muted-foreground">{t('agents:detail.outputFieldsHint')}</p>
                        )}
                      </div>
                    </div>
                  </DetailSection>
                )}

                {/* Trigger, Model, Tool Policy, Notify, Pacing */}
                <div className="rounded-2xl border border-border bg-card p-4 md:rounded-lg space-y-4">
                  {/* Trigger */}
                  <div className="space-y-1.5">
                    <h4 className="text-sm font-medium">{t('agents:detail.trigger')}</h4>
                    <InfoRow
                      label={t('agents:detail.type')}
                      value={t(`agents:editorV2.trigger.${trigger.kind}`, { defaultValue: trigger.kind })}
                    />
                    {trigger.kind === 'interval' && (
                      <InfoRow
                        label={t('agents:detail.interval')}
                        value={formatInterval(trigger.every_secs, t as TFn)}
                      />
                    )}
                    {trigger.kind === 'cron' && (
                      <>
                        <InfoRow label={t('agents:detail.cron')} value={trigger.expr} mono />
                        {trigger.timezone && (
                          <InfoRow label={t('agents:detail.timezone')} value={trigger.timezone} mono />
                        )}
                      </>
                    )}
                    {trigger.kind === 'event' && eventFilterStr && (
                      <InfoRow label={t('agents:detail.eventFilter')} value={eventFilterStr} mono />
                    )}
                    {trigger.kind === 'manual' && (
                      <InfoRow label={t('agents:detail.schedule')} value={t('agents:detail.manualOnly')} />
                    )}
                  </div>

                  {/* LLM Backend */}
                  <div className="space-y-1.5 border-t border-border pt-4">
                    <h4 className="text-sm font-medium">{t('agents:creator.basicInfo.llmBackend')}</h4>
                    {(() => {
                      const backend = llmBackends.find((b) => b.id === agent.llm_backend_id)
                      if (backend) {
                        return (
                          <>
                            <InfoRow label={t('agents:detail.model')} value={backend.name} />
                            {backend.model && (
                              <InfoRow label={t('agents:detail.modelId')} value={backend.model} mono />
                            )}
                          </>
                        )
                      }
                      return (
                        <InfoRow
                          label={t('agents:detail.model')}
                          value={agent.llm_backend_id
                            ? `${agent.llm_backend_id.slice(0, 8)}…`
                            : t('agents:creator.basicInfo.useActiveBackend')}
                          mono={!!agent.llm_backend_id}
                        />
                      )
                    })()}
                  </div>

                  {/* Tool Policy */}
                  <div className="space-y-1.5 border-t border-border pt-4">
                    <h4 className="flex items-center gap-1.5 text-sm font-medium">
                      <Shield className="h-3.5 w-3.5 text-muted-foreground" />
                      {t('agents:detail.toolPolicy')}
                    </h4>
                    <InfoRow label={t('agents:detail.type')} value={toolPolicyInfo.label} />
                    {toolPolicyInfo.allowedTools.length > 0 && (
                      <InfoRow
                        label={t('agents:detail.allowedTools')}
                        value={toolPolicyInfo.allowedTools.join(', ')}
                        mono
                      />
                    )}
                    {toolPolicyInfo.allowedCommands.length > 0 && (
                      <InfoRow
                        label={t('agents:detail.allowedCommands')}
                        value={toolPolicyInfo.allowedCommands.join(', ')}
                        mono
                      />
                    )}
                  </div>

                  {/* Notification Policy */}
                  <div className="space-y-1.5 border-t border-border pt-4">
                    <h4 className="flex items-center gap-1.5 text-sm font-medium">
                      <Bell className="h-3.5 w-3.5 text-muted-foreground" />
                      {t('agents:detail.notifyPolicy')}
                    </h4>
                    <InfoRow
                      label={t('agents:detail.notifyTriggers')}
                      value={notifyTriggers.length > 0 ? notifyTriggers.join(' · ') : t('agents:detail.notifySilent')}
                    />
                    <InfoRow
                      label={t('agents:detail.notifyChannels')}
                      value={
                        notifyInfo.channels.length > 0
                          ? notifyInfo.channels.join(', ')
                          : t('agents:detail.notifyChannelsEmpty')
                      }
                      mono={notifyInfo.channels.length > 0}
                    />
                  </div>

                  {/* Pacing & Guardrails */}
                  {hasPacingSettings(agent.pacing) && (
                    <div className="space-y-1.5 border-t border-border pt-4">
                      <h4 className="flex items-center gap-1.5 text-sm font-medium">
                        <Gauge className="h-3.5 w-3.5 text-muted-foreground" />
                        {t('agents:detail.pacing')}
                      </h4>
                      {agent.pacing?.burst_merge_secs != null && (
                        <InfoRow label={t('agents:detail.burstMerge')} value={`${agent.pacing.burst_merge_secs}s`} />
                      )}
                      {agent.pacing?.min_interval_secs != null && (
                        <InfoRow label={t('agents:detail.minInterval')} value={`${agent.pacing.min_interval_secs}s`} />
                      )}
                      {agent.pacing?.max_runs_per_day != null && (
                        <InfoRow label={t('agents:detail.maxRunsPerDay')} value={agent.pacing.max_runs_per_day} />
                      )}
                      {agent.pacing?.failure_threshold != null && (
                        <InfoRow label={t('agents:detail.failureThreshold')} value={agent.pacing.failure_threshold} />
                      )}
                    </div>
                  )}
                </div>

                {/* Input Requirements */}
                <div className="md:rounded-lg md:border md:border-border md:bg-card md:p-4">
                  <DetailSection title={`${t('agents:detail.inputRequirements')} (${inputRequirements.length})`} icon={Zap}>
                    {inputRequirements.length === 0 ? (
                      <p className="text-xs text-muted-foreground">{t('agents:detail.noInputs')}</p>
                    ) : (
                      <div className="space-y-2">
                        {inputRequirements.map((req) => (
                          <div key={req.key} className="rounded-lg border border-border bg-muted-20 px-3 py-2">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="font-mono text-xs font-medium text-foreground">{req.key}</span>
                              <span className="min-w-0 truncate font-mono text-xs text-muted-foreground">
                                {formatInputSource(req.source)}
                                {req.operation ? ` / ${req.operation}` : ''}
                              </span>
                              <Badge variant="outline" className="ml-auto h-5 px-1.5 text-xs">
                                {req.result_type}
                              </Badge>
                            </div>
                            <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
                              <span>
                                {t('agents:detail.freshness')}: {formatFreshness(req.freshness, t as TFn)}
                              </span>
                              <span>
                                {t('agents:detail.onMissing')}: {formatMissingPolicy(req.on_missing, t as TFn)}
                              </span>
                            </div>
                          </div>
                        ))}
                      </div>
                    )}
                  </DetailSection>
                </div>

                {/* Controlled Actions */}
                {controlledActions.length > 0 && (
                  <div className="md:rounded-lg md:border md:border-border md:bg-card md:p-4">
                    <DetailSection title={`${t('agents:detail.controlledActions')} (${controlledActions.length})`} icon={Workflow}>
                      <div className="space-y-1.5">
                        {controlledActions.map((action, idx) => (
                          <div key={`${action.name}-${idx}`} className="flex flex-wrap items-center gap-2 border-b border-border py-1.5 last:border-0">
                            <span className="text-sm font-medium text-foreground">{action.name}</span>
                            <span className="font-mono text-xs text-muted-foreground truncate">{action.destination}</span>
                            {action.idempotent && (
                              <Badge variant="secondary" className="ml-auto h-5 px-1.5 text-xs">
                                {t('agents:detail.idempotent')}
                              </Badge>
                            )}
                          </div>
                        ))}
                      </div>
                    </DetailSection>
                  </div>
                )}

                {/* Effective Plan */}
                {effectivePlan && (
                  <div className="rounded-2xl border border-dashed border-border bg-muted-20 p-4 md:rounded-lg space-y-2.5">
                    <h4 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      <Sparkles className="h-3.5 w-3.5" />
                      {t('agents:detail.effectivePlan')}
                    </h4>
                    <InfoRow label={t('agents:detail.trigger')} value={effectivePlan.trigger_summary} mono />
                    {effectivePlan.authorized_reads.length > 0 && (
                      <div className="space-y-1 pt-1">
                        <div className="text-xs text-muted-foreground">
                          {t('agents:detail.authorizedReads')} ({effectivePlan.authorized_reads.length})
                        </div>
                        <ul className="space-y-0.5">
                          {effectivePlan.authorized_reads.map((read) => (
                            <li key={read.key} className="font-mono text-xs text-muted-foreground">
                              {read.key} → {read.source_id}
                              {read.operation ? ` / ${read.operation}` : ''}
                            </li>
                          ))}
                        </ul>
                      </div>
                    )}
                    {effectivePlan.warnings && effectivePlan.warnings.length > 0 && (
                      <div className="space-y-1 rounded-md border border-warning/40 bg-warning/5 px-3 py-2">
                        <div className="flex items-center gap-1.5 text-xs font-medium text-warning">
                          <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                          {t('agents:detail.planWarnings')}
                        </div>
                        <ul className="space-y-0.5 text-xs text-warning">
                          {effectivePlan.warnings.map((w, idx) => (
                            <li key={idx}>{w}</li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </div>
                )}
              </>
            )}
            {section === 'history' && (
              <>
                <p className="text-xs text-muted-foreground">
                  {t('agents:detail.evidenceBrowseHint')}
                </p>
                <AgentExecutionTimeline
                  executions={executions}
                  loading={executionsLoading}
                  agentId={agent.id}
                  onViewExecutionDetail={onViewExecutionDetail}
                />
              </>
            )}
            {section === 'memory' && (
              <>
                <div className="mb-3 flex items-center gap-1.5 rounded-md bg-muted-30 px-2.5 py-1.5 text-xs text-muted-foreground">
                  <Brain className="h-3.5 w-3.5 shrink-0" />
                  <span>{t('agents:detail.memoryNote')}</span>
                </div>
                <MemoryContent memory={memory} loading={memoryLoading} />
              </>
            )}
          </div>
        </ScrollArea>
        )}
        {section === 'messages' && (
          <div className={cn(
            "shrink-0 border-t border-border bg-muted-20",
            isMobile ? "px-3 py-2.5" : "px-5 py-3"
          )}>
            <AgentUserMessagesComposer
              agentId={agent.id}
              onSent={() => setMessagesVersion(v => v + 1)}
            />
          </div>
        )}
        </div>
      </div>
    </div>
  )
}

// ============================================================================
// Sub Components
// ============================================================================

interface DetailSectionProps {
  title: string
  icon: React.ComponentType<{ className?: string }> | null
  children: React.ReactNode
}

function DetailSection({ title, icon: Icon, children }: DetailSectionProps) {
  return (
    <div className="rounded-2xl border border-border bg-card p-4 md:rounded-none md:border-0 md:bg-transparent md:p-0">
      {title && Icon && (
        <h3 className="text-sm font-medium flex items-center gap-2 mb-3 text-foreground">
          <Icon className="h-4 w-4" />
          {title}
        </h3>
      )}
      {children}
    </div>
  )
}

interface InfoRowProps {
  label: string
  value: string | number
  mono?: boolean
}

function InfoRow({ label, value, mono }: InfoRowProps) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-1">
      <span className="shrink-0 text-xs text-muted-foreground">{label}</span>
      <span className={cn("truncate text-sm font-medium", mono && "font-mono")}>{value}</span>
    </div>
  )
}

// ============================================================================
// Knowledge File Card — expandable with markdown rendering
// ============================================================================

function KnowledgeFileCard({
  file,
  formatTime,
  emptyLabel,
}: {
  file: KnowledgeFileRef
  formatTime: (ts: string | number) => string
  emptyLabel: string
}) {
  const [expanded, setExpanded] = useState(false)
  const hasContent = file.content && file.content.trim().length > 0

  return (
    <div className="rounded-lg bg-card border border-border hover:border-foreground/30 transition-colors overflow-hidden">
      <button
        type="button"
        className="w-full p-3 text-left flex items-center justify-between gap-2"
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
      >
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-0.5">
            <FileText className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
            <span className="text-sm font-medium font-mono truncate">{file.name}</span>
          </div>
          <p className="text-xs text-muted-foreground line-clamp-1">{file.description}</p>
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <span className="text-xs text-muted-foreground">{formatTime(file.updated_at)}</span>
          <ChevronDown className={cn("h-3.5 w-3.5 text-muted-foreground transition-transform", expanded && "rotate-180")} />
        </div>
      </button>
      {expanded && (
        <div className="px-3 pb-3 pt-0 border-t border-border">
          {hasContent ? (
            <div className="mt-2 prose prose-sm dark:prose-invert max-w-none prose-p:leading-relaxed prose-p:my-1 prose-headings:font-semibold prose-headings:my-2 prose-h1:text-base prose-h2:text-sm prose-h3:text-xs prose-code:rounded prose-code:bg-muted prose-code:px-1 prose-code:py-0.5 prose-code:text-xs prose-code:font-mono prose-pre:bg-muted prose-pre:p-2 prose-pre:rounded-md prose-ul:my-1 prose-ul:pl-4 prose-ol:my-1 prose-ol:pl-4 prose-li:my-0 prose-li:text-xs">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>
                {file.content ?? ''}
              </ReactMarkdown>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground italic mt-2">{emptyLabel}</p>
          )}
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Memory Content - Structured and readable display
// ============================================================================

interface MemoryContentProps {
  memory: AgentMemory | null
  loading: boolean
}

function MemoryContent({ memory, loading }: MemoryContentProps) {
  const { t } = useTranslation(['common', 'agents'])
  const isMobile = useIsMobile()

  if (loading) {
    return (
      <LoadingState size="md" className="py-12" />
    )
  }

  if (!memory) {
    return (
      <div className="flex flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-12 text-center">
        <Brain className="h-8 w-8 text-muted-foreground opacity-40" />
        <p className="text-sm text-muted-foreground">{t('agents:detail.noMemory')}</p>
      </div>
    )
  }

  const journalRecords = memory.journal?.records || []
  const knowledgeFiles = memory.knowledge_files || []
  const isEmptyMemory = journalRecords.length === 0 && knowledgeFiles.length === 0

  if (isEmptyMemory) {
    return (
      <div className="flex flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-12 text-center">
        <Brain className="h-8 w-8 text-muted-foreground opacity-40" />
        <p className="text-sm text-muted-foreground">{t('agents:detail.noMemory')}</p>
        <p className="text-xs text-muted-foreground opacity-70">{t('agents:memory.emptyHint')}</p>
      </div>
    )
  }

  const formatTime = (timestamp: string | number) => {
    const ts = typeof timestamp === 'number' ? timestamp * 1000 : new Date(timestamp).getTime()
    const date = new Date(ts)
    const now = new Date()
    const diff = now.getTime() - date.getTime()
    const minutes = Math.floor(diff / 60000)
    const hours = Math.floor(diff / 3600000)
    const days = Math.floor(diff / 86400000)

    if (minutes < 1) return t('agents:time.justNow')
    if (minutes < 60) return t('agents:time.minutesAgo', { count: minutes })
    if (hours < 24) return t('agents:time.hoursAgo', { count: hours })
    return t('agents:time.daysAgo', { count: days })
  }

  return (
    <div className="space-y-4">
      {/* Stats */}
      <div className="rounded-lg bg-card border border-border p-4">
        <div className={cn("grid gap-4", isMobile ? "grid-cols-2" : "grid-cols-3")}>
          <div>
            <div className="text-base font-semibold tabular-nums leading-tight">{journalRecords.length}</div>
            <div className="text-xs text-muted-foreground">{t('agents:memory.executions')}</div>
          </div>
          <div>
            <div className="text-base font-semibold tabular-nums leading-tight">{knowledgeFiles.length}</div>
            <div className="text-xs text-muted-foreground">{t('agents:memory.knowledgeFiles')}</div>
          </div>
          {memory.updated_at && (
            <div>
              <div className="text-base font-semibold leading-tight">{formatTime(memory.updated_at)}</div>
              <div className="text-xs text-muted-foreground">{t('agents:memory.lastUpdate')}</div>
            </div>
          )}
        </div>
      </div>

      {/* Knowledge Files */}
      {knowledgeFiles.length > 0 && (
        <DetailSection
          title={`${t('agents:memory.knowledgeFiles')} (${knowledgeFiles.length})`}
          icon={FileText}
        >
          <div className="space-y-2">
            {knowledgeFiles.map((file: KnowledgeFileRef) => (
              <KnowledgeFileCard
                key={file.name}
                file={file}
                formatTime={formatTime}
                emptyLabel={t('agents:detail.noContent')}
              />
            ))}
          </div>
        </DetailSection>
      )}

      {/* Execution Journal */}
      {journalRecords.length > 0 && (
        <DetailSection
          title={`${t('agents:memory.journal')} (${journalRecords.length}/${memory.journal?.max_records || 10})`}
          icon={History}
        >
          <div className="space-y-2">
            {journalRecords.map((record: JournalExecutionRecord, idx: number) => (
              <div key={idx} className="group rounded-lg border border-border bg-card hover:border-foreground/25 transition-colors">
                <div className="pl-4 pr-3 py-3">
                  <div className="flex items-center justify-between mb-1">
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-mono text-muted-foreground bg-muted px-1.5 py-0.5 rounded">
                        {record.execution_id?.slice(0, 6)}...
                      </span>
                      <span className="text-xs text-muted-foreground">
                        {formatTime(record.timestamp)}
                      </span>
                    </div>
                    <Badge variant={record.success ? 'default' : 'destructive'} className="text-xs">
                      {record.success ? t('agents:executionStatus.completed') : t('agents:executionStatus.failed')}
                    </Badge>
                  </div>
                  <p className="text-sm">{record.outcome}</p>
                  <div className="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground">
                    <Zap className="h-3.5 w-3.5" />
                    <span>{record.action_taken}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        </DetailSection>
      )}
    </div>
  )
}

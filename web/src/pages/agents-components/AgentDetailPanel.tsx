/**
 * Agent Detail Panel - Right side of Agents page
 *
 * Shows detailed view of a selected agent with tabs.
 */

import { useState, useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { LoadingState } from "@/components/shared/LoadingState"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Database,
  Eye,
  Workflow,
  BrainCircuit,
  Wrench,
  Bot,
  Clock,
  Activity,
  Brain,
  Zap,
  BarChart3,
  FileText,


  MessageSquare,
  History,


  ChevronDown,
} from "lucide-react"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { useIsMobile } from "@/hooks/useMobile"
import { api } from "@/lib/api"
import type { AiAgentDetail, AgentAvailableResources, AgentExecution, AgentMemory, KnowledgeFileRef, JournalExecutionRecord } from "@/types"
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { AgentExecutionStartedEvent, AgentExecutionCompletedEvent } from "@/lib/events"
import { FullScreenDialogSidebar } from "@/components/automation/dialog/FullScreenDialog"
import { useEvents } from "@/hooks/useEvents"

// Import sub-components
import { AgentExecutionTimeline } from "./AgentExecutionTimeline"
import { AgentThinkingPanel } from "./AgentThinkingPanel"
import { AgentUserMessages } from "./AgentUserMessages"

interface AgentDetailPanelProps {
  agent: AiAgentDetail | null
  onEdit: (agent: AiAgentDetail) => void
  onExecute: (agent: AiAgentDetail) => void
  onViewExecutionDetail: (agentId: string, executionId: string) => void
  onRefresh: () => void
  inlineMode?: boolean  // When true, used inside dialog (no empty state)
}

// Role configuration - labels use i18n
const _ROLE_CONFIG: Record<string, { icon: typeof Activity; color: string }> = {
  Monitor: { icon: Activity, color: 'text-info' },
  Executor: { icon: Zap, color: 'text-accent-orange' },
  Analyst: { icon: BarChart3, color: 'text-accent-purple' },
}

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
  const [_availableResources, setAvailableResources] = useState<AgentAvailableResources | null>(null)

  // Real-time status from WebSocket events
  const [realtimeStatus, setRealtimeStatus] = useState<string | null>(null)
  const [section, setSection] = useState<'overview' | 'history' | 'memory' | 'messages'>('overview')

  // Load executions immediately when agent is selected (preload)
  // and when switching back to history tab with stale data
  useEffect(() => {
    if (agent?.id) {
      loadExecutions()
    }
  }, [agent?.id])

  // Memory loads with the agent — the panel is a single page now
  useEffect(() => {
    if (agent?.id) {
      loadMemory()
    }
  }, [agent?.id])

  // Load available resources
  useEffect(() => {
    if (agent?.id) {
      loadAvailableResources()
    }
  }, [agent?.id])

  // Listen to WebSocket events for real-time agent status updates
  // Use ref to avoid stale closure over agent ID
  const agentIdRef = useRef(agent?.id)
  agentIdRef.current = agent?.id

  useEvents({
    enabled: !!agent?.id,
    eventTypes: ['AgentExecutionStarted', 'AgentExecutionCompleted'],
    onConnected: (connected) => {
      if (!connected) {
        // Connection lost - clear realtime executing status
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
            // Clear realtime status - agent's original status will be used
            setRealtimeStatus(null)
            // Reload executions silently to include the just-completed one
            loadExecutions()
            // Reload agent data to get updated stats
            api.getAgent(currentAgentId).then(() => {
              // Notify parent to refresh if needed
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

  const loadExecutions = async () => {
    if (!agent) return
    setExecutionsLoading(true)
    try {
      const data = await api.getAgentExecutions(agent.id)
      setExecutions(data.executions || [])
    } catch (error) {
      handleError(error, { operation: 'Load agent executions', showToast: false })
    } finally {
      setExecutionsLoading(false)
    }
  }

  const loadMemory = async () => {
    if (!agent) return
    setMemoryLoading(true)
    try {
      const data = await api.getAgentMemory(agent.id)
      setMemory(data)
    } catch (error) {
      handleError(error, { operation: 'Load agent memory', showToast: false })
    } finally {
      setMemoryLoading(false)
    }
  }

  const loadAvailableResources = async () => {
    if (!agent?.id) return

    try {
      const resources = await api.getAgentAvailableResources(agent.id)
      setAvailableResources(resources)
    } catch (error) {
      handleError(error, { operation: 'Load available resources', showToast: false })
    }
  }

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

  // Return null if no agent in inline mode (dialog will handle it)
  if (!agent) return null

  // Use realtime status from WebSocket if available, otherwise use agent's status
  const currentStatus = realtimeStatus || agent.status

  // Format duration - handles undefined/null/NaN values
  const formatDuration = (ms: number | undefined | null) => {
    if (ms === undefined || ms === null || Number.isNaN(ms) || ms < 0) {
      return '--'
    }
    if (ms < 1000) return `${ms}ms`
    return `${(ms / 1000).toFixed(1)}s`
  }

  // Safe number to string conversion
  const formatCount = (count: number | undefined | null) => {
    return count !== undefined && count !== null && !Number.isNaN(count) ? count : '--'
  }

  return (
    <div className="flex flex-col gap-3 h-full">
      {/* Real-time Thinking Panel - shows during execution */}
      {agent.id && (
        <AgentThinkingPanel
          agentId={agent.id}
          isExecuting={currentStatus === 'Executing'}
        />
      )}

      {/* Left-rail section nav + content — matches the app's quiet list language */}
      <div className="flex min-h-0 flex-1">
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
          <div className="flex gap-1 overflow-x-auto px-2 pt-2">
            {([
              { value: 'overview', label: t('agents:detail.overview') },
              { value: 'history', label: t('agents:detail.history') },
              { value: 'memory', label: t('agents:detail.memory') },
              { value: 'messages', label: t('agents:detail.messages') },
            ] as const).map(({ value, label }) => (
              <button
                key={value}
                type="button"
                onClick={() => setSection(value)}
                className={cn(
                  "shrink-0 rounded-full border px-3 py-1 text-xs transition-colors",
                  section === value
                    ? "border-foreground bg-foreground text-background"
                    : "border-border text-muted-foreground"
                )}
              >
                {label}
              </button>
            ))}
          </div>
        )}

        <ScrollArea className="min-w-0 flex-1 bg-muted-20">
          <div className={cn("space-y-5", isMobile ? "px-3 py-3" : "px-5 py-4")}>
            {section === 'overview' && (
              <>
                              <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                                {(() => {
                                  const hasContract = (agent.output_schema?.length ?? 0) > 0
                                  const role = agent.execution_mode === 'structured' && hasContract
                                    ? 'recordData'
                                    : agent.execution_mode === 'free' ? 'actOrInvestigate' : 'answer'
                                  const RoleIcon = role === 'recordData' ? BarChart3 : role === 'actOrInvestigate' ? Workflow : MessageSquare
                                  const isTool = (agent.memory_mode ?? (agent.execution_mode === 'structured' ? 'tool' : 'assistant')) === 'tool'
                                  const MemIcon = isTool ? Wrench : BrainCircuit
                                  return (
                                    <>
                                      <span className="inline-flex items-center gap-1 rounded-md bg-muted-30 px-1.5 py-0.5">
                                        <RoleIcon className="h-3.5 w-3.5" />{t(`agents:card.role.${role}`)}
                                      </span>
                                      <span className="inline-flex items-center gap-1 rounded-md bg-muted-30 px-1.5 py-0.5">
                                        <MemIcon className="h-3.5 w-3.5" />{t(`agents:card.memory.${isTool ? 'tool' : 'assistant'}`)}
                                      </span>
                                    </>
                                  )
                                })()}
                              </div>

                              <div className="grid grid-cols-4 gap-3 rounded-lg bg-card p-3 border border-border">
                                <div>
                                  <div className="text-base font-semibold tabular-nums leading-tight">{formatCount(agent.stats?.total_executions ?? agent.execution_count)}</div>
                                  <div className="text-xs text-muted-foreground">{t('agents:detail.executions')}</div>
                                </div>
                                <div>
                                  <div className="text-base font-semibold tabular-nums leading-tight text-success">{formatCount(agent.stats?.successful_executions ?? agent.success_count)}</div>
                                  <div className="text-xs text-muted-foreground">{t('agents:detail.success')}</div>
                                </div>
                                <div>
                                  <div className={cn(
                                    "text-base font-semibold tabular-nums leading-tight",
                                    (agent.stats?.failed_executions ?? agent.error_count) > 0 ? "text-error" : ""
                                  )}>{formatCount(agent.stats?.failed_executions ?? agent.error_count)}</div>
                                  <div className="text-xs text-muted-foreground">{t('agents:detail.failed')}</div>
                                </div>
                                <div>
                                  <div className="text-base font-semibold tabular-nums leading-tight">{formatDuration(agent.stats?.avg_duration_ms ?? agent.avg_duration_ms)}</div>
                                  <div className="text-xs text-muted-foreground">{t('agents:detail.avgDuration')}</div>
                                </div>
                              </div>

                              <DetailSection title={t('agents:userPrompt')} icon={FileText}>
                                <div className="text-sm leading-relaxed whitespace-pre-wrap text-foreground">
                                  {agent.user_prompt || t('agents:card.noDescription')}
                                </div>
                              </DetailSection>

                              {(agent.output_schema?.length ?? 0) > 0 && (
                                <DetailSection title={t('agents:detail.outputFields')} icon={Database}>
                                  <div className="space-y-1">
                                    {agent.output_schema!.map((f) => {
                                      const latest = (agent as { latest_output?: Record<string, unknown> }).latest_output?.[f.name]
                                      return (
                                        <div key={f.name} className="flex items-baseline gap-2 border-b border-border py-1.5 last:border-0">
                                          <span className="font-mono text-xs">{f.name}</span>
                                          <span className="text-xs text-muted-foreground truncate">
                                            {f.field_type.type === 'enum' ? `enum(${f.field_type.values.join('/')})` : t(`agents:detail.fieldType.${f.field_type.type}`)}
                                            {f.unit ? ` · ${f.unit}` : ''}
                                          </span>
                                          <span className="ml-auto shrink-0 text-sm font-medium tabular-nums">
                                            {latest !== undefined ? String(latest) : '—'}
                                          </span>
                                        </div>
                                      )
                                    })}
                                    <p className="pt-1 text-xs text-muted-foreground">{t('agents:detail.outputFieldsHint')}</p>
                                  </div>
                                </DetailSection>
                              )}

                              <div className={cn("grid gap-4", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                                <div className="rounded-lg bg-card border border-border p-4">
                                <DetailSection title={t('agents:detail.schedule')} icon={Clock}>
                                  <div className="space-y-1">
                                    <InfoRow label={t('agents:detail.type')} value={agent.schedule.schedule_type} />
                                    {agent.schedule.interval_seconds && (
                                      <InfoRow label={t('agents:detail.interval')} value={`${agent.schedule.interval_seconds}s`} />
                                    )}
                                    {agent.schedule.cron_expression && (
                                      <InfoRow label={t('agents:detail.cron')} value={agent.schedule.cron_expression} mono />
                                    )}
                                    {agent.schedule.event_filter && (
                                      <InfoRow label={t('agents:creator.schedule.event.triggerEvent')} value={agent.schedule.event_filter} mono />
                                    )}
                                  </div>
                                </DetailSection>

                                <DetailSection title={t('agents:creator.basicInfo.llmBackend')} icon={Brain}>
                                  <div className="space-y-1">
                                    <InfoRow label={t('agents:detail.model')} value={agent.llm_backend_id || t('agents:creator.basicInfo.useActiveBackend')} mono={!!agent.llm_backend_id} />
                                    <InfoRow label={t('common:priority')} value={agent.priority ?? '-'} />
                                  </div>
                                </DetailSection>
                                </div>
                              </div>

                              <div className="rounded-lg bg-card border border-border p-4">
                <DetailSection title={`${t('agents:detail.resources')} (${(agent.resources || []).length})`} icon={Zap}>
                                <div className="space-y-2">
                                  {(agent.resources || []).length > 0 && (
                                    <p className="text-xs text-muted-foreground">
                                      {Object.entries(
                                        (agent.resources || []).reduce((acc, r) => {
                                          const type = r.resource_type.toLowerCase()
                                          acc[type] = (acc[type] || 0) + 1
                                          return acc
                                        }, {} as Record<string, number>)
                                      ).map(([type, count]) => `${count} × ${type}`).join(' · ')}
                                    </p>
                                  )}
                                  <div className={cn("gap-x-4 gap-y-1", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                                    {(agent.resources || []).slice(0, 8).map((resource, idx) => (
                                      <div key={idx} className="flex items-baseline gap-2 border-b border-border py-1 last:border-0">
                                        <span className="min-w-0 flex-1 truncate text-sm" title={resource.resource_id}>
                                          {resource.name || resource.resource_id}
                                        </span>
                                        <span className="shrink-0 text-xs text-muted-foreground">{resource.resource_type}</span>
                                      </div>
                                    ))}
                                  </div>
                                  {(agent.resources || []).length > 8 && (
                                    <div className="text-xs text-muted-foreground pt-0.5">
                                      {t('agents:detail.moreResources', { count: (agent.resources || []).length - 8 })}
                                    </div>
                                  )}
                                </div>
                              </DetailSection>
                </div>

                              <p className="border-t border-border pt-3 text-xs text-muted-foreground">
                                {t('common:createdAt')} {new Date(agent.created_at).toLocaleString()}
                                {' · '}
                                {t('common:updatedAt')} {new Date(agent.updated_at).toLocaleString()}
                                {agent.last_execution_at && (
                                  <>{' · '}{t('agents:lastExecution')} {new Date(agent.last_execution_at).toLocaleString()}</>
                                )}
                              </p>
              </>
            )}
            {section === 'history' && (
              <AgentExecutionTimeline
                            executions={executions}
                            loading={executionsLoading}
                            agentId={agent.id}
                            onViewExecutionDetail={onViewExecutionDetail}
                          />
            )}
            {section === 'memory' && (
              <>
              <div className="mb-3 flex items-center gap-1.5 rounded-md bg-muted-30 px-2 py-1.5 text-xs text-muted-foreground">
                            {(() => {
                              const isTool = (agent.memory_mode ?? (agent.execution_mode === 'structured' ? 'tool' : 'assistant')) === 'tool'
                              const MemIcon = isTool ? Wrench : BrainCircuit
                              return (
                                <>
                                  <MemIcon className="h-3.5 w-3.5 shrink-0" />
                                  <span>{t(`agents:detail.memoryModeNote.${isTool ? 'tool' : 'assistant'}`)}</span>
                                </>
                              )
                            })()}
                          </div>
                          <MemoryContent memory={memory} loading={memoryLoading} />
              </>
            )}
            {section === 'messages' && (
              <AgentUserMessages
                            agentId={agent.id}
                            onMessageAdded={() => {
                              // Refresh agent data to show updated message count
                              onRefresh()
                            }}
                          />
            )}
          </div>
        </ScrollArea>
      </div>
    </div>
  )
}

// ============================================================================
// Sub Components
// ============================================================================

// Unified Section Component for all detail displays
interface DetailSectionProps {
  title: string
  icon: React.ComponentType<{ className?: string }> | null
  children: React.ReactNode
}

function DetailSection({ title, icon: Icon, children }: DetailSectionProps) {
  return (
    <div>
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


// Info Row Component — definition-list row: muted label left, value right
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

function KnowledgeFileCard({ file, formatTime }: { file: KnowledgeFileRef; formatTime: (ts: string | number) => string }) {
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
            <p className="text-xs text-muted-foreground italic mt-2">No content available</p>
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
      <LoadingState size="md" className="h-full" />
    )
  }

  if (!memory) {
    return (
      <div className="h-full flex flex-col items-center justify-center text-muted-foreground">
        <Brain className="h-12 w-12 mb-3 opacity-20" />
        <p className="text-sm">{t('agents:detail.noMemory')}</p>
      </div>
    )
  }

  const journalRecords = memory.journal?.records || []
  const knowledgeFiles = memory.knowledge_files || []
  const isEmptyMemory = journalRecords.length === 0 && knowledgeFiles.length === 0

  if (isEmptyMemory) {
    return (
      <div className="h-full flex flex-col items-center justify-center text-muted-foreground">
        <Brain className="h-12 w-12 mb-3 opacity-20" />
        <p className="text-sm">{t('agents:detail.noMemory')}</p>
        <p className="text-xs mt-1 opacity-60">{t('agents:memory.emptyHint')}</p>
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
    <ScrollArea className="h-full">
      <div className="space-y-4 pr-2">
        {/* Stats */}
        <div className={cn("gap-2", isMobile ? "grid grid-cols-1" : "grid grid-cols-3")}>
          {journalRecords.length > 0 && (
            <div className="flex flex-col items-center p-3 rounded-lg bg-card shadow-sm">
              <History className="h-4 w-4 text-muted-foreground mb-1" />
              <span className="text-lg font-semibold text-foreground">{journalRecords.length}</span>
              <span className={cn(textNano, "text-muted-foreground uppercase tracking-wide")}>{t('agents:memory.executions')}</span>
            </div>
          )}
          {knowledgeFiles.length > 0 && (
            <div className="flex flex-col items-center p-3 rounded-lg bg-card shadow-sm">
              <FileText className="h-4 w-4 text-muted-foreground mb-1" />
              <span className="text-lg font-semibold text-foreground">{knowledgeFiles.length}</span>
              <span className={cn(textNano, "text-muted-foreground uppercase tracking-wide")}>{t('agents:memory.knowledgeFiles')}</span>
            </div>
          )}
          {memory.updated_at && (
            <div className="flex flex-col items-center p-3 rounded-lg bg-card shadow-sm">
              <Clock className="h-4 w-4 text-muted-foreground mb-1" />
              <span className="text-sm font-medium text-foreground">
                {formatTime(memory.updated_at)}
              </span>
              <span className={cn(textNano, "text-muted-foreground uppercase tracking-wide")}>{t('agents:memory.lastUpdate')}</span>
            </div>
          )}
        </div>

        {/* Knowledge Files */}
        {knowledgeFiles.length > 0 && (
          <DetailSection
            title={`${t('agents:memory.knowledgeFiles')} (${knowledgeFiles.length})`}
            icon={FileText}
          >
            <div className="space-y-2">
              {knowledgeFiles.map((file: KnowledgeFileRef) => (
                <KnowledgeFileCard key={file.name} file={file} formatTime={formatTime} />
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
                <div key={idx} className="group rounded-lg border border-border hover:border-foreground/25 transition-colors">
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

        {/* Updated At footer */}
        {memory.updated_at && (
          <div className="flex items-center justify-center gap-2 text-xs text-muted-foreground py-3 border-t border-border">
            <Clock className="h-4 w-4" />
            <span>{t('agents:memory.updatedAt')}: {
              typeof memory.updated_at === 'number'
                ? new Date(memory.updated_at * 1000).toLocaleString()
                : new Date(memory.updated_at).toLocaleString()
            }</span>
          </div>
        )}
          </DetailSection>
        )}
      </div>
    </ScrollArea>
  )
}

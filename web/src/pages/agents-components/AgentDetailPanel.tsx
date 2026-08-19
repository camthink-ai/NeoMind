/**
 * Agent Detail Panel - Right side of Agents page
 *
 * Shows detailed view of a selected agent with tabs.
 */

import { useState, useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { LoadingState } from "@/components/shared/LoadingState"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { Badge } from "@/components/ui/badge"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Separator } from "@/components/ui/separator"
import {
  Bot,
  Clock,
  Activity,
  Brain,
  Eye,
  Zap,
  BarChart3,
  Loader2,
  CheckCircle2,
  XCircle,
  Settings,
  FileText,
  TrendingUp,
  Database,
  MessageSquare,
  History,
  Lightbulb,
  BookOpen,
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

type DetailTab = 'overview' | 'history' | 'memory' | 'messages'

// Role configuration - labels use i18n
const ROLE_CONFIG: Record<string, { icon: typeof Activity; color: string }> = {
  Monitor: { icon: Activity, color: 'text-info' },
  Executor: { icon: Zap, color: 'text-accent-orange' },
  Analyst: { icon: BarChart3, color: 'text-accent-purple' },
}

export function AgentDetailPanel({
  agent,
  onEdit,
  onExecute,
  onViewExecutionDetail,
  onRefresh,
  inlineMode = false,
}: AgentDetailPanelProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()
  const isMobile = useIsMobile()
  const [activeTab, setActiveTab] = useState<DetailTab>('overview')
  const [executions, setExecutions] = useState<AgentExecution[]>([])
  const [executionsLoading, setExecutionsLoading] = useState(false)
  const [memory, setMemory] = useState<AgentMemory | null>(null)
  const [memoryLoading, setMemoryLoading] = useState(false)
  const [availableResources, setAvailableResources] = useState<AgentAvailableResources | null>(null)

  // Real-time status from WebSocket events
  const [realtimeStatus, setRealtimeStatus] = useState<string | null>(null)

  // Load executions immediately when agent is selected (preload)
  // and when switching back to history tab with stale data
  useEffect(() => {
    if (agent?.id) {
      loadExecutions()
    }
  }, [agent?.id])

  // Load memory when memory tab is active
  useEffect(() => {
    if (agent && activeTab === 'memory') {
      loadMemory()
    }
  }, [agent, activeTab])

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
            if (activeTab === 'history') {
              loadExecutions()
            }
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

      {/* Tabs */}
      <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as DetailTab)} className="flex flex-col flex-1 min-h-0">
        <div className={cn("pt-3", isMobile ? "px-2" : "px-4")}>
          <TabsList className={cn(
            isMobile
              ? "grid grid-cols-4 gap-1 h-auto w-full p-0.5 rounded-lg border border-border bg-card"
              : "h-9 p-0.5 rounded-lg border border-border bg-card"
          )}>
            <TabsTrigger
              value="overview"
              className={cn(
                isMobile
                  ? "flex flex-col items-center justify-center gap-1 py-1.5 px-1 min-w-0 rounded-md text-mini leading-none data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none"
                  : "h-7 px-3 rounded-md text-sm data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none data-[state=active]:hover:bg-foreground hover:bg-muted"
              )}
            >
              <Eye className={cn(isMobile ? "h-4 w-4" : "h-4 w-4 mr-1")} />
              <span className={cn(isMobile && "truncate w-full text-center")}>{t('agents:detail.overview')}</span>
            </TabsTrigger>
            <TabsTrigger
              value="history"
              className={cn(
                isMobile
                  ? "flex flex-col items-center justify-center gap-1 py-1.5 px-1 min-w-0 rounded-md text-mini leading-none data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none"
                  : "h-7 px-3 rounded-md text-sm data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none data-[state=active]:hover:bg-foreground hover:bg-muted"
              )}
            >
              <Clock className={cn(isMobile ? "h-4 w-4" : "h-4 w-4 mr-1")} />
              <span className={cn(isMobile && "truncate w-full text-center")}>{t('agents:detail.history')}</span>
            </TabsTrigger>
            <TabsTrigger
              value="memory"
              className={cn(
                isMobile
                  ? "flex flex-col items-center justify-center gap-1 py-1.5 px-1 min-w-0 rounded-md text-mini leading-none data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none"
                  : "h-7 px-3 rounded-md text-sm data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none data-[state=active]:hover:bg-foreground hover:bg-muted"
              )}
            >
              <Brain className={cn(isMobile ? "h-4 w-4" : "h-4 w-4 mr-1")} />
              <span className={cn(isMobile && "truncate w-full text-center")}>{t('agents:detail.memory')}</span>
            </TabsTrigger>
            <TabsTrigger
              value="messages"
              className={cn(
                isMobile
                  ? "flex flex-col items-center justify-center gap-1 py-1.5 px-1 min-w-0 rounded-md text-mini leading-none data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none"
                  : "h-7 px-3 rounded-md text-sm data-[state=active]:bg-foreground data-[state=active]:text-background data-[state=active]:shadow-none data-[state=active]:hover:bg-foreground hover:bg-muted"
              )}
            >
              <MessageSquare className={cn(isMobile ? "h-4 w-4" : "h-4 w-4 mr-1")} />
              <span className={cn(isMobile && "truncate w-full text-center")}>{t('agents:detail.messages')}</span>
            </TabsTrigger>
          </TabsList>
        </div>

        {/* Tab Contents */}
        <div className="flex-1 min-h-0">
          {/* Overview Tab */}
          <TabsContent value="overview" className={cn("h-full m-0 pt-2", isMobile ? "p-2" : "p-4")}>
            <ScrollArea className="h-full">
              <div className="space-y-4 pr-2">
                {/* Stats Grid - Top section */}
                <DetailSection title="" icon={null}>
                  <div className={cn("gap-2", isMobile ? "grid grid-cols-1" : "grid grid-cols-4")}>
                    <StatItem
                      icon={<Activity className="h-4 w-4" />}
                      label={t('agents:detail.executions')}
                      value={formatCount(agent.stats?.total_executions ?? agent.execution_count)}
                      color="text-info"
                    />
                    <StatItem
                      icon={<CheckCircle2 className="h-4 w-4" />}
                      label={t('agents:detail.success')}
                      value={formatCount(agent.stats?.successful_executions ?? agent.success_count)}
                      color="text-success"
                    />
                    <StatItem
                      icon={<XCircle className="h-4 w-4" />}
                      label={t('agents:detail.failed')}
                      value={formatCount(agent.stats?.failed_executions ?? agent.error_count)}
                      color="text-error"
                    />
                    <StatItem
                      icon={<Clock className="h-4 w-4" />}
                      label={t('agents:detail.avgDuration')}
                      value={formatDuration(agent.stats?.avg_duration_ms ?? agent.avg_duration_ms)}
                      color="text-accent-orange"
                    />
                  </div>
                </DetailSection>

                {/* User Intent */}
                <DetailSection title={t('agents:userPrompt')} icon={FileText}>
                  <div className="text-sm leading-relaxed whitespace-pre-wrap text-foreground">
                    {agent.user_prompt || t('agents:card.noDescription')}
                  </div>
                  {agent.parsed_intent && (
                    <div className="mt-3 pt-3 border-t border-border">
                      <div className="text-xs text-muted-foreground mb-1.5">{t('agents:creator.basicInfo.requirement')}</div>
                      <div className="text-sm">
                        <span className="inline-flex items-center gap-1.5 px-2 py-1 rounded bg-info-light text-info">
                          {agent.parsed_intent.intent_type || '-'}
                        </span>
                      </div>
                    </div>
                  )}
                </DetailSection>

                {/* Schedule & Config - Two columns */}
                <div className={cn("gap-4", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                  {/* Schedule */}
                  <DetailSection title={t('agents:detail.schedule')} icon={Clock}>
                    <div className="space-y-1.5">
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

                  {/* LLM Config */}
                  {agent.llm_backend_id ? (
                    <DetailSection title={t('agents:creator.basicInfo.llmBackend')} icon={Brain}>
                      <InfoRow label={t('agents:creator.basicInfo.llmBackend')} value={agent.llm_backend_id} mono />
                    </DetailSection>
                  ) : (
                    <DetailSection title={t('common:info')} icon={Settings}>
                      <div className="space-y-1.5">
                        <InfoRow label={t('common:createdAt')} value={new Date(agent.created_at).toLocaleString()} />
                        <InfoRow label={t('common:updatedAt')} value={new Date(agent.updated_at).toLocaleString()} />
                        {agent.last_execution_at && (
                          <InfoRow label={t('agents:lastExecution')} value={new Date(agent.last_execution_at).toLocaleString()} />
                        )}
                      </div>
                    </DetailSection>
                  )}
                </div>

                {/* Resources - Full width */}
                <DetailSection title={`${t('agents:detail.resources')} (${(agent.resources || []).length})`} icon={Zap}>
                  <div className="space-y-3">
                    {/* Resource summary counts - group by actual types */}
                    <div className="flex flex-wrap gap-3">
                      {Object.entries(
                        (agent.resources || []).reduce((acc, r) => {
                          const type = r.resource_type.toLowerCase()
                          acc[type] = (acc[type] || 0) + 1
                          return acc
                        }, {} as Record<string, number>)
                      ).map(([type, count]) => (
                        <div key={type} className="flex items-center gap-2 px-3 py-1.5 rounded-md bg-muted text-foreground text-sm">
                          <span className="capitalize text-muted-foreground">{type}:</span>
                          <span className="font-semibold">{count}</span>
                        </div>
                      ))}
                    </div>
                    {/* Resource list */}
                    <div className={cn("gap-2", isMobile ? "grid grid-cols-1" : "grid grid-cols-2")}>
                      {(agent.resources || []).slice(0, 8).map((resource, idx) => (
                        <div key={idx} className="flex items-center justify-between px-2.5 py-1.5 rounded-md bg-muted">
                          <span className="text-sm truncate flex-1 mr-2" title={resource.resource_id}>
                            {resource.name || resource.resource_id}
                          </span>
                          <Badge variant="secondary" className="text-xs shrink-0">
                            {resource.resource_type}
                          </Badge>
                        </div>
                      ))}
                    </div>
                    {(agent.resources || []).length > 8 && (
                      <div className="text-xs text-muted-foreground text-center pt-1">
                        {t('agents:detail.moreResources', { count: (agent.resources || []).length - 8 })}
                      </div>
                    )}
                  </div>
                </DetailSection>

                {/* Timestamps - if LLM backend was shown above */}
                {agent.llm_backend_id && (
                  <DetailSection title={t('common:info')} icon={Settings}>
                    <div className={cn("gap-2", isMobile ? "grid grid-cols-1" : "grid grid-cols-3")}>
                      <InfoRow label={t('common:createdAt')} value={new Date(agent.created_at).toLocaleString()} />
                      <InfoRow label={t('common:updatedAt')} value={new Date(agent.updated_at).toLocaleString()} />
                      {agent.last_execution_at && (
                        <InfoRow label={t('agents:lastExecution')} value={new Date(agent.last_execution_at).toLocaleString()} />
                      )}
                    </div>
                  </DetailSection>
                )}
              </div>
            </ScrollArea>
          </TabsContent>

          {/* History Tab */}
          <TabsContent value="history" className="h-full m-0">
            <AgentExecutionTimeline
              executions={executions}
              loading={executionsLoading}
              agentId={agent.id}
              onViewExecutionDetail={onViewExecutionDetail}
            />
          </TabsContent>

          {/* Memory Tab */}
          <TabsContent value="memory" className={cn("h-full m-0 pt-2", isMobile ? "p-2" : "p-4")}>
            <MemoryContent memory={memory} loading={memoryLoading} />
          </TabsContent>

          {/* Messages Tab */}
          <TabsContent value="messages" className="h-full m-0">
            <AgentUserMessages
              agentId={agent.id}
              onMessageAdded={() => {
                // Refresh agent data to show updated message count
                onRefresh()
              }}
            />
          </TabsContent>
        </div>
      </Tabs>
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

// Compact Stat Item for stats grid
interface StatItemProps {
  icon: React.ReactNode
  label: string
  value: string | number
  color: string
}

function StatItem({ icon, label, value, color }: StatItemProps) {
  return (
    <div className="flex items-center gap-2 px-3 py-2.5 rounded-lg bg-card shadow-sm">
      <div className={cn("shrink-0", color)}>{icon}</div>
      <div className="flex-1 min-w-0">
        <div className="text-xs text-muted-foreground truncate">{label}</div>
        <div className="text-sm font-semibold truncate">{value}</div>
      </div>
    </div>
  )
}

// Info Row Component
interface InfoRowProps {
  label: string
  value: string | number
  mono?: boolean
}

function InfoRow({ label, value, mono }: InfoRowProps) {
  return (
    <div className="flex items-baseline gap-2 py-1 text-sm">
      <span className="text-muted-foreground text-xs shrink-0">{label}</span>
      <span className={cn("font-medium text-xs truncate", mono && "font-mono")}>{value}</span>
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
          </DetailSection>
        )}

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
      </div>
    </ScrollArea>
  )
}

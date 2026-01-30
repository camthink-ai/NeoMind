/**
 * Agent Monitor Widget
 *
 * A widget for monitoring AI agent execution history on the dashboard.
 * Features:
 * - Displays agent's real-time status
 * - Execution history with real-time updates
 * - Real-time thinking/progress display
 * - Click to view execution details
 * - Statistics display
 * - User messages support
 * - Memory view
 *
 * The agent to monitor is configured via the agentId prop (set in config panel).
 */

import { useState, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bot,
  CheckCircle2,
  XCircle,
  Loader2,
  Eye,
  Clock,
  AlertCircle,
  MoreHorizontal,
  ChevronRight,
  Brain,
  Sparkles,
  Send,
  MessageSquare,
  History,
  Database,
  Settings,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { useEvents } from '@/hooks/useEvents'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Tabs, TabsList, TabsTrigger, TabsContent } from '@/components/ui/tabs'
import { Separator } from '@/components/ui/separator'
import type { AiAgent, AgentExecution } from '@/types'
import type {
  AgentExecutionStartedEvent,
  AgentExecutionCompletedEvent,
  AgentThinkingEvent,
  AgentDecisionEvent,
  AgentProgressEvent,
} from '@/lib/events'

type WidgetTab = 'overview' | 'history' | 'memory' | 'messages'

interface AgentMonitorWidgetProps {
  className?: string
  agentId?: string // Configured agent ID from config panel
  editMode?: boolean
}

// Execution item for history
interface ExecutionItemProps {
  execution: AgentExecution
  isLatest?: boolean
  isRunning?: boolean
  onClick: () => void
}

function ExecutionItem({ execution, isLatest, isRunning, onClick }: ExecutionItemProps) {
  const getStatusIcon = () => {
    if (isRunning) {
      return <Loader2 className="h-3.5 w-3.5 text-blue-500 shrink-0 animate-spin" />
    }
    switch (execution.status) {
      case 'Completed':
        return <CheckCircle2 className="h-3.5 w-3.5 text-green-500 shrink-0" />
      case 'Failed':
      case 'Cancelled':
        return <XCircle className="h-3.5 w-3.5 text-red-500 shrink-0" />
      default:
        return <AlertCircle className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
    }
  }

  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`
    return `${(ms / 1000).toFixed(1)}s`
  }

  const formatTime = (timestamp: string | number) => {
    const date = typeof timestamp === 'number'
      ? new Date(timestamp * 1000)
      : new Date(timestamp)
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
  }

  return (
    <button
      onClick={onClick}
      className={cn(
        "w-full flex items-center gap-2 py-1.5 px-2 rounded transition-all text-left",
        isLatest ? "bg-primary/10 border border-primary/20" : "hover:bg-muted/50 border border-transparent"
      )}
    >
      {getStatusIcon()}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] font-mono text-muted-foreground">
            #{execution.id.slice(-6)}
          </span>
          {isLatest && (
            <Badge variant="outline" className="text-[8px] h-3.5 px-1">
              New
            </Badge>
          )}
        </div>
        {execution.error && (
          <p className="text-[10px] text-red-500 truncate mt-0.5">{execution.error}</p>
        )}
      </div>
      <div className="text-[10px] text-muted-foreground shrink-0">
        {formatTime(execution.timestamp)}
      </div>
      {execution.duration_ms > 0 && (
        <div className="text-[10px] font-medium tabular-nums shrink-0 min-w-[35px] text-right">
          {formatDuration(execution.duration_ms)}
        </div>
      )}
      <ChevronRight className="h-3 w-3 text-muted-foreground shrink-0" />
    </button>
  )
}

// Timestamp formatter
function formatTimestamp(timestamp: string | number): string {
  const date = typeof timestamp === 'number'
    ? new Date(timestamp * 1000)
    : new Date(timestamp)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(diff / 60000)
  const hours = Math.floor(diff / 3600000)
  const days = Math.floor(diff / 86400000)

  if (seconds < 60) return `${seconds}s ago`
  if (minutes < 60) return `${minutes}m ago`
  if (hours < 24) return `${hours}h ago`
  return `${days}d ago`
}

// Execution Detail Dialog
interface ExecutionDetailDialogProps {
  execution: AgentExecution | null
  open: boolean
  onClose: () => void
}

function ExecutionDetailDialog({ execution, open, onClose }: ExecutionDetailDialogProps) {
  const { t } = useTranslation('agents')
  const [detail, setDetail] = useState<any>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (open && execution?.id) {
      setLoading(true)
      api.getAgentExecution(execution.agent_id, execution.id)
        .then(setDetail)
        .catch(console.error)
        .finally(() => setLoading(false))
    }
  }, [open, execution])

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[80vh] z-[1000]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bot className="h-5 w-5" />
            Execution #{execution?.id.slice(-6)}
          </DialogTitle>
        </DialogHeader>
        <ScrollArea className="max-h-[60vh]">
          <div className="w-full p-4 space-y-4">
            {/* Basic Info */}
            <div className="grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="text-muted-foreground">Status</span>
                <div className="font-medium mt-1">
                  {execution?.status}
                </div>
              </div>
              <div>
                <span className="text-muted-foreground">Duration</span>
                <div className="font-medium mt-1">
                  {execution?.duration_ms ? `${execution.duration_ms}ms` : '-'}
                </div>
              </div>
              <div>
                <span className="text-muted-foreground">Timestamp</span>
                <div className="font-medium mt-1">
                  {execution?.timestamp ? new Date(execution.timestamp).toLocaleString() : '-'}
                </div>
              </div>
              <div>
                <span className="text-muted-foreground">Trigger</span>
                <div className="font-medium mt-1">
                  {execution?.trigger_type || '-'}
                </div>
              </div>
            </div>

            {/* Error */}
            {execution?.error && (
              <div className="p-3 bg-red-50 dark:bg-red-900/20 rounded-lg border border-red-200 dark:border-red-800">
                <p className="text-sm text-red-600 dark:text-red-400 font-medium">Error</p>
                <p className="text-sm text-red-600 dark:text-red-400 mt-1">{execution.error}</p>
              </div>
            )}

            {/* Decision Process */}
            {detail?.decision_process && (
              <div className="space-y-3">
                <h4 className="text-sm font-medium flex items-center gap-2">
                  <Brain className="h-4 w-4" />
                  Decision Process
                </h4>
                <div className="space-y-3 text-sm">
                  <div>
                    <span className="text-muted-foreground">Situation Analysis</span>
                    <p className="mt-1">{detail.decision_process.situation_analysis}</p>
                  </div>
                  {detail.decision_process.reasoning_steps?.length > 0 && (
                    <div>
                      <span className="text-muted-foreground">Reasoning Steps</span>
                      <div className="mt-2 space-y-2">
                        {detail.decision_process.reasoning_steps.map((step: any, i: number) => (
                          <div key={i} className="p-2 bg-muted/50 rounded">
                            <span className="text-muted-foreground">Step {i + 1}:</span>
                            <p className="mt-1">{step.description}</p>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                  <div>
                    <span className="text-muted-foreground">Conclusion</span>
                    <p className="mt-1">{detail.decision_process.conclusion}</p>
                  </div>
                </div>
              </div>
            )}

            {/* Loading state */}
            {loading && (
              <div className="flex items-center justify-center py-8">
                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
              </div>
            )}
          </div>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  )
}

export function AgentMonitorWidget({
  className,
  agentId,
  editMode = false,
}: AgentMonitorWidgetProps) {
  const { t } = useTranslation(['common', 'agents', 'dashboardComponents'])

  // State
  const [agent, setAgent] = useState<AiAgent | null>(null)
  const [loading, setLoading] = useState(true)
  const [executions, setExecutions] = useState<AgentExecution[]>([])
  const [isExecuting, setIsExecuting] = useState(false)
  const [newExecutionId, setNewExecutionId] = useState<string | null>(null)
  const [currentThinking, setCurrentThinking] = useState<string | null>(null)
  const [thinkingSteps, setThinkingSteps] = useState<Array<{ step: number; description: string }>>([])
  const [currentStage, setCurrentStage] = useState<string | null>(null) // 'collecting', 'analyzing', 'executing', null
  const [stageLabel, setStageLabel] = useState<string | null>(null)
  const [stageDetails, setStageDetails] = useState<string | null>(null)
  const [activeTab, setActiveTab] = useState<WidgetTab>('overview')

  // Messages state
  const [userMessages, setUserMessages] = useState<Array<{ id: string; content: string; timestamp: number }>>([])
  const [newMessage, setNewMessage] = useState('')
  const [sendingMessage, setSendingMessage] = useState(false)

  // Memory state
  const [memory, setMemory] = useState<any>(null)
  const [memoryLoading, setMemoryLoading] = useState(false)

  // Dialog state
  const [selectedExecution, setSelectedExecution] = useState<AgentExecution | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)

  // Track if we've loaded data
  const hasLoadedRef = useRef(false)

  // Fetch agent data
  const loadAgent = useCallback(async () => {
    if (!agentId) {
      setAgent(null)
      setLoading(false)
      return
    }

    try {
      const data = await api.getAgent(agentId)
      console.log('[AgentMonitorWidget] Agent data:', data)
      setAgent(data)
    } catch (error) {
      console.error('Failed to load agent:', error)
      setAgent(null)
    } finally {
      setLoading(false)
    }
  }, [agentId])

  // Fetch executions for the agent
  const loadExecutions = useCallback(async () => {
    if (!agentId) return
    try {
      const data = await api.getAgentExecutions(agentId, 50)
      console.log('[AgentMonitorWidget] Executions loaded:', data.executions?.length || 0)
      setExecutions(data.executions || [])
      hasLoadedRef.current = true
    } catch (error) {
      console.error('Failed to load executions:', error)
      setExecutions([])
    }
  }, [agentId])

  // Fetch user messages
  const loadUserMessages = useCallback(async () => {
    if (!agentId) return
    try {
      const data = await api.getAgentUserMessages(agentId)
      setUserMessages(data || [])
    } catch (error) {
      console.error('Failed to load user messages:', error)
      setUserMessages([])
    }
  }, [agentId])

  // Send user message
  const handleSendMessage = useCallback(async () => {
    if (!agentId || !newMessage.trim() || sendingMessage) return
    setSendingMessage(true)
    try {
      await api.addAgentUserMessage(agentId, newMessage.trim())
      setNewMessage('')
      // Reload messages
      loadUserMessages()
    } catch (error) {
      console.error('Failed to send message:', error)
    } finally {
      setSendingMessage(false)
    }
  }, [agentId, newMessage, sendingMessage, loadUserMessages])

  // Fetch memory
  const loadMemory = useCallback(async () => {
    if (!agentId) return
    setMemoryLoading(true)
    try {
      const data = await api.getAgentMemory(agentId)
      setMemory(data)
    } catch (error) {
      console.error('Failed to load memory:', error)
      setMemory(null)
    } finally {
      setMemoryLoading(false)
    }
  }, [agentId])

  // Load data when tab changes
  useEffect(() => {
    if (!agentId) return
    if (activeTab === 'messages') {
      loadUserMessages()
    } else if (activeTab === 'memory') {
      loadMemory()
    }
  }, [activeTab, agentId, loadUserMessages, loadMemory])

  // Initial load and refresh on agentId change
  useEffect(() => {
    console.log('[AgentMonitorWidget] Init with agentId:', agentId)
    hasLoadedRef.current = false
    setNewExecutionId(null)
    setCurrentStage(null)
    setStageLabel(null)
    setStageDetails(null)
    setCurrentThinking(null)
    setThinkingSteps([])
    loadAgent()
    loadExecutions()
  }, [loadAgent, loadExecutions])

  // WebSocket for real-time updates
  useEvents({
    enabled: !!agentId,
    eventTypes: [
      'AgentExecutionStarted',
      'AgentExecutionCompleted',
      'AgentThinking',
      'AgentDecision',
      'AgentProgress',
    ],
    onEvent: (event) => {
      console.log('[AgentMonitorWidget] Received event:', event.type, 'data:', event.data)
      switch (event.type) {
        case 'AgentExecutionStarted': {
          const startedData = (event as AgentExecutionStartedEvent).data
          console.log('[AgentMonitorWidget] AgentExecutionStarted - event.agent_id:', startedData.agent_id, 'widget.agentId:', agentId, 'match:', startedData.agent_id === agentId)
          if (startedData.agent_id === agentId) {
            console.log('[AgentMonitorWidget] Execution started:', startedData.execution_id)
            setIsExecuting(true)
            setNewExecutionId(startedData.execution_id || null)
            setCurrentStage('collecting')
            setStageLabel('Collecting data')
            setCurrentThinking('Starting execution...')
            setThinkingSteps([])
            loadExecutions()
          }
          break
        }

        case 'AgentProgress': {
          const progressData = (event as AgentProgressEvent).data
          if (progressData.agent_id === agentId) {
            console.log('[AgentMonitorWidget] Progress:', progressData)
            setCurrentStage(progressData.stage)
            setStageLabel(progressData.stage_label)
            setStageDetails(progressData.details || null)
            // Update current thinking to show stage progress
            if (progressData.details) {
              setCurrentThinking(`${progressData.stage_label}: ${progressData.details}`)
            } else {
              setCurrentThinking(progressData.stage_label)
            }
          }
          break
        }

        case 'AgentThinking': {
          const thinkingData = (event as AgentThinkingEvent).data
          console.log('[AgentMonitorWidget] AgentThinking - event.agent_id:', thinkingData.agent_id, 'widget.agentId:', agentId, 'match:', thinkingData.agent_id === agentId)
          if (thinkingData.agent_id === agentId) {
            console.log('[AgentMonitorWidget] Thinking:', thinkingData)
            setCurrentThinking(thinkingData.description)
            setThinkingSteps(prev => [
              ...prev.filter(s => s.step !== thinkingData.step_number),
              { step: thinkingData.step_number, description: thinkingData.description }
            ])
          }
          break
        }

        case 'AgentDecision': {
          const decisionData = (event as AgentDecisionEvent).data
          if (decisionData.agent_id === agentId) {
            console.log('[AgentMonitorWidget] Decision:', decisionData)
            setCurrentThinking(`Decided: ${decisionData.action}`)
          }
          break
        }

        case 'AgentExecutionCompleted': {
          const completedData = (event as AgentExecutionCompletedEvent).data
          if (completedData.agent_id === agentId) {
            console.log('[AgentMonitorWidget] Execution completed:', completedData.execution_id)
            setIsExecuting(false)
            setCurrentStage(null)
            setStageLabel(null)
            setStageDetails(null)
            setCurrentThinking(null)
            setThinkingSteps([])
            // Reload to get updated stats
            loadAgent()
            loadExecutions()
            // Clear new execution highlight after a delay
            setTimeout(() => setNewExecutionId(null), 3000)
          }
          break
        }
      }
    },
  })

  // Handle click on execution
  const handleExecutionClick = (execution: AgentExecution) => {
    setSelectedExecution(execution)
    setDetailOpen(true)
  }

  // Mock data for preview/edit mode when no agent is configured
  const mockAgent: AiAgent = {
    id: 'mock-agent-id',
    name: editMode ? t('dashboardComponents:agentMonitorWidget.selectAgent') : 'Sample Agent',
    status: 'Active',
    description: 'Sample agent for preview',
    created_at: String(Math.floor(Date.now() / 1000)),
    last_execution_at: String(Math.floor(Date.now() / 1000) - 300),
    execution_count: 12,
    success_count: 10,
    error_count: 2,
    avg_duration_ms: 1500,
  }

  const mockExecutions: AgentExecution[] = [
    {
      id: 'mock-exec-1',
      agent_id: agentId || 'mock-agent-id',
      status: 'Completed',
      trigger_type: 'manual',
      timestamp: String(Math.floor(Date.now() / 1000) - 60),
      duration_ms: 1250,
    },
    {
      id: 'mock-exec-2',
      agent_id: agentId || 'mock-agent-id',
      status: 'Completed',
      trigger_type: 'scheduled',
      timestamp: String(Math.floor(Date.now() / 1000) - 420),
      duration_ms: 1800,
    },
    {
      id: 'mock-exec-3',
      agent_id: agentId || 'mock-agent-id',
      status: 'Failed',
      trigger_type: 'manual',
      timestamp: String(Math.floor(Date.now() / 1000) - 900),
      duration_ms: 500,
      error: 'Connection timeout',
    },
  ]

  // In edit mode, always use mock data for preview
  // In normal mode, use real agent data
  const displayAgent = editMode ? mockAgent : agent
  const displayExecutions = editMode ? mockExecutions : executions

  // Calculate stats - from displayAgent or fallback to displayExecutions
  const statsFromExecutions = displayExecutions.length > 0 ? {
    total: displayExecutions.length,
    success: displayExecutions.filter(e => e.status === 'Completed').length,
    failed: displayExecutions.filter(e => e.status === 'Failed' || e.status === 'Cancelled').length,
    avgDuration: displayExecutions
      .filter(e => e.duration_ms && e.duration_ms > 0)
      .reduce((sum, e) => sum + (e.duration_ms || 0), 0) / displayExecutions.filter(e => e.duration_ms && e.duration_ms > 0).length || 0,
  } : null

  const executionCount = displayAgent?.execution_count || statsFromExecutions?.total || 0
  const successCount = displayAgent?.success_count || statsFromExecutions?.success || 0
  const errorCount = displayAgent?.error_count || statsFromExecutions?.failed || 0
  const avgDurationMs = displayAgent?.avg_duration_ms || statsFromExecutions?.avgDuration || 0

  const successRate = executionCount > 0 ? Math.round((successCount / executionCount) * 100) : 0
  const avgDuration = avgDurationMs < 1000 ? `${avgDurationMs}ms` : `${(avgDurationMs / 1000).toFixed(1)}s`

  const currentlyExecuting = isExecuting || displayAgent?.status === 'Executing'

  // Empty state - no agent configured (only show this outside edit mode)
  if (!agentId && !loading && !editMode) {
    return (
      <div className={cn("bg-card rounded-xl border shadow-sm overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center p-6">
          <div className="flex items-center justify-center mb-4">
            <Eye className="h-12 w-12 opacity-20 text-muted-foreground" />
          </div>
          <p className="text-sm text-muted-foreground">
            {t('dashboardComponents:agentMonitorWidget.noAgentConfigured')}
          </p>
        </div>
      </div>
    )
  }

  // Loading state - skip in edit mode to show preview
  if (loading && !editMode) {
    return (
      <div className={cn("bg-card rounded-xl border shadow-sm overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center">
          <Loader2 className="h-8 w-8 animate-spin text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground">{t('common:loading')}</p>
        </div>
      </div>
    )
  }

  // Agent not found - only show this outside edit mode
  if (!displayAgent && !editMode) {
    return (
      <div className={cn("bg-card rounded-xl border shadow-sm overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center">
          <Bot className="h-12 w-12 opacity-20 text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground">{t('dashboardComponents:agentMonitorWidget.agentNotFound')}</p>
        </div>
      </div>
    )
  }

  // Final fallback - should not happen since editMode always has mockAgent
  if (!displayAgent) {
    return (
      <div className={cn("bg-card rounded-xl border shadow-sm overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center">
          <Bot className="h-12 w-12 opacity-20 text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground">{t('dashboardComponents:agentMonitorWidget.agentNotFound')}</p>
        </div>
      </div>
    )
  }

  return (
    <>
      <div className={cn("bg-card rounded-lg border-0 shadow-sm overflow-hidden flex flex-col w-full h-full bg-background", className)}>
        {/* Compact Header - Agent Info */}
        <div className="px-3 py-2 border-b border-border/50 bg-muted/30 shrink-0">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-2">
              <div className={cn(
                "h-7 w-7 rounded-md flex items-center justify-center transition-colors",
                currentlyExecuting ? "bg-blue-500/20" : "bg-green-500/20"
              )}>
                <Bot className={cn(
                  "h-3.5 w-3.5 transition-colors",
                  currentlyExecuting ? "text-blue-500" : "text-green-500"
                )} />
              </div>
              <div>
                <h3 className="font-medium text-xs">{displayAgent.name}</h3>
                <div className="flex items-center gap-1.5 mt-0.5">
                  {currentlyExecuting ? (
                    <Badge variant="default" className="text-[9px] h-4 gap-0.5 px-1">
                      <Loader2 className="h-2 w-2 animate-spin" />
                      Running
                    </Badge>
                  ) : displayAgent.status === 'Active' ? (
                    <Badge variant="outline" className="text-[9px] h-4 text-green-600 border-green-200 px-1">
                      <CheckCircle2 className="h-2 w-2 mr-0.5" />
                      Active
                    </Badge>
                  ) : displayAgent.status === 'Error' ? (
                    <Badge variant="destructive" className="text-[9px] h-4 px-1">
                      Error
                    </Badge>
                  ) : (
                    <Badge variant="secondary" className="text-[9px] h-4 px-1">
                      Paused
                    </Badge>
                  )}
                  <span className="text-[9px] text-muted-foreground">
                    {displayAgent.last_execution_at
                      ? formatTimestamp(displayAgent.last_execution_at)
                      : 'Never'
                    }
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* Compact Tabs List */}
        <div className="px-2 pt-1.5 pb-0 shrink-0">
          <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as WidgetTab)} className="w-full">
            <TabsList className="w-full justify-start bg-muted/30 h-7 px-0">
              <TabsTrigger value="overview" className="h-6 px-2 text-[10px] data-[state=active]:bg-background">
                <Sparkles className="h-2.5 w-2.5 mr-1" />
                Overview
              </TabsTrigger>
              <TabsTrigger value="history" className="h-6 px-2 text-[10px] data-[state=active]:bg-background">
                <History className="h-2.5 w-2.5 mr-1" />
                History
              </TabsTrigger>
              <TabsTrigger value="memory" className="h-6 px-2 text-[10px] data-[state=active]:bg-background">
                <Brain className="h-2.5 w-2.5 mr-1" />
                Memory
              </TabsTrigger>
              <TabsTrigger value="messages" className="h-6 px-2 text-[10px] data-[state=active]:bg-background">
                <MessageSquare className="h-2.5 w-2.5 mr-1" />
                Messages
                {userMessages.length > 0 && (
                  <span className="ml-1 h-3.5 w-3.5 rounded-full bg-primary text-primary-foreground text-[8px] flex items-center justify-center">
                    {userMessages.length}
                  </span>
                )}
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>

        {/* Real-time Progress - full-width, minimal style */}
        {currentlyExecuting && (
          <div className="w-full bg-primary/5 border-b border-primary/10 relative overflow-hidden shrink-0">
            {/* Progress bar - full width */}
            <div className="w-full h-1 bg-muted/30 relative overflow-hidden">
              <div className={cn(
                "absolute inset-y-0 left-0 transition-all duration-500 ease-out",
                "bg-gradient-to-r from-blue-500 via-purple-500 to-green-500",
                "animate-[shimmer_2s_infinite]"
              )} style={{
                width: currentStage === 'collecting' ? '33%' : currentStage === 'analyzing' ? '66%' : '90%'
              }} />
            </div>

            {/* Stage info - compact inline */}
            <div className="flex items-center justify-between px-2 py-1.5">
              <div className="flex items-center gap-1.5">
                <div className={cn(
                  "w-1.5 h-1.5 rounded-full animate-pulse",
                  currentStage === 'collecting' ? "bg-blue-500" :
                  currentStage === 'analyzing' ? "bg-purple-500" : "bg-green-500"
                )} />
                <span className="text-[9px] font-medium text-foreground/80">
                  {stageLabel || 'Processing'}
                </span>
              </div>

              {/* Current thinking step - single line, truncates */}
              {thinkingSteps.length > 0 && (
                <span className="text-[8px] text-muted-foreground truncate max-w-[60%] font-mono">
                  {thinkingSteps[thinkingSteps.length - 1]?.description}
                </span>
              )}
            </div>
          </div>
        )}

        {/* Tab Content - minimal padding for full container fill */}
        <div className="w-full flex-1 min-h-0 overflow-hidden px-2">
          <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as WidgetTab)} className="w-full h-full flex flex-col">
            {/* Overview Tab Content */}
            <TabsContent value="overview" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full gap-2 overflow-hidden">
                {/* Compact Stats Bar */}
                <div className="w-full flex items-center divide-x divide-border/50 border border-border/50 rounded bg-muted/20 shrink-0">
                  <div className="flex-1 px-2 py-1.5 text-center">
                    <div className="text-sm font-semibold tabular-nums">{executionCount}</div>
                    <div className="text-[8px] text-muted-foreground uppercase tracking-wide">Runs</div>
                  </div>
                  <div className="flex-1 px-2 py-1.5 text-center">
                    <div className={cn(
                      "text-sm font-semibold tabular-nums",
                      successRate >= 80 ? "text-green-600" : successRate >= 50 ? "text-yellow-600" : "text-red-600"
                    )}>{successRate}%</div>
                    <div className="text-[8px] text-muted-foreground uppercase tracking-wide">Success</div>
                  </div>
                  <div className="flex-1 px-2 py-1.5 text-center">
                    <div className="text-sm font-semibold tabular-nums text-red-500">{errorCount}</div>
                    <div className="text-[8px] text-muted-foreground uppercase tracking-wide">Failed</div>
                  </div>
                  <div className="flex-1 px-2 py-1.5 text-center">
                    <div className="text-sm font-semibold tabular-nums">{avgDuration}</div>
                    <div className="text-[8px] text-muted-foreground uppercase tracking-wide">Avg</div>
                  </div>
                </div>

                {/* Recent Executions */}
                <div className="w-full flex-1 min-h-0 flex flex-col">
                  <div className="w-full px-2 py-1 border-b border-border/30 flex items-center justify-between bg-muted/10 rounded-t shrink-0">
                    <span className="text-[10px] font-medium">Recent</span>
                    <span className="text-[8px] text-muted-foreground">
                      {displayExecutions.slice(0, 5).length}
                    </span>
                  </div>

                  <ScrollArea className="flex-1 w-full border border-border/30 rounded-b">
                    {displayExecutions.length === 0 ? (
                      <div className="flex flex-col items-center justify-center py-8 text-center">
                        <MoreHorizontal className="h-6 w-6 text-muted-foreground opacity-50 mb-1" />
                        <p className="text-[10px] text-muted-foreground">No history</p>
                      </div>
                    ) : (
                      <div className="w-full p-1 space-y-0.5">
                        {displayExecutions.slice(0, 5).map((exec, index) => (
                          <ExecutionItem
                            key={exec.id}
                            execution={exec}
                            isLatest={index === 0 && exec.id === newExecutionId}
                            isRunning={exec.status === 'Running' && (index === 0 || exec.id === newExecutionId)}
                            onClick={() => handleExecutionClick(exec)}
                          />
                        ))}
                      </div>
                    )}
                  </ScrollArea>
                </div>
              </div>
            </TabsContent>

            {/* History Tab Content */}
            <TabsContent value="history" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full overflow-hidden">
                <div className="px-2 py-1 border-b border-border/30 flex items-center justify-between bg-muted/10 rounded-t shrink-0">
                  <span className="text-[10px] font-medium">History</span>
                  <span className="text-[8px] text-muted-foreground">
                    {displayExecutions.length}
                  </span>
                </div>

                <ScrollArea className="flex-1 w-full border border-border/30 rounded-b">
                  {displayExecutions.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-8 text-center">
                      <MoreHorizontal className="h-6 w-6 text-muted-foreground opacity-50 mb-1" />
                      <p className="text-[10px] text-muted-foreground">No history</p>
                    </div>
                  ) : (
                    <div className="w-full p-1 space-y-0.5">
                      {displayExecutions.map((exec, index) => (
                        <ExecutionItem
                          key={exec.id}
                          execution={exec}
                          isLatest={index === 0 && exec.id === newExecutionId}
                          isRunning={exec.status === 'Running' && (index === 0 || exec.id === newExecutionId)}
                          onClick={() => handleExecutionClick(exec)}
                        />
                      ))}
                    </div>
                  )}
                </ScrollArea>
              </div>
            </TabsContent>

            {/* Memory Tab Content */}
            <TabsContent value="memory" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full overflow-hidden">
                <div className="px-2 py-1 border-b border-border/30 flex items-center bg-muted/10 rounded-t shrink-0">
                  <span className="text-[10px] font-medium">Memory</span>
                </div>

                <ScrollArea className="flex-1 w-full border border-border/30 rounded-b">
                  {memoryLoading ? (
                    <div className="flex items-center justify-center py-8">
                      <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                    </div>
                  ) : memory ? (
                    <div className="w-full p-2 space-y-3">
                      {/* State Variables */}
                      {memory.state_variables && Object.keys(memory.state_variables).length > 0 && (
                        <div>
                          <h4 className="text-[10px] font-semibold text-muted-foreground mb-1.5 flex items-center gap-1">
                            <Database className="h-2.5 w-2.5" />
                            State Variables
                          </h4>
                          <div className="space-y-1.5">
                            {Object.entries(memory.state_variables).map(([key, value], idx) => (
                              <div key={idx} className="text-[10px] p-1.5 rounded bg-muted/30">
                                <div className="flex items-center justify-between mb-0.5">
                                  <span className="text-[9px] font-mono text-blue-600">{key}</span>
                                </div>
                                <p className="text-[10px] break-all font-mono">
                                  {typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value)}
                                </p>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* Learned Patterns */}
                      {memory.learned_patterns && memory.learned_patterns.length > 0 && (
                        <div>
                          <h4 className="text-[10px] font-semibold text-muted-foreground mb-1.5 flex items-center gap-1">
                            <Sparkles className="h-2.5 w-2.5" />
                            Patterns
                          </h4>
                          <div className="space-y-1">
                            {memory.learned_patterns.map((pattern: string, idx: number) => (
                              <div key={idx} className="text-[10px] p-1.5 rounded bg-purple-500/10 border border-purple-500/20">
                                <p className="text-[10px]">{pattern}</p>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* Trend Data */}
                      {memory.trend_data && memory.trend_data.length > 0 && (
                        <div>
                          <h4 className="text-[10px] font-semibold text-muted-foreground mb-1.5 flex items-center gap-1">
                            <Clock className="h-2.5 w-2.5" />
                            Trends
                          </h4>
                          <div className="space-y-1">
                            {memory.trend_data.slice(-5).map((point: any, idx: number) => (
                              <div key={idx} className="text-[10px] p-1.5 rounded bg-muted/30">
                                <div className="flex items-center justify-between mb-0.5">
                                  <span className="text-[9px] text-muted-foreground">
                                    {new Date(point.timestamp * 1000).toLocaleString()}
                                  </span>
                                  <span className="text-[9px] text-green-600">{point.metric}</span>
                                </div>
                                <p className="text-[10px] font-mono">{point.value}</p>
                              </div>
                            ))}
                          </div>
                        </div>
                      )}

                      {/* No data */}
                      {(!memory.state_variables || Object.keys(memory.state_variables).length === 0) &&
                       (!memory.learned_patterns || memory.learned_patterns.length === 0) &&
                       (!memory.trend_data || memory.trend_data.length === 0) && (
                        <div className="flex flex-col items-center justify-center py-8 text-center">
                          <Brain className="h-6 w-6 text-muted-foreground opacity-50 mb-1" />
                          <p className="text-[10px] text-muted-foreground">No memory data</p>
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="flex flex-col items-center justify-center py-8 text-center">
                      <Brain className="h-6 w-6 text-muted-foreground opacity-50 mb-1" />
                      <p className="text-[10px] text-muted-foreground">No memory data</p>
                    </div>
                  )}
                </ScrollArea>
              </div>
            </TabsContent>

            {/* Messages Tab Content */}
            <TabsContent value="messages" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full gap-1.5 overflow-hidden">
                {/* Header */}
                <div className="w-full px-2 py-1 border flex items-center bg-muted/10 rounded shrink-0">
                  <span className="text-[10px] font-medium">Messages</span>
                  {userMessages.length > 0 && (
                    <span className="text-[8px] text-muted-foreground ml-auto">
                      {userMessages.length}
                    </span>
                  )}
                </div>

                {/* Messages list - scrollable */}
                <ScrollArea className="flex-1 w-full border border-border/30 rounded">
                  {userMessages.length === 0 ? (
                    <div className="flex flex-col items-center justify-center py-8 text-center">
                      <MessageSquare className="h-6 w-6 text-muted-foreground opacity-50 mb-1" />
                      <p className="text-[10px] text-muted-foreground">No messages</p>
                    </div>
                  ) : (
                    <div className="w-full p-1.5 space-y-1">
                      {userMessages.map((msg) => (
                        <div key={msg.id} className="text-[10px] p-1.5 rounded bg-muted/30">
                          <div className="flex items-center justify-between mb-0.5">
                            <span className="text-[8px] text-muted-foreground">
                              {new Date(msg.timestamp * 1000).toLocaleString()}
                            </span>
                          </div>
                          <p className="text-[10px]">{msg.content}</p>
                        </div>
                      ))}
                    </div>
                  )}
                </ScrollArea>

                {/* Send message input - compact */}
                <div className="w-full border border-border/30 rounded p-1.5 shrink-0">
                  <div className="flex gap-1.5">
                    <Textarea
                      placeholder="Send message..."
                      value={newMessage}
                      onChange={(e) => setNewMessage(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault()
                          handleSendMessage()
                        }
                      }}
                      className="min-h-[30px] h-8 text-[10px] resize-none"
                      disabled={sendingMessage}
                    />
                    <Button
                      size="sm"
                      onClick={handleSendMessage}
                      disabled={!newMessage.trim() || sendingMessage}
                      className="h-8 px-2 shrink-0"
                    >
                      {sendingMessage ? (
                        <Loader2 className="h-3 w-3 animate-spin" />
                      ) : (
                        <Send className="h-3 w-3" />
                      )}
                    </Button>
                  </div>
                </div>
              </div>
            </TabsContent>
          </Tabs>
        </div>
      </div>

      {/* Execution Detail Dialog */}
      <ExecutionDetailDialog
        execution={selectedExecution}
        open={detailOpen}
        onClose={() => setDetailOpen(false)}
      />
    </>
  )
}

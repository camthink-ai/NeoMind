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
  Image as ImageIcon,
  Monitor,
  ChevronDown,
  ChevronUp,
  TrendingUp,
  Zap,
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
      type="button"
      onClick={(e) => {
        e.stopPropagation()
        e.preventDefault()
        onClick()
      }}
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

// Normalize decision_process: API may return it as JSON string or with situation_analysis as JSON string
function normalizeDecisionProcess(raw: unknown): {
  situation_analysis: string
  reasoning_steps: Array<{ description: string; step_number?: number; output?: string }>
  conclusion: string
} | null {
  if (raw == null) return null
  let dp = raw
  if (typeof dp === 'string') {
    try {
      dp = JSON.parse(dp) as Record<string, unknown>
    } catch {
      return null
    }
  }
  if (typeof dp !== 'object' || dp === null) return null
  const obj = dp as Record<string, unknown>
  let situation_analysis = (obj.situation_analysis as string) ?? ''
  let conclusion = (obj.conclusion as string) ?? ''
  let reasoning_steps = Array.isArray(obj.reasoning_steps) ? obj.reasoning_steps : []

  // If situation_analysis looks like JSON (backend sent whole object as one field), parse and extract
  if (typeof situation_analysis === 'string' && situation_analysis.trim().startsWith('{')) {
    try {
      const parsed = JSON.parse(situation_analysis) as Record<string, unknown>
      situation_analysis = (parsed.situation_analysis as string) ?? situation_analysis
      conclusion = (parsed.conclusion as string) ?? conclusion
      if (Array.isArray(parsed.reasoning_steps)) reasoning_steps = parsed.reasoning_steps
    } catch {
      // keep as-is
    }
  }

  const steps = reasoning_steps.map((s: unknown, i: number) => {
    if (s && typeof s === 'object' && 'description' in s) {
      return { description: (s as Record<string, unknown>).description as string, step_number: i + 1 }
    }
    if (s && typeof s === 'object' && 'output' in s) {
      return { description: (s as Record<string, unknown>).output as string, step_number: i + 1 }
    }
    return { description: String(s), step_number: i + 1 }
  })

  return {
    situation_analysis: situation_analysis || '',
    reasoning_steps: steps,
    conclusion: conclusion || '',
  }
}

// ============================================================================
// Memory Content - Structured and readable display (synced with AgentDetailPanel)
// ============================================================================

interface MemoryContentProps {
  memory: any
  loading: boolean
}

function MemoryContent({ memory, loading }: MemoryContentProps) {
  const { t } = useTranslation(['common', 'agents'])

  if (loading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (!memory) {
    return (
      <div className="flex flex-col items-center justify-center py-8 text-center">
        <Brain className="h-8 w-8 text-muted-foreground opacity-50 mb-2" />
        <p className="text-xs text-muted-foreground">{t('agents:detail.noMemory')}</p>
      </div>
    )
  }

  // Count memory items
  const stateVarCount = Object.keys(memory.state_variables || {}).length
  const learnedPatternsCount = memory.learned_patterns?.length || 0
  const longTermPatternsCount = memory.long_term?.patterns?.length || 0
  const shortTermSummariesCount = memory.short_term?.summaries?.length || 0
  const longTermMemoriesCount = memory.long_term?.memories?.length || 0
  const hasWorkingMemory = memory.working && (memory.working.current_analysis || memory.working.current_conclusion)

  // Check if memory is empty
  const isEmptyMemory = stateVarCount === 0 && learnedPatternsCount === 0 &&
    shortTermSummariesCount === 0 && longTermMemoriesCount === 0 &&
    longTermPatternsCount === 0 && !hasWorkingMemory

  if (isEmptyMemory) {
    return (
      <div className="flex flex-col items-center justify-center py-8 text-center">
        <Brain className="h-8 w-8 text-muted-foreground opacity-50 mb-2" />
        <p className="text-xs text-muted-foreground">{t('agents:detail.noMemory')}</p>
      </div>
    )
  }

  // Format timestamp
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

  // Memory Stats Summary
  const showStatsSummary = shortTermSummariesCount > 0 || longTermMemoriesCount > 0 || longTermPatternsCount > 0

  return (
    <div className="space-y-3">
      {/* Memory Stats Summary */}
      {showStatsSummary && (
        <div className="grid grid-cols-3 gap-2">
          {shortTermSummariesCount > 0 && (
            <div className="flex flex-col items-center p-2 rounded-lg bg-blue-500/10 border border-blue-500/20">
              <History className="h-3 w-3 text-blue-500 mb-1" />
              <span className="text-sm font-bold text-blue-600 dark:text-blue-400">{shortTermSummariesCount}</span>
              <span className="text-[8px] text-muted-foreground uppercase tracking-wide">{t('agents:memory.shortTerm')}</span>
            </div>
          )}
          {longTermMemoriesCount > 0 && (
            <div className="flex flex-col items-center p-2 rounded-lg bg-purple-500/10 border border-purple-500/20">
              <Sparkles className="h-3 w-3 text-purple-500 mb-1" />
              <span className="text-sm font-bold text-purple-600 dark:text-purple-400">{longTermMemoriesCount}</span>
              <span className="text-[8px] text-muted-foreground uppercase tracking-wide">{t('agents:memory.longTerm')}</span>
            </div>
          )}
          {longTermPatternsCount > 0 && (
            <div className="flex flex-col items-center p-2 rounded-lg bg-amber-500/10 border border-amber-500/20">
              <TrendingUp className="h-3 w-3 text-amber-500 mb-1" />
              <span className="text-sm font-bold text-amber-600 dark:text-amber-400">{longTermPatternsCount}</span>
              <span className="text-[8px] text-muted-foreground uppercase tracking-wide">{t('agents:detail.learnedPatterns')}</span>
            </div>
          )}
        </div>
      )}

      {/* Working Memory - Current Analysis */}
      {hasWorkingMemory && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center gap-2 mb-2 text-xs font-medium text-muted-foreground">
            <Zap className="h-3.5 w-3.5" />
            {t('agents:memory.working')}
          </div>
          <div className="p-3 rounded-lg bg-gradient-to-br from-blue-500/5 to-purple-500/5 border border-blue-500/10">
            {memory.working.current_analysis && (
              <div className="mb-2">
                <div className="text-[9px] text-muted-foreground uppercase tracking-wide mb-1">{t('agents:memory.situationAnalysis')}</div>
                <p className="text-xs leading-relaxed">{memory.working.current_analysis}</p>
              </div>
            )}
            {memory.working.current_conclusion && (
              <div className="flex items-start gap-2 pt-2 border-t border-border/50">
                <CheckCircle2 className="h-3 w-3 text-green-500 mt-0.5 shrink-0" />
                <div>
                  <div className="text-[9px] text-muted-foreground uppercase tracking-wide mb-0.5">{t('agents:memory.conclusion')}</div>
                  <p className="text-xs font-medium">{memory.working.current_conclusion}</p>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Short-Term Memory - Recent Executions */}
      {shortTermSummariesCount > 0 && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
              <History className="h-3.5 w-3.5" />
              {t('agents:memory.shortTerm')} ({shortTermSummariesCount}/{memory.short_term?.max_summaries || 10})
            </div>
          </div>
          <div className="space-y-2">
            {memory.short_term?.summaries?.map((summary: any, idx: number) => (
              <div key={idx} className="group relative overflow-hidden rounded-lg bg-background border border-border/50 hover:border-blue-500/30 transition-colors">
                {/* Success indicator strip */}
                <div className={`absolute left-0 top-0 bottom-0 w-1 ${summary.success ? 'bg-green-500' : 'bg-red-500'}`} />

                <div className="pl-4 pr-3 py-3">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-2">
                      <span className="text-[10px] font-mono text-muted-foreground bg-muted/50 px-1.5 py-0.5 rounded">
                        {summary.execution_id?.slice(0, 6)}...
                      </span>
                      <span className="text-[10px] text-muted-foreground">
                        {formatTime(summary.timestamp)}
                      </span>
                    </div>
                    <Badge
                      variant={summary.success ? 'default' : 'destructive'}
                      className="text-[9px] h-4"
                    >
                      {summary.success ? t('agents:executionStatus.completed') : t('agents:executionStatus.failed')}
                    </Badge>
                  </div>

                  {summary.conclusion && (
                    <div className="mb-2">
                      <div className="text-[9px] text-muted-foreground mb-0.5">{t('agents:memory.conclusion')}</div>
                      <p className="text-[10px]">{summary.conclusion}</p>
                    </div>
                  )}

                  {summary.situation && (
                    <div className="mb-2">
                      <div className="text-[9px] text-muted-foreground mb-0.5">{t('agents:memory.situationAnalysis')}</div>
                      <p className="text-[10px] text-muted-foreground line-clamp-2">{summary.situation}</p>
                    </div>
                  )}

                  {summary.decisions && summary.decisions.length > 0 && (
                    <div className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                      <Zap className="h-2.5 w-2.5" />
                      <span>{summary.decisions.length} {t('agents:memory.decisions')}</span>
                    </div>
                  )}
                </div>
              </div>
            ))}
          </div>

          {/* Archive info */}
          {memory.short_term?.last_archived_at && (
            <div className="mt-2 text-[9px] text-center text-muted-foreground">
              {t('agents:memory.lastArchived')}: {formatTime(memory.short_term.last_archived_at)}
            </div>
          )}
        </div>
      )}

      {/* Long-Term Memory - Important Memories */}
      {longTermMemoriesCount > 0 && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center gap-2 mb-3 text-xs font-medium text-muted-foreground">
            <Sparkles className="h-3.5 w-3.5" />
            {t('agents:memory.longTerm')} ({longTermMemoriesCount}/{memory.long_term?.max_memories || 50})
          </div>
          <div className="space-y-2">
            {memory.long_term?.memories?.map((mem: any, idx: number) => (
              <div key={idx} className="p-3 rounded-lg bg-gradient-to-br from-purple-500/5 to-pink-500/5 border border-purple-500/10 hover:border-purple-500/20 transition-colors">
                <div className="flex items-center justify-between mb-2">
                  <Badge variant="outline" className="text-[9px] h-4">
                    {mem.memory_type}
                  </Badge>
                  <div className="flex items-center gap-1">
                    <div className="w-12 h-1 bg-muted rounded-full overflow-hidden">
                      <div
                        className="h-full bg-purple-500 rounded-full"
                        style={{ width: `${Math.round((mem.importance || 0) * 100)}%` }}
                      />
                    </div>
                    <span className="text-[9px] text-muted-foreground">{Math.round((mem.importance || 0) * 100)}%</span>
                  </div>
                </div>
                <p className="text-[10px] line-clamp-3">{mem.content}</p>
                {mem.metadata?.execution_id && (
                  <div className="mt-2 text-[9px] text-muted-foreground font-mono">
                    {mem.metadata.execution_id.slice(0, 8)}...
                  </div>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Learned Patterns (from long_term) */}
      {longTermPatternsCount > 0 && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center gap-2 mb-3 text-xs font-medium text-muted-foreground">
            <TrendingUp className="h-3.5 w-3.5" />
            {t('agents:detail.learnedPatterns')} ({longTermPatternsCount})
          </div>
          <div className="space-y-2">
            {memory.long_term?.patterns?.map((pattern: any, idx: number) => (
              <div key={idx} className="p-3 rounded-lg bg-gradient-to-br from-amber-500/5 to-orange-500/5 border border-amber-500/10">
                <div className="flex items-center justify-between mb-2">
                  <Badge variant="outline" className="text-[9px] h-4">
                    {pattern.pattern_type}
                  </Badge>
                  <div className="flex items-center gap-2">
                    <span className="text-[9px] text-muted-foreground">{formatTime(pattern.learned_at)}</span>
                    <span className="text-[9px] font-medium text-amber-600 dark:text-amber-400">
                      {Math.round((pattern.confidence || 0) * 100)}%
                    </span>
                  </div>
                </div>
                <p className="text-[10px]">{pattern.description}</p>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Legacy Learned Patterns */}
      {learnedPatternsCount > 0 && longTermPatternsCount === 0 && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center gap-2 mb-3 text-xs font-medium text-muted-foreground">
            <TrendingUp className="h-3.5 w-3.5" />
            {t('agents:detail.learnedPatterns')} ({learnedPatternsCount})
          </div>
          <div className="space-y-2">
            {memory.learned_patterns.map((pattern: any, idx: number) => (
              <div key={idx} className="p-3 rounded-lg bg-gradient-to-br from-amber-500/5 to-orange-500/5 border border-amber-500/10">
                <div className="flex items-center justify-between mb-2">
                  <Badge variant="outline" className="text-[9px] h-4">
                    {pattern.pattern_type}
                  </Badge>
                  <span className="text-[9px] font-medium text-amber-600 dark:text-amber-400">
                    {Math.round((pattern.confidence || 0) * 100)}%
                  </span>
                </div>
                <p className="text-[10px]">{pattern.description}</p>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* State Variables */}
      {stateVarCount > 0 && (
        <div className="bg-muted/20 rounded-lg p-3">
          <div className="flex items-center gap-2 mb-3 text-xs font-medium text-muted-foreground">
            <Database className="h-3.5 w-3.5" />
            {t('agents:detail.stateVariables')}
          </div>
          <div className="grid grid-cols-2 gap-2">
            {Object.entries(memory.state_variables || {}).map(([key, value]) => (
              <div key={key} className="flex items-center justify-between px-3 py-2 rounded-lg bg-background border">
                <span className="text-[10px] font-medium truncate flex-1 mr-2" title={key}>{key}</span>
                <span className="text-[10px] font-mono text-muted-foreground truncate max-w-[80px]" title={String(value)}>
                  {typeof value === 'object' ? JSON.stringify(value) : String(value)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Updated At footer */}
      {memory.updated_at && (
        <div className="flex items-center justify-center gap-2 text-[9px] text-muted-foreground py-2 border-t border-border/50">
          <Clock className="h-2.5 w-2.5" />
          <span>{t('agents:memory.updatedAt')}: {
            typeof memory.updated_at === 'number'
              ? new Date(memory.updated_at * 1000).toLocaleString()
              : new Date(memory.updated_at).toLocaleString()
          }</span>
        </div>
      )}
    </div>
  )
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
  const [expandedDataIndices, setExpandedDataIndices] = useState<Set<number>>(new Set())
  const dialogRef = useRef<HTMLDivElement>(null)

  // Prevent event propagation to grid when interacting with dialog
  useEffect(() => {
    if (!open || !dialogRef.current) return

    const dialogElement = dialogRef.current

    // Stop propagation of pointer events to prevent grid dragging
    const stopPropagation = (e: Event) => {
      e.stopPropagation()
      e.stopImmediatePropagation()
    }

    dialogElement.addEventListener('mousedown', stopPropagation, { capture: true })
    dialogElement.addEventListener('touchstart', stopPropagation, { capture: true })
    dialogElement.addEventListener('pointerdown', stopPropagation, { capture: true })

    return () => {
      dialogElement.removeEventListener('mousedown', stopPropagation, { capture: true } as any)
      dialogElement.removeEventListener('touchstart', stopPropagation, { capture: true } as any)
      dialogElement.removeEventListener('pointerdown', stopPropagation, { capture: true } as any)
    }
  }, [open])

  useEffect(() => {
    if (open && execution?.id) {
      setLoading(true)
      api.getAgentExecution(execution.agent_id, execution.id)
        .then(setDetail)
        .catch(console.error)
        .finally(() => setLoading(false))
    }
  }, [open, execution])

  const decisionProcess = detail?.decision_process != null ? normalizeDecisionProcess(detail.decision_process) : null
  const dataCollected = detail?.decision_process?.data_collected || []

  const toggleDataExpanded = (index: number) => {
    setExpandedDataIndices(prev => {
      const next = new Set(prev)
      if (next.has(index)) {
        next.delete(index)
      } else {
        next.add(index)
      }
      return next
    })
  }

  // Helper functions for image handling
  const isPureBase64 = (str: string): boolean => {
    if (!str || str.length < 100) return false
    const cleaned = str.trim()
    if (cleaned.startsWith('http://') || cleaned.startsWith('https://') || cleaned.startsWith('/')) return false
    if (cleaned.startsWith('data:')) return false
    const base64Regex = /^[A-Za-z0-9+/=_-]+$/
    if (!base64Regex.test(cleaned)) return false
    try {
      atob(cleaned.slice(0, 100))
      return true
    } catch {
      return false
    }
  }

  const detectImageFormat = (base64Data: string): { mime: string } | null => {
    try {
      const pureBase64 = base64Data.replace(/^data:image\/[^;]+;base64,/, '').replace(/^data:,/, '')
      const binaryString = atob(pureBase64.slice(0, 32))
      const magicBytes: Record<string, { magic: number[]; mime: string }> = {
        png: { magic: [0x89, 0x50, 0x4E, 0x47], mime: 'image/png' },
        jpeg: { magic: [0xFF, 0xD8, 0xFF], mime: 'image/jpeg' },
        gif: { magic: [0x47, 0x49, 0x46], mime: 'image/gif' },
        webp: { magic: [0x52, 0x49, 0x46, 0x46], mime: 'image/webp' },
        bmp: { magic: [0x42, 0x4D], mime: 'image/bmp' },
      }
      for (const info of Object.values(magicBytes)) {
        if (info.magic.every((byte, i) => binaryString.charCodeAt(i) === byte)) {
          return { mime: info.mime }
        }
      }
    } catch {
      // Invalid base64
    }
    return null
  }

  const normalizeImageUrl = (value: unknown): string | null => {
    if (!value) return null
    const valueStr = String(value)
    const trimmed = valueStr.trim()
    if (trimmed === '-' || trimmed === 'undefined' || trimmed === 'null' || trimmed === '') return null
    if (trimmed.startsWith('data:image/')) return trimmed
    if (trimmed.startsWith('data:base64,')) {
      const base64Data = trimmed.slice(12)
      const formatInfo = detectImageFormat(base64Data) || { mime: 'image/png' }
      return `data:${formatInfo.mime};base64,${base64Data}`
    }
    if (isPureBase64(trimmed)) {
      const formatInfo = detectImageFormat(trimmed) || { mime: 'image/png' }
      return `data:${formatInfo.mime};base64,${trimmed}`
    }
    return trimmed
  }

  const extractImageData = (data: any) => {
    const values = data?.values
    if (!values) return null

    // Helper to check and normalize image value
    const checkImageValue = (val: any): { src: string; mimeType?: string } | null => {
      // Skip null/undefined
      if (val == null) return null

      // If it's a string, check if it's an image
      if (typeof val === 'string') {
        const str = val.trim()
        // Check for data URL format
        if (str.startsWith('data:image/')) {
          return { src: str }
        }
        // Check for data:image/jpeg;base64, format (may have comma typo)
        if (str.startsWith('data:image/') && str.includes('base64,')) {
          return { src: str }
        }
        // Check for common base64 patterns
        if (str.length > 100 && (str.includes('/9j/') || str.includes('iVBORw0KGgo'))) {
          // Raw base64 - detect format and add prefix
          const mime = str.includes('iVBORw0KGgo') ? 'image/png' : 'image/jpeg'
          return { src: `data:${mime};base64,${str}` }
        }
        // Check for URL
        if (str.startsWith('http://') || str.startsWith('https://') || str.startsWith('/')) {
          return { src: str }
        }
      }

      // If it's a number or other type, try to convert to string and check
      const str = String(val).trim()
      if (str.length > 100 && (str.includes('/9j/') || str.includes('iVBORw0KGgo'))) {
        const mime = str.includes('iVBORw0KGgo') ? 'image/png' : 'image/jpeg'
        return { src: `data:${mime};base64,${str}` }
      }

      return null
    }

    // If values is an array
    if (Array.isArray(values)) {
      for (const item of values) {
        if (typeof item === 'object' && item !== null) {
          // Check common image keys in object
          for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src', 'value']) {
            const result = checkImageValue(item[key])
            if (result) return { ...result, mimeType: item.image_mime_type || item.mimeType }
          }
        }
        // Check if the item itself is an image string
        const result = checkImageValue(item)
        if (result) return result
      }
    }

    // If values is an object
    if (typeof values === 'object' && values !== null) {
      // Check common image keys
      for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src', 'value']) {
        const result = checkImageValue(values[key])
        if (result) return { ...result, mimeType: values.image_mime_type || values.mimeType }
      }
    }

    // If values is a string or other primitive
    const result = checkImageValue(values)
    if (result) return result

    return null
  }

  const getDataDisplayPairs = (data: any) => {
    const values = data?.values
    const pairs: { key: string; value: string }[] = []
    if (!values) return pairs

    if (Array.isArray(values)) {
      values.forEach((item, idx) => {
        if (typeof item !== 'object' || item === null) {
          pairs.push({ key: `[${idx}]`, value: String(item ?? '-') })
        } else {
          for (const [k, v] of Object.entries(item)) {
            if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
              pairs.push({ key: k, value: String(v ?? '-') })
            }
          }
        }
      })
    } else if (typeof values === 'object') {
      for (const [k, v] of Object.entries(values)) {
        if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
          pairs.push({ key: k, value: String(v ?? '-') })
        }
      }
    } else {
      pairs.push({ key: 'value', value: String(values) })
    }

    return pairs
  }

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent
        ref={dialogRef}
        className="max-w-2xl max-h-[80vh]"
        onOpenAutoFocus={(e) => {
          e.preventDefault()
        }}
        onPointerDownCapture={(e) => {
          // Prevent event from reaching react-grid-layout
          e.stopPropagation()
        }}
        onMouseDownCapture={(e) => {
          // Prevent event from reaching react-grid-layout
          e.stopPropagation()
        }}
      >
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

            {/* Input Data - with Image Support */}
            {!loading && dataCollected.length > 0 && (
              <div className="space-y-3">
                <h4 className="text-sm font-medium flex items-center gap-2">
                  <Monitor className="h-4 w-4" />
                  输入数据
                  <span className="text-xs text-muted-foreground">({dataCollected.length})</span>
                </h4>
                <div className="space-y-2">
                  {dataCollected.map((data: any, idx: number) => {
                    const imageData = extractImageData(data)
                    const hasImage = imageData !== null
                    const dataPairs = getDataDisplayPairs(data)
                    const isExpanded = expandedDataIndices.has(idx)

                    return (
                      <div key={idx} className="border rounded-lg overflow-hidden">
                        {/* Header */}
                        <div
                          className="flex items-center justify-between p-2 bg-muted/50 cursor-pointer hover:bg-muted/70 transition-colors"
                          onClick={() => toggleDataExpanded(idx)}
                        >
                          <div className="flex items-center gap-2 min-w-0 flex-1">
                            {hasImage && <ImageIcon className="h-3 w-3 text-purple-500 shrink-0" />}
                            <span className="text-xs font-medium truncate">{data.source}</span>
                            <Badge variant="outline" className="text-[9px] h-4 px-1 shrink-0">{data.data_type}</Badge>
                          </div>
                          {dataPairs.length > 0 && (
                            isExpanded ? (
                              <ChevronUp className="h-3 w-3 text-muted-foreground shrink-0" />
                            ) : (
                              <ChevronDown className="h-3 w-3 text-muted-foreground shrink-0" />
                            )
                          )}
                        </div>

                        {/* Image Preview */}
                        {hasImage && (
                          <div className="p-2 bg-black/5">
                            <img
                              src={imageData!.src}
                              alt={`${data.source} - 输入图像`}
                              className="w-full max-h-[200px] object-contain rounded-md bg-background"
                              loading="lazy"
                            />
                          </div>
                        )}

                        {/* Additional Data */}
                        {isExpanded && dataPairs.length > 0 && (
                          <div className="p-2 border-t bg-muted/30">
                            <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[10px]">
                              {dataPairs.slice(0, 10).map((pair, pairIdx) => (
                                <div key={pairIdx} className="flex items-baseline gap-1 min-w-0">
                                  <span className="text-muted-foreground shrink-0">{pair.key}:</span>
                                  <span className="truncate font-mono">{pair.value}</span>
                                </div>
                              ))}
                              {dataPairs.length > 10 && (
                                <div className="col-span-2 text-muted-foreground text-[9px]">
                                  +{dataPairs.length - 10} more fields
                                </div>
                              )}
                            </div>
                          </div>
                        )}

                        {/* Expand hint */}
                        {!isExpanded && dataPairs.length > 0 && (
                          <div className="px-2 pb-1">
                            <span className="text-[9px] text-muted-foreground">
                              {dataPairs.length} 个数据字段
                            </span>
                          </div>
                        )}
                      </div>
                    )
                  })}
                </div>
              </div>
            )}

            {/* Decision Process */}
            {decisionProcess && (decisionProcess.situation_analysis || decisionProcess.conclusion || decisionProcess.reasoning_steps.length > 0) && (
              <div className="space-y-3">
                <h4 className="text-sm font-medium flex items-center gap-2">
                  <Brain className="h-4 w-4" />
                  Decision Process
                </h4>
                <div className="space-y-3 text-sm">
                  {decisionProcess.situation_analysis && (
                    <div>
                      <span className="text-muted-foreground">Situation Analysis</span>
                      <p className="mt-1 whitespace-pre-wrap">{decisionProcess.situation_analysis}</p>
                    </div>
                  )}
                  {decisionProcess.reasoning_steps.length > 0 && (
                    <div>
                      <span className="text-muted-foreground">Reasoning Steps</span>
                      <div className="mt-2 space-y-2">
                        {decisionProcess.reasoning_steps.map((step, i) => (
                          <div key={i} className="p-2 bg-muted/50 rounded">
                            <span className="text-muted-foreground">Step {step.step_number ?? i + 1}:</span>
                            <p className="mt-1 whitespace-pre-wrap">{step.description}</p>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                  {decisionProcess.conclusion && (
                    <div>
                      <span className="text-muted-foreground">Conclusion</span>
                      <p className="mt-1 whitespace-pre-wrap">{decisionProcess.conclusion}</p>
                    </div>
                  )}
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

  // History Tab - expanded executions and details
  const [expandedExecutions, setExpandedExecutions] = useState<Set<string>>(new Set())
  const [executionDetails, setExecutionDetails] = useState<Record<string, any>>({})
  const [loadingDetails, setLoadingDetails] = useState<Set<string>>(new Set())

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
    setAgent(null)  // Reset agent state when agentId changes
    setExecutions([])  // Reset executions when agentId changes
    loadAgent()
    loadExecutions()
  }, [agentId])  // Depend on agentId directly, not callback functions

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

  // Toggle execution expansion in History tab
  const toggleExecution = async (executionId: string) => {
    const newExpanded = new Set(expandedExecutions)
    const isExpanding = !newExpanded.has(executionId)

    if (isExpanding) {
      newExpanded.add(executionId)
      // Load details if not already loaded
      if (!executionDetails[executionId]) {
        setLoadingDetails(prev => new Set(prev).add(executionId))
        try {
          const data = await api.getAgentExecution(agentId!, executionId)
          setExecutionDetails(prev => ({ ...prev, [executionId]: data }))
        } catch (error) {
          console.error('Failed to load execution detail:', error)
        } finally {
          setLoadingDetails(prev => {
            const next = new Set(prev)
            next.delete(executionId)
            return next
          })
        }
      }
    } else {
      newExpanded.delete(executionId)
    }
    setExpandedExecutions(newExpanded)
  }

  // Helper function to normalize decision process
  const normalizeDecisionProcessForDisplay = (raw: unknown) => {
    if (raw == null) return null
    let dp = raw
    if (typeof dp === 'string') {
      try {
        dp = JSON.parse(dp) as Record<string, unknown>
      } catch {
        return null
      }
    }
    if (typeof dp !== 'object' || dp === null) return null
    const obj = dp as Record<string, unknown>
    let situation_analysis = (obj.situation_analysis as string) ?? ''
    let conclusion = (obj.conclusion as string) ?? ''
    let reasoning_steps = Array.isArray(obj.reasoning_steps) ? obj.reasoning_steps : []

    // If situation_analysis looks like JSON, parse and extract
    if (typeof situation_analysis === 'string' && situation_analysis.trim().startsWith('{')) {
      try {
        const parsed = JSON.parse(situation_analysis) as Record<string, unknown>
        situation_analysis = (parsed.situation_analysis as string) ?? situation_analysis
        conclusion = (parsed.conclusion as string) ?? conclusion
        if (Array.isArray(parsed.reasoning_steps)) reasoning_steps = parsed.reasoning_steps
      } catch {
        // keep as-is
      }
    }

    return {
      situation_analysis: situation_analysis || '',
      reasoning_steps: reasoning_steps.map((s: unknown, i: number) => {
        if (s && typeof s === 'object' && 'description' in s) {
          return { description: (s as Record<string, unknown>).description as string, step_number: i + 1 }
        }
        if (s && typeof s === 'object' && 'output' in s) {
          return { description: (s as Record<string, unknown>).output as string, step_number: i + 1 }
        }
        return { description: String(s), step_number: i + 1 }
      }),
      conclusion: conclusion || '',
    }
  }

  // Display agent and executions - show empty state when no data
  const displayAgent = agent
  const displayExecutions = executions

  // Calculate stats - from agent or executions
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
      <div className={cn("bg-card/50 backdrop-blur rounded-lg border-0 shadow-sm overflow-hidden flex flex-col w-full h-full", className)}>
        {/* Compact Header - Agent Info (glass) */}
        <div className="px-3 py-2 border-b border-border/50 bg-muted/30 backdrop-blur-sm shrink-0">
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

        {/* Compact Tabs List (glass, same as header) */}
        <div className="px-2 pt-1.5 pb-0 shrink-0 bg-muted/30 backdrop-blur-sm -mb-px">
          <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as WidgetTab)} className="w-full">
            <TabsList className="w-full justify-start bg-transparent border-0 shadow-none h-7 px-0">
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

        {/* Tab Content - glass background aligned with header/tabs */}
        <div className="w-full flex-1 min-h-0 overflow-hidden px-2 pt-2 bg-muted/30 backdrop-blur-sm rounded-b-lg">
          <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as WidgetTab)} className="w-full h-full flex flex-col">
            {/* Overview Tab Content */}
            <TabsContent value="overview" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full gap-2 overflow-hidden p-2">
                {/* Compact Stats Bar */}
                <div className="w-full flex items-center divide-x divide-border/50 border border-border/50 rounded bg-muted/30 shrink-0">
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
                <div className="w-full flex-1 min-h-0 flex flex-col bg-background/50 rounded overflow-hidden">
                  <div className="w-full px-2 py-1 border-b border-border/30 flex items-center justify-between rounded-t shrink-0">
                    <span className="text-[10px] font-medium">Recent</span>
                    <span className="text-[8px] text-muted-foreground">
                      {displayExecutions.slice(0, 5).length}
                    </span>
                  </div>

                  {/* Real-time Progress - shown inside Recent section during execution */}
                  {currentlyExecuting && (
                    <div className="px-1">
                      <div className="w-full flex items-center gap-2 py-1.5 px-2 rounded bg-primary/5 border border-primary/20">
                        {/* Status icon - animated */}
                        <div className={cn(
                          "w-1.5 h-1.5 rounded-full animate-pulse shrink-0",
                          currentStage === 'collecting' ? "bg-blue-500" :
                          currentStage === 'analyzing' ? "bg-purple-500" : "bg-green-500"
                        )} />

                        {/* Content area */}
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-1.5">
                            <span className="text-[10px] font-medium text-foreground/80">
                              {stageLabel || 'Processing'}
                            </span>
                            {/* Progress bar - inline */}
                            <div className="flex-1 h-1 bg-muted/30 rounded-full overflow-hidden max-w-[80px]">
                              <div className={cn(
                                "h-full transition-all duration-500 ease-out",
                                "bg-gradient-to-r from-blue-500 via-purple-500 to-green-500",
                                "animate-[shimmer_2s_infinite]"
                              )} style={{
                                width: currentStage === 'collecting' ? '33%' : currentStage === 'analyzing' ? '66%' : '90%'
                              }} />
                            </div>
                          </div>
                          {/* Current thinking step - single line, truncates */}
                          {thinkingSteps.length > 0 && (
                            <p className="text-[10px] text-muted-foreground truncate mt-0.5">
                              {thinkingSteps[thinkingSteps.length - 1]?.description}
                            </p>
                          )}
                        </div>

                        {/* Time/elapsed indicator */}
                        <span className="text-[10px] text-muted-foreground font-mono shrink-0">
                          Running...
                        </span>

                        {/* Chevron indicator */}
                        <Loader2 className="h-3 w-3 text-muted-foreground shrink-0 animate-spin" />
                      </div>
                    </div>
                  )}

                  <ScrollArea className="flex-1 w-full">
                    {displayExecutions.length === 0 ? (
                      <div className="flex items-center justify-center h-full min-h-[120px] text-center">
                        <div className="flex flex-col items-center gap-2">
                          <MoreHorizontal className="h-6 w-6 text-muted-foreground opacity-50" />
                          <p className="text-[10px] text-muted-foreground">No history</p>
                        </div>
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
              <div className="w-full flex flex-col h-full rounded overflow-hidden p-2">
                <div className="px-2 py-1 border-b border-border/30 flex items-center justify-between rounded-t shrink-0">
                  <span className="text-[10px] font-medium">History</span>
                  <span className="text-[8px] text-muted-foreground">
                    {displayExecutions.length}
                  </span>
                </div>

                <ScrollArea className="flex-1 w-full">
                  {displayExecutions.length === 0 ? (
                    <div className="flex items-center justify-center h-full min-h-[120px] text-center">
                      <div className="flex flex-col items-center gap-2">
                        <MoreHorizontal className="h-6 w-6 text-muted-foreground opacity-50" />
                        <p className="text-[10px] text-muted-foreground">No history</p>
                      </div>
                    </div>
                  ) : (
                    <div className="w-full p-2 space-y-3">
                      {displayExecutions.map((exec, index) => {
                        const isExpanded = expandedExecutions.has(exec.id)
                        const detail = executionDetails[exec.id]
                        const isLoadingDetail = loadingDetails.has(exec.id)
                        const isLatest = index === 0 && exec.id === newExecutionId

                        const getStatusConfig = () => {
                          switch (exec.status) {
                            case 'Running':
                              return { icon: Loader2, color: 'text-blue-500', bg: 'bg-blue-500/10 border-blue-500/20' }
                            case 'Completed':
                              return { icon: CheckCircle2, color: 'text-green-500', bg: 'bg-green-500/10 border-green-500/20' }
                            case 'Failed':
                            case 'Cancelled':
                              return { icon: XCircle, color: 'text-red-500', bg: 'bg-red-500/10 border-red-500/20' }
                            default:
                              return { icon: AlertCircle, color: 'text-gray-500', bg: 'bg-gray-500/10 border-gray-500/20' }
                          }
                        }

                        const statusConfig = getStatusConfig()
                        const StatusIcon = statusConfig.icon

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
                          <div key={exec.id} className="relative">
                            {/* Timeline Card */}
                            <div
                              className={cn(
                                "border rounded-lg overflow-hidden transition-all",
                                isExpanded && statusConfig.bg,
                                !isExpanded && "hover:bg-muted/30"
                              )}
                            >
                              {/* Header - Always Visible */}
                              <button
                                type="button"
                                onClick={() => {
                                  toggleExecution(exec.id)
                                }}
                                className="w-full p-2 flex items-center gap-2 text-left"
                              >
                                <StatusIcon className={cn("h-3.5 w-3.5 shrink-0", exec.status === 'Running' && "animate-spin")} />
                                <div className="flex-1 min-w-0">
                                  <div className="flex items-center gap-1.5">
                                    <span className="text-[10px] font-mono text-muted-foreground">
                                      #{exec.id.slice(-6)}
                                    </span>
                                    {isLatest && (
                                      <Badge variant="outline" className="text-[8px] h-3.5 px-1">
                                        New
                                      </Badge>
                                    )}
                                    <span className="text-[9px] text-muted-foreground">{exec.trigger_type}</span>
                                  </div>
                                  <div className="flex items-center gap-2 text-[9px] text-muted-foreground mt-0.5">
                                    <span>{formatTime(exec.timestamp)}</span>
                                    {exec.duration_ms > 0 && (
                                      <span>{formatDuration(exec.duration_ms)}</span>
                                    )}
                                  </div>
                                </div>
                                <div className="shrink-0">
                                  {isExpanded ? (
                                    <ChevronUp className="h-3 w-3 text-muted-foreground" />
                                  ) : (
                                    <ChevronDown className="h-3 w-3 text-muted-foreground" />
                                  )}
                                </div>
                              </button>

                              {/* Expanded Details */}
                              {isExpanded && (
                                <div className="border-t p-2">
                                  {isLoadingDetail ? (
                                    <div className="flex items-center justify-center py-4">
                                      <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                                    </div>
                                  ) : detail ? (
                                    <div className="space-y-2">
                                      {/* Situation Analysis */}
                                      {detail.decision_process?.situation_analysis && (
                                        <div>
                                          <div className="flex items-center gap-1 mb-1">
                                            <Brain className="h-2.5 w-2.5 text-purple-500" />
                                            <span className="text-[9px] font-medium text-muted-foreground">Situation Analysis</span>
                                          </div>
                                          <p className="text-[10px] pl-3">{detail.decision_process.situation_analysis}</p>
                                        </div>
                                      )}

                                      {/* Data Collected */}
                                      {detail.decision_process?.data_collected && detail.decision_process.data_collected.length > 0 && (
                                        <div>
                                          <div className="flex items-center gap-1 mb-1">
                                            <Database className="h-2.5 w-2.5 text-blue-500" />
                                            <span className="text-[9px] font-medium text-muted-foreground">Data Collected</span>
                                            <span className="text-[8px] text-muted-foreground">({detail.decision_process.data_collected.length})</span>
                                          </div>
                                          <div className="grid grid-cols-2 gap-1">
                                            {detail.decision_process.data_collected.map((data: any, idx: number) => (
                                              <div key={idx} className="text-[9px] p-1.5 rounded bg-muted/30">
                                                <div className="flex items-center justify-between mb-0.5">
                                                  <span className="text-[8px] font-medium truncate flex-1" title={data.source}>{data.source}</span>
                                                  <Badge variant="outline" className="text-[7px] h-3 px-0.5 shrink-0 ml-1">{data.data_type}</Badge>
                                                </div>
                                              </div>
                                            ))}
                                          </div>
                                        </div>
                                      )}

                                      {/* Reasoning Steps */}
                                      {detail.decision_process?.reasoning_steps && detail.decision_process.reasoning_steps.length > 0 && (
                                        <div>
                                          <div className="flex items-center gap-1 mb-1">
                                            <ChevronRight className="h-2.5 w-2.5 text-orange-500" />
                                            <span className="text-[9px] font-medium text-muted-foreground">Reasoning Steps</span>
                                          </div>
                                          <div className="space-y-1 pl-3">
                                            {detail.decision_process.reasoning_steps.slice(0, 3).map((step: any, idx: number) => (
                                              <div key={idx} className="text-[9px]">
                                                <span className="text-muted-foreground">Step {step.step_number ?? idx + 1}:</span>
                                                <span className="ml-1">{step.description}</span>
                                              </div>
                                            ))}
                                            {detail.decision_process.reasoning_steps.length > 3 && (
                                              <div className="text-[8px] text-muted-foreground">
                                                +{detail.decision_process.reasoning_steps.length - 3} more steps
                                              </div>
                                            )}
                                          </div>
                                        </div>
                                      )}

                                      {/* Decisions */}
                                      {detail.decision_process?.decisions && detail.decision_process.decisions.length > 0 && (
                                        <div>
                                          <div className="flex items-center gap-1 mb-1">
                                            <Zap className="h-2.5 w-2.5 text-green-500" />
                                            <span className="text-[9px] font-medium text-muted-foreground">Decisions</span>
                                          </div>
                                          <div className="space-y-1">
                                            {detail.decision_process.decisions.map((decision: any, idx: number) => (
                                              <div key={idx} className="text-[9px] p-1.5 rounded bg-muted/30">
                                                <div className="font-medium">{decision.description}</div>
                                                {decision.action && (
                                                  <div className="text-[8px] text-muted-foreground mt-0.5">
                                                    Action: <span className="font-mono">{decision.action}</span>
                                                  </div>
                                                )}
                                              </div>
                                            ))}
                                          </div>
                                        </div>
                                      )}

                                      {/* Conclusion */}
                                      {detail.decision_process?.conclusion && (
                                        <div className="p-2 bg-muted/50 rounded">
                                          <div className="text-[9px] font-medium">Conclusion</div>
                                          <p className="text-[10px] mt-1">{detail.decision_process.conclusion}</p>
                                        </div>
                                      )}

                                      {/* Actions Executed */}
                                      {detail.result?.actions_executed && detail.result.actions_executed.length > 0 && (
                                        <div>
                                          <div className="flex items-center gap-1 mb-1">
                                            <Zap className="h-2.5 w-2.5 text-yellow-500" />
                                            <span className="text-[9px] font-medium text-muted-foreground">Actions Executed</span>
                                          </div>
                                          <div className="space-y-1">
                                            {detail.result.actions_executed.map((action: any, idx: number) => (
                                              <div key={idx} className={cn(
                                                "text-[9px] p-1.5 rounded border",
                                                action.success ? "bg-green-500/10 border-green-500/20" : "bg-red-500/10 border-red-500/20"
                                              )}>
                                                <div className="flex items-center justify-between">
                                                  <span className="font-medium truncate flex-1" title={action.description}>{action.description}</span>
                                                  <Badge variant={action.success ? "default" : "destructive"} className="text-[7px] h-3 px-0.5 shrink-0 ml-1">
                                                    {action.success ? '✓' : '✗'}
                                                  </Badge>
                                                </div>
                                              </div>
                                            ))}
                                          </div>
                                        </div>
                                      )}

                                      {/* View full detail button */}
                                      <button
                                        type="button"
                                        onClick={() => handleExecutionClick(exec)}
                                        className="w-full text-[9px] py-1 px-2 rounded bg-primary/10 hover:bg-primary/20 text-primary text-center transition-colors"
                                      >
                                        View Full Detail
                                      </button>
                                    </div>
                                  ) : (
                                    <div className="text-center py-2 text-[9px] text-muted-foreground">
                                      No detail available
                                    </div>
                                  )}
                                </div>
                              )}
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  )}
                </ScrollArea>
              </div>
            </TabsContent>

            {/* Memory Tab Content */}
            <TabsContent value="memory" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full rounded overflow-hidden p-2">
                <ScrollArea className="flex-1 w-full">
                  <MemoryContent memory={memory} loading={memoryLoading} />
                </ScrollArea>
              </div>
            </TabsContent>

            {/* Messages Tab Content */}
            <TabsContent value="messages" className="w-full flex-1 min-h-0 data-[state=active]:flex data-[state=inactive]:hidden">
              <div className="w-full flex flex-col h-full rounded overflow-hidden p-2">
                {/* Header */}
                <div className="w-full px-2 py-1 border-b border-border/30 flex items-center rounded-t shrink-0">
                  <span className="text-[10px] font-medium">Messages</span>
                  {userMessages.length > 0 && (
                    <span className="text-[8px] text-muted-foreground ml-auto">
                      {userMessages.length}
                    </span>
                  )}
                </div>

                {/* Messages list - scrollable */}
                <ScrollArea className="flex-1 w-full">
                  {userMessages.length === 0 ? (
                    <div className="flex items-center justify-center h-full min-h-[120px] text-center">
                      <div className="flex flex-col items-center gap-2">
                        <MessageSquare className="h-6 w-6 text-muted-foreground opacity-50" />
                        <p className="text-[10px] text-muted-foreground">No messages</p>
                      </div>
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
                <div className="w-full border border-border/30 bg-muted/20 rounded p-1.5 shrink-0 mt-1.5">
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

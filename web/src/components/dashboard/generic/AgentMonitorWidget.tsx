/**
 * Agent Monitor Widget
 *
 * A widget for monitoring AI agent with a data-flow style layout:
 * - Top: Agent basic info + user prompt
 * - Middle: Execution history as a data flow stream (with input data/images)
 * - Bottom: User input for adding task hints
 */

import { useState, useCallback, useEffect, useRef, useMemo, memo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Bot,
  CheckCircle2,
  XCircle,
  Loader2,
  Clock,
  AlertCircle,
  ChevronRight,
  Brain,
  Send,
  MessageSquare,
  Zap,
  Activity,
  Target,
  CircleDot,
  Pause,
  Database,
  Maximize2,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { textMicro, badgeSize, textNano, textMini } from '@/design-system/tokens/typography'
import { api } from '@/lib/api'
import { resolveImageSrc } from '@/lib/imageUtils'
import { MarkdownMessage } from '@/components/chat/MarkdownMessage'
import { useEvents } from '@/hooks/useEvents'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { dashboardCardBase } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { LoadingState } from '../shared'
import type { AiAgent, AgentExecution, DataCollected } from '@/types'
import type {
  AgentExecutionStartedEvent,
  AgentExecutionCompletedEvent,
  AgentThinkingEvent,
  AgentDecisionEvent,
  AgentProgressEvent,
} from '@/lib/events'

interface AgentMonitorWidgetProps {
  className?: string
  agentId?: string
  editMode?: boolean
}

// Extract images from data_collected values
function extractImagesFromData(data: DataCollected[]): Array<{ source: string; image: string; timestamp: number }> {
  const images: Array<{ source: string; image: string; timestamp: number }> = []

  for (const item of data) {
    const values = item.values
    if (!values) continue

    // Handle array values
    const valueList = Array.isArray(values) ? values : [values]

    for (const value of valueList) {
      if (!value) continue

      // Direct string value that might be an image (base64, /api/images/, or image URL)
      if (typeof value === 'string') {
        const imgSrc = resolveImageSrc(value)
        if (imgSrc) {
          images.push({
            source: item.source,
            image: imgSrc,
            timestamp: item.timestamp
          })
          continue
        }
      }

      // Object with image field
      if (typeof value === 'object') {
        const obj = value as Record<string, unknown>

        // Check image_url first — it's a direct URL, no base64 detection needed
        for (const key of ['image_url', 'url', 'src']) {
          const v = obj[key]
          if (typeof v === 'string' && (v.startsWith('http://') || v.startsWith('https://') || v.startsWith('/'))) {
            images.push({
              source: `${item.source}.${key}`,
              image: v,
              timestamp: item.timestamp
            })
            break
          }
        }

        // Check image fields (base64, /api/images/, or image URL)
        if (images.length === 0) {
          for (const key of ['image', 'image_base64', 'src', 'url', 'data', 'value', 'base64', 'image_data']) {
            const v = obj[key]
            if (typeof v === 'string') {
              const imgSrc = resolveImageSrc(v)
              if (imgSrc) {
                images.push({
                  source: `${item.source}.${key}`,
                  image: imgSrc,
                  timestamp: item.timestamp
                })
                break
              }
            }
          }
        }
      }
    }
  }

  return images
}

// Clean stage label - remove emojis and use English defaults for non-English text
function cleanStageLabel(stage: string | null, label: string | null): string {
  // Remove emojis
  const cleanLabel = label?.replace(/[\u{1F300}-\u{1F9FF}]|[\u{2600}-\u{26FF}]|[\u{2700}-\u{27BF}]/gu, '').trim()

  // Default English labels based on stage
  if (stage === 'collecting') return 'Collecting data'
  if (stage === 'analyzing') return 'Analyzing data'
  if (stage === 'deciding') return 'Making decision'
  if (stage === 'executing') return 'Executing actions'

  // If label is empty or contains Chinese, use default
  if (!cleanLabel || /[\u4e00-\u9fff]/.test(cleanLabel)) {
    return 'Processing...'
  }

  return cleanLabel
}

// Extract metric names/tags from data_collected
function extractMetricTags(data: DataCollected[]): string[] {
  const tags: Set<string> = new Set()

  for (const item of data) {
    // Add source name as tag
    if (item.source) {
      tags.add(item.source)
    }

    // Add data_type if it's meaningful
    if (item.data_type && item.data_type !== 'unknown' && item.data_type !== 'object') {
      tags.add(item.data_type)
    }

    // Extract metric names from values
    const values = item.values
    if (!values) continue

    const valueList = Array.isArray(values) ? values : [values]
    for (const value of valueList) {
      if (!value) continue

      if (typeof value === 'object') {
        const obj = value as Record<string, unknown>
        // Look for metric name fields
        for (const key of ['name', 'metric', 'metric_name', 'display_name', 'key', 'field']) {
          if (obj[key] && typeof obj[key] === 'string') {
            tags.add(obj[key] as string)
          }
        }
        // Also add object keys that might be metric names (excluding common non-metric keys)
        const excludeKeys = ['value', 'data', 'image', 'src', 'url', 'base64', 'timestamp', 'time', 'id', '_id']
        for (const key of Object.keys(obj)) {
          if (!excludeKeys.includes(key.toLowerCase()) && !key.startsWith('_')) {
            const v = obj[key]
            if (typeof v === 'number' || (typeof v === 'string' && v.length < 30)) {
              tags.add(key)
            }
          }
        }
      }
    }
  }

  return Array.from(tags).slice(0, 8) // Limit to 8 tags
}

// Normalize decision_process
function normalizeDecisionProcess(raw: unknown): {
  situation_analysis: string
  reasoning_steps: Array<{ description: string; step_number?: number }>
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

// Execution Detail Dialog
interface ExecutionDetailDialogProps {
  execution: AgentExecution | null
  open: boolean
  onClose: () => void
  agentId: string
}

function ExecutionDetailDialog({ execution, open, onClose, agentId }: ExecutionDetailDialogProps) {
  const { t } = useTranslation('agents')
  const [detail, setDetail] = useState<any>(null)
  const [loading, setLoading] = useState(false)
  const [fullscreenImage, setFullscreenImage] = useState<string | null>(null)

  // Prevent loading flash: only show spinner when loading AND no detail exists yet
  const showLoading = loading && !detail
  const dialogRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!dialogRef.current) return
    const dialogElement = dialogRef.current
    const stopPropagation = (e: Event) => {
      e.stopPropagation()
      e.stopImmediatePropagation()
    }
    dialogElement.addEventListener('mousedown', stopPropagation, { capture: true })
    dialogElement.addEventListener('touchstart', stopPropagation, { capture: true })
    return () => {
      dialogElement.removeEventListener('mousedown', stopPropagation, { capture: true } as any)
      dialogElement.removeEventListener('touchstart', stopPropagation, { capture: true } as any)
    }
  }, [open])

  useEffect(() => {
    if (open && execution?.id) {
      setLoading(true)
      api.getAgentExecution(agentId, execution.id)
        .then(setDetail)
        .catch(console.error)
        .finally(() => setLoading(false))
    }
  }, [open, execution, agentId])

  if (!execution) return null

  const decisionProcess = detail?.decision_process != null ? normalizeDecisionProcess(detail.decision_process) : null
  const dataCollected = detail?.decision_process?.data_collected || []
  const images = extractImagesFromData(dataCollected)
  const metricTags = extractMetricTags(dataCollected)

  return (
    <>
      <Dialog open={open} onOpenChange={(v) => !v && onClose()}>
        <DialogContent ref={dialogRef} className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col z-[110]">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2 text-base">
              {execution.status === 'Completed' ? (
                <CheckCircle2 className="h-4 w-4 text-success" />
              ) : execution.status === 'Running' ? (
                <Loader2 className="h-4 w-4 text-info animate-spin" />
              ) : (
                <XCircle className="h-4 w-4 text-error" />
              )}
              <span>#{execution.id.slice(-6)}</span>
              <Badge variant="outline" className={textNano}>{execution.trigger_type}</Badge>
            </DialogTitle>
            <DialogDescription className="sr-only">Execution details</DialogDescription>
          </DialogHeader>

          <div className="flex-1 overflow-auto space-y-4">
            {showLoading ? (
              <LoadingState size="md" />
            ) : detail ? (
              <>
                {/* Timing */}
                <div className="flex items-center gap-4 text-xs text-muted-foreground">
                  <div className="flex items-center gap-1">
                    <Clock className="h-4 w-4" />
                    {new Date(execution.timestamp).toLocaleString()}
                  </div>
                  {execution.duration_ms > 0 && (
                    <div className="flex items-center gap-1">
                      <Activity className="h-4 w-4" />
                      {execution.duration_ms < 1000 ? `${execution.duration_ms}ms` : `${(execution.duration_ms / 1000).toFixed(1)}s`}
                    </div>
                  )}
                </div>

                {/* Error */}
                {execution.error && (
                  <div className="p-3 rounded-lg bg-error-light border border-error text-sm text-error">
                    {execution.error}
                  </div>
                )}

                {/* Input Data Section */}
                {(images.length > 0 || metricTags.length > 0) && (
                  <div className="space-y-2">
                    <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                      <Database className="h-4 w-4" />
                      Input Data
                    </div>

                    {/* Images Grid */}
                    {images.length > 0 && (
                      <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                        {images.map((img, idx) => (
                          <div
                            key={idx}
                            className="relative group rounded-lg overflow-hidden border border-border bg-muted-30 cursor-pointer aspect-video"
                            onClick={() => setFullscreenImage(img.image)}
                          >
                            <img
                              src={img.image}
                              alt={img.source}
                              className="w-full h-full object-cover"
                            />
                            <div className="absolute inset-0 bg-gradient-to-t from-black/70 via-transparent to-transparent opacity-0 group-hover:opacity-100 transition-opacity" />
                            <div className="absolute bottom-0 left-0 right-0 p-1.5 opacity-0 group-hover:opacity-100 transition-opacity">
                              <span className={cn(textMicro, "text-white/90 truncate block")}>{img.source}</span>
                            </div>
                            <div className="absolute top-1 right-1 opacity-0 group-hover:opacity-100 transition-opacity bg-overlay-medium rounded p-0.5">
                              <Maximize2 className="h-4 w-4 text-white" />
                            </div>
                          </div>
                        ))}
                      </div>
                    )}

                    {/* Metric Tags */}
                    {metricTags.length > 0 && (
                      <div className="flex flex-wrap gap-1.5">
                        {metricTags.map((tag, idx) => (
                          <Badge key={idx} variant="secondary" className={cn(textNano, "h-5 px-1.5")}>
                            {tag}
                          </Badge>
                        ))}
                      </div>
                    )}
                  </div>
                )}

                {/* Decision Process */}
                {decisionProcess && (
                  <div className="space-y-3">
                    {decisionProcess.situation_analysis && (
                      <div>
                        <div className="flex items-center gap-1.5 mb-1.5 text-xs font-medium text-muted-foreground">
                          <Target className="h-4 w-4" />
                          Analysis
                        </div>
                        <p className="text-sm bg-muted-50 p-3 rounded-lg">{decisionProcess.situation_analysis}</p>
                      </div>
                    )}

                    {decisionProcess.reasoning_steps.length > 0 && (
                      <div>
                        <div className="flex items-center gap-1.5 mb-1.5 text-xs font-medium text-muted-foreground">
                          <Brain className="h-4 w-4" />
                          Reasoning
                        </div>
                        <div className="space-y-1">
                          {decisionProcess.reasoning_steps.map((step, idx) => (
                            <div key={idx} className="flex items-start gap-2 text-sm">
                              <span className="text-muted-foreground font-mono text-xs">#{step.step_number || idx + 1}</span>
                              <span>{step.description}</span>
                            </div>
                          ))}
                        </div>
                      </div>
                    )}

                    {decisionProcess.conclusion && (
                      <div>
                        <div className="flex items-center gap-1.5 mb-1.5 text-xs font-medium text-muted-foreground">
                          <CheckCircle2 className="h-4 w-4" />
                          Conclusion
                        </div>
                        <div className="text-sm bg-success-light p-3 rounded-lg border border-success-light prose prose-sm max-w-none"><MarkdownMessage content={decisionProcess.conclusion} /></div>
                      </div>
                    )}
                  </div>
                )}

                {/* Actions Executed */}
                {detail?.result?.actions_executed && detail.result.actions_executed.length > 0 && (
                  <div>
                    <div className="flex items-center gap-1.5 mb-1.5 text-xs font-medium text-muted-foreground">
                      <Zap className="h-4 w-4" />
                      Actions Executed
                    </div>
                    <div className="space-y-1">
                      {detail.result.actions_executed.map((action: any, idx: number) => (
                        <div key={idx} className={cn(
                          "flex items-center justify-between text-sm p-2 rounded",
                          action.success ? "bg-success-light" : "bg-error-light"
                        )}>
                          <span>{action.description}</span>
                          {action.success ? (
                            <CheckCircle2 className="h-4 w-4 text-success" />
                          ) : (
                            <XCircle className="h-4 w-4 text-error" />
                          )}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </>
            ) : (
              <div className="text-center py-8 text-muted-foreground">No details available</div>
            )}
          </div>
        </DialogContent>
      </Dialog>

      {/* Fullscreen Image */}
      {fullscreenImage && (
        <Dialog open={!!fullscreenImage} onOpenChange={() => setFullscreenImage(null)}>
          <DialogContent className="max-w-4xl p-2 z-[110]">
            <DialogHeader className="sr-only">
              <DialogTitle>Fullscreen Image</DialogTitle>
              <DialogDescription>Image preview</DialogDescription>
            </DialogHeader>
            <img
              src={fullscreenImage}
              alt="Fullscreen"
              className="w-full h-auto max-h-[80vh] object-contain rounded-lg"
            />
          </DialogContent>
        </Dialog>
      )}
    </>
  )
}

// Data Flow Node - Single execution item with input data preview
interface FlowNodeProps {
  execution: AgentExecution
  isLatest?: boolean
  isRunning?: boolean
  onClick: () => void
  detail?: any
}

const FlowNode = memo(function FlowNode({ execution, isLatest, isRunning, onClick, detail }: FlowNodeProps) {
  const dp = useMemo(() =>
    detail?.decision_process ? normalizeDecisionProcess(detail.decision_process) : null,
    [detail?.decision_process]
  )
  const dataCollected = detail?.decision_process?.data_collected || []
  const images = useMemo(() => extractImagesFromData(dataCollected), [dataCollected])
  const metricTags = useMemo(() => extractMetricTags(dataCollected), [dataCollected])

  const formatTime = (timestamp: string | number) => {
    const date = typeof timestamp === 'number'
      ? new Date(timestamp * 1000)
      : new Date(timestamp)
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
  }

  const formatDuration = (ms: number) => {
    if (ms < 1000) return `${ms}ms`
    return `${(ms / 1000).toFixed(1)}s`
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
        "w-full text-left group relative rounded-lg border transition-all duration-200",
        "hover:shadow-md hover:border-border",
        isLatest ? "border-border bg-muted" : "border-border bg-bg-50",
        isRunning && "border-info bg-info-light"
      )}
    >
      {/* Header row */}
      <div className="px-3 py-2 border-b border-border">
        <div className="flex flex-wrap items-center gap-1.5">
          {execution.status === 'Running' ? (
            <Loader2 className="h-4 w-4 text-info shrink-0 animate-spin" />
          ) : execution.status === 'Completed' ? (
            <CheckCircle2 className="h-4 w-4 text-success shrink-0" />
          ) : (
            <XCircle className="h-4 w-4 text-error shrink-0" />
          )}

          <span className={cn(textNano, "font-mono text-muted-foreground")}>
            #{execution.id.slice(-6)}
          </span>

          {isLatest && (
            <Badge variant="default" className={cn(textMicro, "h-4 px-1")}>
              Latest
            </Badge>
          )}

          <Badge variant="outline" className={cn(badgeSize.micro, "h-4 px-1")}>
            {execution.trigger_type}
          </Badge>

          <span className={cn(textNano, "text-muted-foreground")}>{formatTime(execution.timestamp)}</span>
          {execution.duration_ms > 0 && (
            <span className={cn(textNano, "font-mono text-muted-foreground")}>{formatDuration(execution.duration_ms)}</span>
          )}
          <ChevronRight className="h-4 w-4 text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity ml-auto" />
        </div>
      </div>

      {/* Input Data Preview - Images and Tags */}
      {(images.length > 0 || metricTags.length > 0) && (
        <div className="px-3 py-2">
          {/* Images */}
          {images.length > 0 && (
            <div className="flex flex-wrap gap-1.5 mb-2">
              {images.slice(0, 4).map((img, idx) => (
                <img
                  key={idx}
                  src={img.image}
                  alt={img.source}
                  className="h-10 w-10 object-cover rounded border border-border"
                />
              ))}
              {images.length > 4 && (
                <div className="h-10 w-10 rounded border border-border bg-muted-50 flex items-center justify-center">
                  <span className={cn(textMicro, "text-muted-foreground")}>+{images.length - 4}</span>
                </div>
              )}
            </div>
          )}

          {/* Metric Tags */}
          {metricTags.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {metricTags.slice(0, 8).map((tag, idx) => (
                <Badge key={idx} variant="secondary" className={cn(badgeSize.micro, "h-4 px-1")}>
                  {tag}
                </Badge>
              ))}
              {metricTags.length > 8 && (
                <Badge variant="outline" className={cn(badgeSize.micro, "h-4 px-1")}>
                  +{metricTags.length - 8}
                </Badge>
              )}
            </div>
          )}
        </div>
      )}

      {/* Conclusion preview */}
      {dp?.conclusion && (
        <div className="px-3 py-2">
          <div className={cn(textMini, "text-foreground line-clamp-2")}><MarkdownMessage content={dp.conclusion} /></div>
        </div>
      )}

      {/* Error */}
      {execution.error && (
        <div className="px-3 py-2">
          <p className={cn(textNano, "text-error line-clamp-2")}>{execution.error}</p>
        </div>
      )}
    </button>
  )
})

// Main Component
export function AgentMonitorWidget({
  className,
  agentId,
  editMode = false,
}: AgentMonitorWidgetProps) {
  const { t } = useTranslation(['common', 'agents', 'dashboardComponents'])

  // State
  const [agent, setAgent] = useState<AiAgent | null>(null)
  const [loading, setLoading] = useState(true)
  const [agentNotFound, setAgentNotFound] = useState(false)
  const agentNotFoundRef = useRef(false)

  // Prevent loading flash: only show loading spinner when loading AND no agent data exists yet
  const agentShowLoading = loading && !agent
  const [executions, setExecutions] = useState<AgentExecution[]>([])
  const [executionDetails, setExecutionDetails] = useState<Record<string, any>>({})
  const [isExecuting, setIsExecuting] = useState(false)
  const [newExecutionId, setNewExecutionId] = useState<string | null>(null)
  const [currentStage, setCurrentStage] = useState<string | null>(null)
  const [stageLabel, setStageLabel] = useState<string | null>(null)
  const [thinkingSteps, setThinkingSteps] = useState<Array<{ step: number; description: string }>>([])

  // Messages state
  const [userMessages, setUserMessages] = useState<Array<{ id: string; content: string; timestamp: number }>>([])
  const [newMessage, setNewMessage] = useState('')
  const [sendingMessage, setSendingMessage] = useState(false)

  // Dialog state
  const [selectedExecution, setSelectedExecution] = useState<AgentExecution | null>(null)
  const [detailOpen, setDetailOpen] = useState(false)

  // Fetch agent data
  const loadAgent = useCallback(async () => {
    if (!agentId) {
      setAgent(null)
      setAgentNotFound(false)
      setLoading(false)
      return
    }

    try {
      const data = await api.getAgent(agentId)
      setAgent(data)
      agentNotFoundRef.current = false
      setAgentNotFound(false)
    } catch (error) {
      if (!agentNotFoundRef.current) {
        console.warn('Agent not found:', agentId)
      }
      agentNotFoundRef.current = true
      setAgent(null)
      setAgentNotFound(true)
    } finally {
      setLoading(false)
    }
  }, [agentId])

  // Fetch executions
  const loadExecutions = useCallback(async () => {
    if (!agentId || agentNotFoundRef.current) return
    try {
      const data = await api.getAgentExecutions(agentId, 20)
      setExecutions(data.executions || [])
    } catch (error) {
      if (!agentNotFoundRef.current) {
        console.warn('Failed to load executions:', agentId)
      }
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
      loadUserMessages()
    } catch (error) {
      console.error('Failed to send message:', error)
    } finally {
      setSendingMessage(false)
    }
  }, [agentId, newMessage, sendingMessage, loadUserMessages])

  // Initial load
  useEffect(() => {
    setNewExecutionId(null)
    setCurrentStage(null)
    setStageLabel(null)
    setThinkingSteps([])
    setAgent(null)
    setAgentNotFound(false)
    agentNotFoundRef.current = false
    setExecutions([])
    setExecutionDetails({})
    loadAgent().then(() => {
      if (!agentNotFoundRef.current) {
        loadExecutions()
        loadUserMessages()
      }
    })
  }, [agentId])

  // Load execution details for flow nodes (all visible executions)
  // Uses batch API to avoid N+1 calls
  const executionDetailsRef = useRef(executionDetails)
  executionDetailsRef.current = executionDetails

  useEffect(() => {
    if (!agentId) return
    const loaded = executionDetailsRef.current
    // Only load details for the latest 10 executions to reduce payload
    const toLoad = executions.slice(0, 10).filter(exec => !loaded[exec.id])
    if (toLoad.length === 0) return

    const ids = toLoad.map(exec => exec.id)
    api.batchGetExecutions(agentId, ids)
      .then(res => {
        if (res?.details) {
          setExecutionDetails(prev => ({ ...prev, ...res.details }))
        }
      })
      .catch(err => {
        console.warn('[AgentMonitor] batch get executions failed:', err)
      })
  }, [executions, agentId])

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
      switch (event.type) {
        case 'AgentExecutionStarted': {
          const startedData = (event as AgentExecutionStartedEvent).data
          if (startedData.agent_id === agentId) {
            setIsExecuting(true)
            setNewExecutionId(startedData.execution_id || null)
            setCurrentStage('collecting')
            setStageLabel('Collecting data')
            setThinkingSteps([])
            loadExecutions()
          }
          break
        }

        case 'AgentProgress': {
          const progressData = (event as AgentProgressEvent).data
          if (progressData.agent_id === agentId) {
            setCurrentStage(progressData.stage)
            setStageLabel(progressData.stage_label)
          }
          break
        }

        case 'AgentThinking': {
          const thinkingData = (event as AgentThinkingEvent).data
          if (thinkingData.agent_id === agentId) {
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
            setThinkingSteps(prev => [...prev, { step: prev.length + 1, description: `Decided: ${decisionData.action}` }])
          }
          break
        }

        case 'AgentExecutionCompleted': {
          const completedData = (event as AgentExecutionCompletedEvent).data
          if (completedData.agent_id === agentId) {
            setIsExecuting(false)
            setCurrentStage(null)
            setStageLabel(null)
            setThinkingSteps([])
            loadAgent()
            loadExecutions()
            setTimeout(() => setNewExecutionId(null), 3000)
          }
          break
        }
      }
    },
  })

  // Calculate stats
  const executionCount = agent?.execution_count || executions.length || 0
  const successCount = agent?.success_count || executions.filter(e => e.status === 'Completed').length || 0
  const avgDurationMs = agent?.avg_duration_ms || 0
  const successRate = executionCount > 0 ? Math.round((successCount / executionCount) * 100) : 0
  const currentlyExecuting = isExecuting || agent?.status === 'Executing'

  // Memoize sorted timeline items — prevents re-sorting on every render
  const sortedItems = useMemo(() => {
    const all: Array<{
      type: 'execution' | 'message'
      timestamp: number
      data: any
    }> = [
      ...executions.map(e => ({
        type: 'execution' as const,
        timestamp: typeof e.timestamp === 'number' ? e.timestamp : new Date(e.timestamp).getTime() / 1000,
        data: e
      })),
      ...userMessages.map(m => ({
        type: 'message' as const,
        timestamp: m.timestamp,
        data: m
      }))
    ].sort((a, b) => b.timestamp - a.timestamp)
    return all
  }, [executions, userMessages])

  // Empty state
  if (!agentId && !loading && !editMode) {
    return (
      <div className={cn(dashboardCardBase, "overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center p-6">
          <Bot className="h-12 w-12 opacity-20 text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground">
            {t('dashboardComponents:agentMonitorWidget.noAgentConfigured')}
          </p>
        </div>
      </div>
    )
  }

  // Loading state
  if (agentShowLoading && !editMode) {
    return (
      <div className={cn(dashboardCardBase, "overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <LoadingState size="lg" />
      </div>
    )
  }

  // Agent not found
  if (!agent && !editMode) {
    return (
      <div className={cn(dashboardCardBase, "overflow-hidden flex items-center justify-center min-h-[200px]", className)}>
        <div className="text-center">
          <AlertCircle className="h-12 w-12 opacity-20 text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground">{t('dashboardComponents:agentMonitorWidget.agentNotFound')}</p>
        </div>
      </div>
    )
  }

  const displayAgent = agent
  const promptText = displayAgent?.user_prompt || displayAgent?.description || ''

  return (
    <>
      <div className={cn(
        dashboardCardBase, "overflow-hidden flex flex-col w-full h-full",
        className
      )}>
        {/* Header: Agent Info */}
        <div className="shrink-0 px-4 py-3 border-b border-border">
          <div className="flex items-start gap-3">
            {/* Avatar */}
            <div className={cn(
              "w-10 h-10 rounded-lg flex items-center justify-center shrink-0",
              currentlyExecuting ? "bg-info-light" : "bg-muted"
            )}>
              {currentlyExecuting ? (
                <Loader2 className="h-5 w-5 text-info animate-spin" />
              ) : (
                <Bot className="h-5 w-5 text-primary" />
              )}
            </div>

            {/* Info */}
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-1">
                <h3 className={cn(indicatorFontWeight.title, 'truncate')}>{displayAgent?.name || 'Agent'}</h3>
                {currentlyExecuting ? (
                  <Badge variant="default" className={cn(textNano, "h-5 gap-0.5 px-1.5")}>
                    <Loader2 className="h-2.5 w-2.5 animate-spin" />
                    Running
                  </Badge>
                ) : displayAgent?.status === 'Active' ? (
                  <Badge variant="outline" className={cn(textNano, "h-5 text-success border-success-light")}>
                    <CheckCircle2 className="h-2.5 w-2.5 mr-0.5" />
                    Active
                  </Badge>
                ) : displayAgent?.status === 'Paused' ? (
                  <Badge variant="secondary" className={cn(textNano, "h-5")}>
                    <Pause className="h-2.5 w-2.5 mr-0.5" />
                    Paused
                  </Badge>
                ) : displayAgent?.status === 'Error' ? (
                  <Badge variant="destructive" className={cn(textNano, "h-5")}>Error</Badge>
                ) : null}
              </div>

              {/* Stats inline */}
              <div className={cn("flex items-center gap-3", textMini, "text-muted-foreground")}>
                <span className="flex items-center gap-1">
                  <Activity className="h-4 w-4" />
                  {executionCount} runs
                </span>
                <span className={cn(
                  "flex items-center gap-1",
                  successRate >= 80 ? "text-success" : successRate >= 50 ? "text-warning" : "text-error"
                )}>
                  <Target className="h-4 w-4" />
                  {successRate}% success
                </span>
                {avgDurationMs > 0 && (
                  <span className="flex items-center gap-1">
                    <Clock className="h-4 w-4" />
                    {avgDurationMs < 1000 ? `${avgDurationMs}ms` : `${(avgDurationMs / 1000).toFixed(1)}s`}
                  </span>
                )}
              </div>
            </div>
          </div>

          {/* User Prompt */}
          {promptText && (
            <div className="mt-3 p-2.5 rounded-lg bg-muted-30 border border-border">
              <div className="flex items-center gap-1.5 mb-1">
                <Target className="h-4 w-4 text-primary" />
                <span className={cn(textNano, "font-medium text-muted-foreground uppercase tracking-wide")}>Task Prompt</span>
              </div>
              <p className="text-xs text-foreground line-clamp-2">{promptText}</p>
            </div>
          )}
        </div>

        {/* Flow Stream: Executions + User Messages */}
        <div className="flex-1 min-h-0 overflow-hidden">
          <ScrollArea className="h-full">
            <div className="p-3 space-y-2">
              {/* Real-time execution progress */}
              {currentlyExecuting && (
                <div className="mb-2 p-3 rounded-lg bg-info-light border border-info">
                  <div className="flex items-center gap-2 mb-2">
                    <div className="w-2 h-2 rounded-full bg-info animate-pulse" />
                    <span className="text-xs font-medium text-info">
                      {cleanStageLabel(currentStage, stageLabel)}
                    </span>
                  </div>

                  {/* Progress bar */}
                  <div className="h-1 bg-info-light rounded-full overflow-hidden mb-2">
                    <div
                      className="h-full bg-info transition-all duration-500"
                      style={{
                        width: currentStage === 'collecting' ? '33%' :
                               currentStage === 'analyzing' ? '66%' : '90%'
                      }}
                    />
                  </div>

                  {/* Thinking steps */}
                  {thinkingSteps.length > 0 && (
                    <div className="space-y-1">
                      {thinkingSteps.slice(-3).map((step, idx) => {
                        // Clean description - remove emojis and use English for Chinese text
                        let desc = step.description.replace(/[\u{1F300}-\u{1F9FF}]|[\u{2600}-\u{26FF}]|[\u{2700}-\u{27BF}]/gu, '').trim()
                        if (/[\u4e00-\u9fff]/.test(desc)) {
                          // Chinese text - use generic label based on keywords
                          if (desc.includes('收集') || desc.includes('collect')) desc = 'Collecting data'
                          else if (desc.includes('分析') || desc.includes('analyze')) desc = 'Analyzing'
                          else if (desc.includes('决策') || desc.includes('decide')) desc = 'Making decision'
                          else desc = `Step ${step.step}`
                        }
                        return (
                          <div key={idx} className={cn("flex items-start gap-2", textMini, "text-info")}>
                            <span className={cn("font-mono", textNano, "text-info")}>#{step.step}</span>
                            <span className="line-clamp-1">{desc}</span>
                          </div>
                        )
                      })}
                    </div>
                  )}
                </div>
              )}

              {/* Data Flow: Interleaved messages and executions */}
              {(() => {
                if (sortedItems.length === 0) {
                  return (
                    <div className="flex flex-col items-center justify-center py-8 text-center">
                      <CircleDot className="h-8 w-8 text-muted-foreground opacity-30 mb-2" />
                      <p className="text-xs text-muted-foreground">No activity yet</p>
                    </div>
                  )
                }

                return sortedItems.map((item, idx) => {
                  if (item.type === 'message') {
                    const msg = item.data
                    return (
                      <div key={`msg-${msg.id}`} className="flex items-start gap-2">
                        <div className="w-6 h-6 rounded-full bg-accent-purple-light flex items-center justify-center shrink-0 mt-0.5">
                          <MessageSquare className="h-4 w-4 text-accent-purple" />
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className={cn(textNano, "text-muted-foreground mb-0.5")}>
                            User hint • {new Date(msg.timestamp * 1000).toLocaleString()}
                          </div>
                          <p className="text-xs bg-accent-purple-light rounded-lg px-2.5 py-1.5 border border-accent-purple-light">
                            {msg.content}
                          </p>
                        </div>
                      </div>
                    )
                  } else {
                    const exec = item.data
                    const isLatest = idx === 0 && exec.id === newExecutionId
                    const isRunning = exec.status === 'Running' && (idx === 0 || exec.id === newExecutionId)
                    const detail = executionDetails[exec.id]

                    return (
                      <FlowNode
                        key={`exec-${exec.id}`}
                        execution={exec}
                        isLatest={isLatest}
                        isRunning={isRunning}
                        onClick={() => {
                          setSelectedExecution(exec)
                          setDetailOpen(true)
                        }}
                        detail={detail}
                      />
                    )
                  }
                })
              })()}
            </div>
          </ScrollArea>
        </div>

        {/* Footer: User Input */}
        <div className="shrink-0 p-3 border-t border-border bg-muted-20">
          <div className="flex gap-2">
            <Textarea
              placeholder="Add task hints or context..."
              value={newMessage}
              onChange={(e) => setNewMessage(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault()
                  handleSendMessage()
                }
              }}
              className="min-h-[36px] max-h-[72px] h-9 text-xs placeholder:text-xs resize-none py-1.5"
              disabled={sendingMessage}
            />
            <Button
              size="sm"
              onClick={handleSendMessage}
              disabled={!newMessage.trim() || sendingMessage}
              className="h-9 px-3 shrink-0"
            >
              {sendingMessage ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <Send className="h-4 w-4" />
              )}
            </Button>
          </div>
        </div>
      </div>

      {/* Execution Detail Dialog */}
      {agentId && (
        <ExecutionDetailDialog
          execution={selectedExecution}
          open={detailOpen}
          onClose={() => setDetailOpen(false)}
          agentId={agentId}
        />
      )}
    </>
  )
}
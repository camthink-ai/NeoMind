import { useState, useCallback, useRef, useEffect } from 'react'
import { api } from '@/lib/api'
import { resolveImageSrc } from '@/lib/imageUtils'
import { useEvents } from '@/hooks/useEvents'
import { useVisiblePolling } from '@/hooks/useVisiblePolling'
import type { ResourceRequest } from '@/types'
import type { DataSource, DataSourceOrList } from '@/types/dashboard'
import { getSourceId, normalizeDataSource } from '@/types/dashboard'
import type { AiAnalystConfig, AnalystMessage } from './types'

interface UseAnalystSessionParams {
  componentId: string
  config: AiAnalystConfig
  dataSource?: DataSourceOrList
  /** Display title from dashboard component — used as agent name */
  title?: string
  onConfigUpdate: (updates: Partial<AiAnalystConfig>) => void
}

interface UseAnalystSessionReturn {
  messages: AnalystMessage[]
  isStreaming: boolean
  streamingContent: string
  streamingMsgId: string | null
  error: string | null
  initializing: boolean
  initSession: () => Promise<void>
  sendImage: (imageDataUrl: string, dataSource?: string) => void
  /** Add a non-image data event to timeline (shows a compact summary, not raw value) */
  sendData: (value: unknown, dataSource?: string) => void
  sendText: (text: string) => void
  isConnected: boolean
}

let msgCounter = 0
function nextId(): string {
  return `analyst-${Date.now()}-${++msgCounter}`
}

/** Maximum number of messages to keep in the timeline */
const MAX_MESSAGES = 40

/** Trim messages to MAX_MESSAGES, keeping the most recent */
function trimMessages(msgs: AnalystMessage[]): AnalystMessage[] {
  if (msgs.length <= MAX_MESSAGES) return msgs
  return msgs.slice(msgs.length - MAX_MESSAGES)
}

/** Internal/metadata fields to hide from data display */
const META_FIELDS = new Set([
  '_is_event_data', '_is_image', '_is_binary', '_source_type',
  'image_mime_type', 'mime_type',
  // Backend data_collected metadata fields for extension metrics
  'extension_id', 'timestamp', 'points_count', 'has_history',
])

/** Check if a field key is internal metadata that should be hidden */
function isMetaField(key: string): boolean {
  return META_FIELDS.has(key) || key.startsWith('_')
}

/** Find index of the last completed (non-streaming) AI/error message */
function findLastCompletedAi(msgs: AnalystMessage[]): number {
  for (let i = msgs.length - 1; i >= 0; i--) {
    const m = msgs[i]
    if ((m.type === 'ai' || m.type === 'error') && !m.isStreaming) return i
  }
  return -1
}

/** Build a compact display string from data values */
function summarizeData(value: unknown): string {
  if (value === null || value === undefined) return ''
  if (typeof value === 'number' || typeof value === 'boolean') return String(value)
  if (typeof value === 'string') {
    return (value as string).length > 100 ? (value as string).slice(0, 100) + '...' : value as string
  }
  if (typeof value === 'object') {
    try {
      if (Array.isArray(value)) {
        if (value.length > 0 && typeof value[0] === 'object' && value[0] !== null) {
          // Check if these are data_points with {timestamp, value} structure
          const first = value[0] as Record<string, unknown>
          if ('value' in first && 'timestamp' in first) {
            // Extract just the values, show latest first
            const vals = (value as Array<Record<string, unknown>>)
              .map(p => p.value)
              .filter(v => v !== undefined)
            if (vals.length === 0) return ''
            if (vals.length === 1) return summarizeData(vals[0])
            return vals.slice(0, 6).map(v => summarizeData(v)).join(', ') +
              (vals.length > 6 ? ` ... (+${vals.length - 6})` : '')
          }
          // Generic object array: show key: value pairs from first object
          // (e.g. [{cloud_cover: 100, temp: 23.8, ...}] → "cloud_cover: 100\ntemp: 23.8")
          const entries = Object.entries(first)
            .filter(([k]) => !isMetaField(k))
            .slice(0, 6)
          if (entries.length === 0) return ''
          const summary = entries.map(([k, v]) => {
            const vs = typeof v === 'object' && v !== null
              ? `(${Array.isArray(v) ? v.length + ' items' : Object.keys(v).length + ' keys'})`
              : String(v)
            return `${k}: ${vs.length > 30 ? vs.slice(0, 30) + '...' : vs}`
          }).join('\n')
          return value.length > 1 ? summary + `\n(${value.length} records)` : summary
        }
        // Primitive array
        const strs = value.map(v => String(v)).slice(0, 6)
        return strs.join(', ') + (value.length > 6 ? ` ... (+${value.length - 6})` : '')
      }
      // Object: show key: value summary, filtering out metadata fields
      const obj = value as Record<string, unknown>
      const entries = Object.entries(obj).filter(([k]) => !isMetaField(k)).slice(0, 6)
      if (entries.length === 0) return ''
      return entries.map(([k, v]) => {
        const vs = typeof v === 'object' && v !== null
          ? `(${Array.isArray(v) ? v.length + ' items' : Object.keys(v).length + ' keys'})`
          : String(v)
        return `${k}: ${vs.length > 30 ? vs.slice(0, 30) + '...' : vs}`
      }).join('\n')
    } catch {
      return String(value)
    }
  }
  return String(value)
}

/**
 * Resolve the metric field name from a DataSource.
 * Dashboard picker uses different property names depending on the source type:
 * - Device metrics → metricId (e.g., "values.image")
 * - Extension metrics → extensionMetric
 * - Others → property or fallback to "image"
 */
function resolveMetricField(dataSource: DataSource): string {
  const raw = dataSource.metricId
    || dataSource.extensionMetric
    || dataSource.property
    || dataSource.infoProperty
    || 'image'
  // Frontend data source picker uses "produce:" prefix for extension metrics
  // but backend stores/publishes raw metric names without this prefix
  return raw.startsWith('produce:') ? raw.slice(8) : raw
}

function buildResources(dataSources: DataSource[]): ResourceRequest[] | undefined {
  if (!dataSources.length) return undefined
  const resources: ResourceRequest[] = dataSources.flatMap((ds): ResourceRequest[] => {
    const sourceId = getSourceId(ds)
    const field = resolveMetricField(ds)

    // Device commands → same format as AgentEditorFullScreen
    if (ds.type === 'command' && sourceId && ds.command) {
      return [{
        resource_id: `${sourceId}:${ds.command}`,
        resource_type: 'command',
        name: ds.command,
        config: {
          device_id: sourceId,
          command_name: ds.command,
          parameters: ds.commandParams || {},
        },
      }]
    }

    // Extension commands → same format as AgentEditorFullScreen
    if (ds.type === 'extension-command' && ds.extensionId && ds.extensionCommand) {
      return [{
        resource_id: `extension:${ds.extensionId}:${ds.extensionCommand}`,
        resource_type: 'extension_tool',
        name: ds.extensionCommand,
        config: {
          extension_id: ds.extensionId,
          command_name: ds.extensionCommand,
          parameters: ds.commandParams || {},
        },
      }]
    }

    if (ds.type === 'extension' && ds.extensionId) {
      return [{
        resource_id: `extension:${ds.extensionId}:${field}`,
        resource_type: 'extension_metric',
        name: ds.extensionDisplayName || field,
        config: {
          extension_id: ds.extensionId,
          metric_name: field,
        },
      }]
    }
    if (sourceId) {
      return [{
        resource_id: `${sourceId}:${field}`,
        resource_type: ds.type === 'extension-metric' ? 'extension_metric' : 'metric',
        name: field,
        config: {
          device_id: sourceId,
          metric_name: field,
        },
      }]
    }
    return []
  })
  return resources.length > 0 ? resources : undefined
}

/**
 * Build event_filter JSON for schedule_type: 'event' so the agent editor
 * can display the Data-driven trigger sources.
 * Format: {"sources": [{"type": "device"|"extension", "id": "...", "name": "...", "field": "..."}]}
 */
function buildEventFilter(dataSources: DataSource[]): string | undefined {
  // Commands should not trigger agent execution — only metrics drive events
  const metricSources = dataSources.filter(ds =>
    ds.type !== 'command' && ds.type !== 'extension-command'
  )
  if (!metricSources.length) return undefined
  const sources = metricSources.flatMap((ds) => {
    const sourceId = getSourceId(ds)
    const field = resolveMetricField(ds)

    if (ds.type === 'extension' && ds.extensionId) {
      return [{
        type: 'extension',
        id: ds.extensionId,
        name: ds.extensionDisplayName || ds.extensionId,
        field,
      }]
    }
    if (sourceId) {
      return [{
        type: 'device',
        id: sourceId,
        name: sourceId,
        field,
      }]
    }
    return []
  })
  return sources.length > 0 ? JSON.stringify({ sources }) : undefined
}

/**
 * Load existing agent execution history and convert to AnalystMessage[].
 * Only loads the most recent executions that fit within contextWindowSize.
 */
async function loadHistoryMessages(
  agentId: string,
  contextWindowSize: number,
): Promise<AnalystMessage[]> {
  try {
    const resp = await api.getAgentExecutions(agentId, contextWindowSize)
    if (!resp?.executions?.length) return []

    // Executions come newest-first; reverse to chronological order
    const sorted = [...resp.executions].reverse()

    // Batch-fetch execution details to avoid N+1 API calls
    const nonRunning = sorted.filter(exec => exec.status !== 'Running')
    const ids = nonRunning.map(exec => exec.id)
    let batchDetails: Record<string, Awaited<ReturnType<typeof api.getExecution>>> = {}
    if (ids.length > 0) {
      try {
        const batchRes = await api.batchGetExecutions(agentId, ids)
        batchDetails = batchRes?.details || {}
      } catch {
        // Fall back to empty details
      }
    }

    const details: Array<{ exec: typeof sorted[number]; detail: Awaited<ReturnType<typeof api.getExecution>> } | null> = []
    for (const exec of nonRunning) {
      const detail = batchDetails[exec.id]
      if (detail) {
        details.push({ exec, detail })
      }
    }

    const messages: AnalystMessage[] = []
    for (const entry of details) {
      if (!entry) continue
      const { exec, detail } = entry

      const conclusion = detail?.decision_process?.conclusion
      if (!conclusion) continue

      const ts = new Date(exec.timestamp).getTime() || Date.now()

      // Restore ALL data_collected entries as ONE data message before AI response
      const dataCollected = detail?.decision_process?.data_collected
      if (dataCollected && dataCollected.length > 0) {
        const allImages: string[] = []
        const allLines: string[] = []
        for (const dc of dataCollected) {
          // Skip device_info (device metadata) and summary entries (agent memory
          // context: last_conclusion / recent_conclusions / total_executions —
          // prior-execution context for the LLM, not sensor data; must not leak
          // into the Data source bubble on history load).
          if (dc.data_type === 'device_info' || dc.data_type === 'summary' || dc.source === 'memory') continue

          const values = dc.values
          const dataType = dc.data_type

          if (values && typeof values === 'object' && !Array.isArray(values)) {
            const record = values as Record<string, unknown>

            // Detect backend data_collected format for extension metrics:
            // { extension_id, value, timestamp, points_count, has_history }
            // Or event-triggered data: { value, timestamp, _is_event_data }
            // Only the "value" field is meaningful data; everything else is metadata.
            const isStructuredData = ('extension_id' in record && 'value' in record)
              || ('value' in record && '_is_event_data' in record)
            if (isStructuredData) {
              const val = record.value
              const imgSrc = typeof val === 'string' ? resolveImageSrc(val) : null
              if (imgSrc) {
                allImages.push(imgSrc)
              } else {
                const s = summarizeData(val)
                if (s) allLines.push(dataType ? `${dataType}: ${s}` : s)
              }
              // Also check image_base64 field for raw base64 image data
              const b64 = record.image_base64
              if (typeof b64 === 'string' && b64.length > 100) {
                const cleanB64 = b64.replace(/[\s\r\n]+/g, '')
                const recordMime = record.image_mime_type || record.mime_type
                // If already a data URL, use as-is; otherwise add prefix
                const dataUrl = cleanB64.startsWith('data:') ? cleanB64
                  : typeof recordMime === 'string'
                    ? `data:${recordMime};base64,${cleanB64}`
                    : `data:image/png;base64,${cleanB64}`
                allImages.push(dataUrl)
              }
            } else {
              // Generic object: iterate entries, filter metadata
              for (const [key, val] of Object.entries(record)) {
                const imgSrc = typeof val === 'string' ? resolveImageSrc(val) : null
                if (imgSrc) {
                  allImages.push(imgSrc)
                } else if (!isMetaField(key)) {
                  const s = summarizeData(val)
                  if (s) allLines.push(`${key}: ${s}`)
                }
              }
            }
          } else {
            const s = summarizeData(values)
            if (s) allLines.push(dataType ? `${dataType}: ${s}` : s)
          }
        }
        if (allImages.length > 0 || allLines.length > 0) {
          messages.push({
            id: `hist-data-${exec.id}`,
            type: 'data',
            content: allLines.join('\n'),
            images: allImages.length > 0 ? allImages : undefined,
            timestamp: ts,
          })
        }
      }

      // AI response after data
      messages.push({
        id: `hist-${exec.id}`,
        type: exec.status === 'Failed' ? 'error' : 'ai',
        content: conclusion,
        timestamp: ts,
        duration: exec.duration_ms,
      })
    }

    // Load persisted user messages and interleave by timestamp
    try {
      const userMsgs = await api.getAgentUserMessages(agentId)
      if (userMsgs && userMsgs.length > 0) {
        for (const um of userMsgs) {
          messages.push({
            id: `hist-user-${um.id}`,
            type: 'user',
            content: um.content,
            // [unit fix] the API returns SECONDS (UserMessageDto ← Utc::now().
            // timestamp()); the renderer uses ms semantics — without ×1000
            // historical user hints rendered as Jan-1970 and sorted to the
            // timeline front. Same normalization AgentMonitorWidget applies.
            timestamp:
              typeof um.timestamp === 'number'
                ? um.timestamp * 1000
                : new Date(um.timestamp).getTime(),
          })
        }
        // Sort all messages by timestamp
        messages.sort((a, b) => a.timestamp - b.timestamp)
      }
    } catch { /* non-fatal */ }

    return trimMessages(messages)
  } catch {
    return []
  }
}

export function useAnalystSession({
  componentId,
  config,
  dataSource,
  title,
  onConfigUpdate,
}: UseAnalystSessionParams): UseAnalystSessionReturn {
  const [messages, setMessages] = useState<AnalystMessage[]>([])
  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingContent, setStreamingContent] = useState('')
  const [streamingMsgId, setStreamingMsgId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [initializing, setInitializing] = useState(false)
  const [isConnected, setIsConnected] = useState(false)

  const agentIdRef = useRef<string | null>(null)
  // Ref guard survives StrictMode unmount/remount — prevents double agent creation
  const initGuardRef = useRef(false)
  // Track mount state to avoid state updates after unmount
  const mountedRef = useRef(true)
  // Polling interval: while streaming, check execution status every 10 seconds.
  // This recovers from lost AgentExecutionCompleted events (WebSocket drops, race conditions, etc.)
  const streamingPollRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const isStreamingRef = useRef(false)
  const recoverFnRef = useRef<() => void>(() => {})

  useEffect(() => {
    return () => {
      mountedRef.current = false
      // Clean up streaming polling on unmount
      if (streamingPollRef.current) {
        clearInterval(streamingPollRef.current)
        streamingPollRef.current = null
      }
    }
  }, [])

  // Pause/resume streaming poll based on tab visibility
  useEffect(() => {
    const handleVisibility = () => {
      if (document.hidden) {
        // Pause polling while tab is hidden
        if (streamingPollRef.current) {
          clearInterval(streamingPollRef.current)
          streamingPollRef.current = null
        }
      } else if (isStreamingRef.current) {
        // Tab visible and still streaming — trigger immediate check and restart poll
        recoverFnRef.current()
        if (!streamingPollRef.current) {
          streamingPollRef.current = setInterval(() => recoverFnRef.current(), 5_000)
        }
      }
    }
    document.addEventListener('visibilitychange', handleVisibility)
    return () => document.removeEventListener('visibilitychange', handleVisibility)
  }, [])
  // Track whether history has been loaded for the current agent
  const historyLoadedRef = useRef(false)
  // Cross-path dedup: tracks ALL execution IDs added as AI messages (history + live).
  // Prevents the same execution from appearing twice with different ID prefixes (hist-X vs exec-X).
  const seenExecIdsRef = useRef<Set<string>>(new Set())

  /** Prune dedup set to prevent unbounded growth */
  const addSeenExecId = useCallback((id: string) => {
    const set = seenExecIdsRef.current
    set.add(id)
    if (set.size > 200) {
      const keys = Array.from(set)
      const half = Math.floor(keys.length / 2)
      for (let i = 0; i < half; i++) set.delete(keys[i])
    }
  }, [])

  // Stable refs for config values — avoids recreating initSession when config changes
  const configRef = useRef(config)
  configRef.current = config
  const dataSourceRef = useRef(dataSource)
  dataSourceRef.current = dataSource
  const titleRef = useRef(title)
  titleRef.current = title
  const onConfigUpdateRef = useRef(onConfigUpdate)
  onConfigUpdateRef.current = onConfigUpdate

  // Ref to track streaming msg ID for reliable cleanup in onExecutionCompleted
  const streamingMsgIdRef = useRef<string | null>(null)


  const recoverFromStuckStreaming = useCallback(() => {
    const agentId = agentIdRef.current
    if (!agentId) return

    api.getAgentExecutions(agentId, 1)
      .then((resp) => {
        const exec = resp?.executions?.[0]
        if (!exec || exec.status === 'Running') return
        // Execution completed but we missed the event — recover

        // Clear the polling interval
        if (streamingPollRef.current) {
          clearInterval(streamingPollRef.current)
          streamingPollRef.current = null
        }

        setIsStreaming(false)
        isStreamingRef.current = false
        setStreamingContent('')
        const placeholderId = streamingMsgIdRef.current
        setStreamingMsgId(null)
        streamingMsgIdRef.current = null

        api.getExecution(agentId, exec.id)
          .then((detail) => {
            const conclusion = detail?.decision_process?.conclusion
            // Cross-path dedup: skip if this execution was already added (history or live)
            if (seenExecIdsRef.current.has(exec.id)) {
              // Still need to clean up the streaming placeholder
              if (placeholderId) {
                setMessages((prev) => prev.filter((m) => m.id !== placeholderId))
              }
              return
            }
            addSeenExecId(exec.id)
            const dedupKey = `exec-${exec.id}`
            const aiMsg: AnalystMessage = {
              id: dedupKey,
              type: exec.status === 'Failed' ? 'error' : 'ai',
              content: conclusion || 'No result',
              timestamp: Date.now(),
              duration: exec.duration_ms,
            }
            setMessages((prev) => {
              // Skip if live event already added this execution
              if (prev.some((m) => m.id === dedupKey)) return prev
              const withoutStreaming = placeholderId
                ? prev.filter((m) => m.id !== placeholderId)
                : prev
              return trimMessages([...withoutStreaming, aiMsg])
            })
          })
          .catch(() => {
            setMessages((prev) => {
              const withoutStreaming = placeholderId
                ? prev.filter((m) => m.id !== placeholderId)
                : prev
              return trimMessages([...withoutStreaming, {
                id: `exec-recover-${exec.id}`,
                type: 'ai' as const,
                content: 'Analysis completed',
                timestamp: Date.now(),
              }])
            })
          })
      })
      .catch(() => {
        // Can't recover — clear stuck state
        if (streamingPollRef.current) {
          clearInterval(streamingPollRef.current)
          streamingPollRef.current = null
        }
        setIsStreaming(false)
        isStreamingRef.current = false
        setStreamingContent('')
        setStreamingMsgId(null)
        streamingMsgIdRef.current = null
      })
  }, [])
  recoverFnRef.current = recoverFromStuckStreaming

  // ---- Agent event listeners ----
  // Use useEvents directly (same pattern as AgentMonitorWidget) to avoid
  // the intermediate useAgentEvents layer which may miss events when
  // activeAgentId changes during the component lifecycle.
  const activeAgentId = config.agentId || agentIdRef.current || ''
  // Ref-based ID for reliable event filtering — avoids closure staleness where
  // AgentExecutionStarted matches but AgentExecutionCompleted is silently dropped
  // because the onEvent callback captured a stale activeAgentId from a prior render.
  const activeAgentIdRef = useRef(activeAgentId)
  activeAgentIdRef.current = activeAgentId

  useEvents({
    enabled: !!activeAgentId,
    onEvent: (event) => {
      // Filter events for our agent — use ref for reliable matching
      const data = event.data as { agent_id?: string }
      if (data.agent_id !== activeAgentIdRef.current) return

      switch (event.type) {
        case 'AgentExecutionStarted': {
          setIsStreaming(true)
          isStreamingRef.current = true
          const id = `stream-${Date.now()}`
          setStreamingMsgId(id)
          streamingMsgIdRef.current = id
          setStreamingContent('')
          const analyzingMsg: AnalystMessage = {
            id,
            type: 'ai',
            content: '',
            timestamp: Date.now(),
            isStreaming: true,
          }
          setMessages((prev) => trimMessages([...prev, analyzingMsg]))

          // Start polling — check execution status every 5 seconds while streaming.
          // This proactively recovers if AgentExecutionCompleted event is lost.
          if (streamingPollRef.current) clearInterval(streamingPollRef.current)
          streamingPollRef.current = setInterval(recoverFromStuckStreaming, 5_000)
          break
        }

        case 'AgentProgress': {
          const progressData = data as { stage: string; stage_label: string; details?: string }
          // Show stage progress in the streaming bubble (e.g. "Collecting data...",
          // "Analyzing 5 data points...", "Executing decisions...")
          const text = progressData.details || progressData.stage_label
          if (text) setStreamingContent(text)
          break
        }

        case 'AgentThinking': {
          const thinkingData = data as { description: string }
          // Thinking events give finer-grained step descriptions than progress
          // stages (e.g. "Collected device-temp: 5 data points", "Tool-calling
          // round 2"). Prefer the latest thinking description.
          if (thinkingData.description) setStreamingContent(thinkingData.description)
          break
        }

        case 'AgentExecutionCompleted': {
          // Stop the polling — event arrived successfully
          if (streamingPollRef.current) {
            clearInterval(streamingPollRef.current)
            streamingPollRef.current = null
          }

          // Stop the "Analyzing" badge immediately, but DON'T clear streamingContent/
          // streamingMsgId yet — the bubble still shows the last progress text while
          // we fetch the result. Clearing too early causes a blank-bubble gap between
          // Completed event and getExecution API response.
          setIsStreaming(false)
          isStreamingRef.current = false
          const placeholderId = streamingMsgIdRef.current

          const completedData = data as { agent_id: string; execution_id: string; success: boolean; duration_ms: number }
          // Cross-path dedup: skip if this execution was already added (history or recovery)
          if (seenExecIdsRef.current.has(completedData.execution_id)) {
            // Already have this execution — just remove streaming placeholder
            if (placeholderId) {
              setMessages((prev) => prev.filter((m) => m.id !== placeholderId))
            }
            setStreamingContent('')
            setStreamingMsgId(null)
            streamingMsgIdRef.current = null
            break
          }
          addSeenExecId(completedData.execution_id)
          // Dedup key — prevent recovery polling and live event from adding the same execution twice
          const dedupKey = `exec-${completedData.execution_id}`
          api.getExecution(completedData.agent_id, completedData.execution_id)
            .then((detail) => {
              const conclusion = detail?.decision_process?.conclusion
              const aiMsg: AnalystMessage = {
                id: dedupKey,
                type: completedData.success ? 'ai' : 'error',
                content: conclusion || 'No result',
                timestamp: Date.now(),
                duration: completedData.duration_ms,
              }
              setMessages((prev) => {
                // Skip if this execution was already added (e.g., by recovery polling)
                if (prev.some((m) => m.id === dedupKey)) return prev
                const withoutStreaming = placeholderId
                  ? prev.filter((m) => m.id !== placeholderId)
                  : prev
                return trimMessages([...withoutStreaming, aiMsg])
              })
              // NOW safe to clear streaming state — result is in the timeline
              setStreamingContent('')
              setStreamingMsgId(null)
              streamingMsgIdRef.current = null
            })
            .catch((err) => {
              const errMsg: AnalystMessage = {
                id: `exec-err-${completedData.execution_id}`,
                type: 'error',
                content: err instanceof Error ? err.message : 'Failed to fetch execution result',
                timestamp: Date.now(),
              }
              setMessages((prev) => {
                const withoutStreaming = placeholderId
                  ? prev.filter((m) => m.id !== placeholderId)
                  : prev
                return trimMessages([...withoutStreaming, errMsg])
              })
              setStreamingContent('')
              setStreamingMsgId(null)
              streamingMsgIdRef.current = null
            })
          break
        }
      }
    },
  })

  // Create agent (once per component lifecycle)
  // Uses refs for config/dataSource so the callback identity is stable —
  // this prevents the auto-init useEffect in AiAnalyst.tsx from re-triggering
  // when config props change (e.g., modelId, systemPrompt updates during editing).
  const initSession = useCallback(async () => {
    if (initGuardRef.current || agentIdRef.current) return

    try {
      initGuardRef.current = true
      setInitializing(true)
      setError(null)

      const cfg = configRef.current
      const ds = dataSourceRef.current
      const dsList = normalizeDataSource(ds)
      const resources = buildResources(dsList)
      const eventFilter = buildEventFilter(dsList)
      const agentName = `${titleRef.current || 'AI Analyst'} [${componentId.slice(0, 8)}]`

      // Search for existing agent by name to avoid duplicates.
      let agentId: string
      try {
        const list = await api.listAgents()
        const existing = list.agents?.find(a => a.name === agentName)
        if (existing) {
          // Reuse existing agent — update config to match current settings
          agentId = existing.id
          await api.updateAgent(agentId, {
            name: agentName,
            user_prompt: cfg.systemPrompt,
            llm_backend_id: cfg.modelId,
            schedule: { schedule_type: 'event', event_filter: eventFilter },
            resources,
            context_window_size: cfg.contextWindowSize,
          })
        } else {
          const agent = await api.createAgent({
            name: agentName,
            user_prompt: cfg.systemPrompt,
            llm_backend_id: cfg.modelId,
            schedule: { schedule_type: 'event', event_filter: eventFilter },
            execution_mode: 'focused',
            context_window_size: cfg.contextWindowSize,
            resources,
          })
          agentId = agent.id
        }
      } catch {
        // listAgents failed — fall through to create
        const agent = await api.createAgent({
          name: agentName,
          user_prompt: cfg.systemPrompt,
          llm_backend_id: cfg.modelId,
          schedule: { schedule_type: 'event', event_filter: eventFilter },
          execution_mode: 'focused',
          context_window_size: cfg.contextWindowSize,
          resources,
        })
        agentId = agent.id
      }

      agentIdRef.current = agentId
      setIsConnected(true)
      onConfigUpdateRef.current({ agentId })
      setInitializing(false)
    } catch (err) {
      initGuardRef.current = false
      setInitializing(false)
      setError(err instanceof Error ? err.message : 'Failed to create agent')
    }
  }, [componentId])

  // On mount: if config has a saved agentId, verify it still exists.
  // If the agent was deleted, clear the stale ID so a new agent will be created.
  useEffect(() => {
    const savedId = config.agentId
    if (!savedId) return

    let cancelled = false
    setInitializing(true)

    api.getAgent(savedId)
      .then(() => {
        if (cancelled) return
        agentIdRef.current = savedId
        setIsConnected(true)
        setInitializing(false)
      })
      .catch(() => {
        if (cancelled) return
        // Agent no longer exists — clear stale config and allow fresh creation
        agentIdRef.current = null
        initGuardRef.current = false
        onConfigUpdateRef.current({ agentId: undefined })
        setInitializing(false)
      })

    return () => { cancelled = true }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Sync llm_backend_id, user_prompt, and context_window_size when config changes after agent creation.
  // Also fires on initial connection (isConnected transition) to sync saved config.
  const prevModelIdRef = useRef<string | undefined>(undefined)
  const prevPromptRef = useRef<string | undefined>(undefined)
  const prevContextSizeRef = useRef<number | undefined>(undefined)
  useEffect(() => {
    const agentId = agentIdRef.current
    if (!agentId || !isConnected) return

    const updates: Record<string, unknown> = {}
    if (config.modelId && config.modelId !== prevModelIdRef.current) {
      updates.llm_backend_id = config.modelId
      prevModelIdRef.current = config.modelId
    }
    if (config.systemPrompt && config.systemPrompt !== prevPromptRef.current) {
      updates.user_prompt = config.systemPrompt
      prevPromptRef.current = config.systemPrompt
    }
    if (config.contextWindowSize && config.contextWindowSize !== prevContextSizeRef.current) {
      updates.context_window_size = config.contextWindowSize
      prevContextSizeRef.current = config.contextWindowSize
    }

    if (Object.keys(updates).length === 0) return
    // Defer update if agent is mid-execution to avoid model/prompt switch during tool loop
    if (isStreamingRef.current) {
      const check = setInterval(() => {
        if (!isStreamingRef.current) {
          clearInterval(check)
          api.updateAgent(agentId, updates).catch(() => {})
        }
      }, 1000)
      return () => clearInterval(check)
    }
    api.updateAgent(agentId, updates).catch(() => {
      // Non-critical: agent will use whatever configuration it has
    })
  }, [config.modelId, config.systemPrompt, config.contextWindowSize, isConnected])

  // Sync agent name when title changes after agent creation
  const prevTitleRef = useRef(title)
  useEffect(() => {
    const agentId = agentIdRef.current
    if (!agentId || !isConnected) return
    if (!title || title === prevTitleRef.current) return
    prevTitleRef.current = title
    const nameWithId = `${title} [${componentId.slice(0, 8)}]`
    api.updateAgent(agentId, { name: nameWithId }).catch(() => {})
  }, [title, isConnected])

  // Sync event_filter and resources when dataSource changes after agent creation
  // This handles component editing (changing data source) without creating a new agent
  const prevDataSourceRef = useRef(dataSource)
  useEffect(() => {
    const agentId = agentIdRef.current
    if (!agentId || !isConnected) return
    // Skip initial mount — agent was just created with this dataSource
    if (prevDataSourceRef.current === dataSource) return
    prevDataSourceRef.current = dataSource

    const dsList = normalizeDataSource(dataSource)
    const resources = buildResources(dsList)
    const eventFilter = buildEventFilter(dsList)

    api.updateAgent(agentId, {
      schedule: { schedule_type: 'event', event_filter: eventFilter },
      resources,
    }).catch(() => {
      // Non-critical: agent will use previous configuration
    })
  }, [dataSource, isConnected])

  // Load execution history when agent connects (verification or creation)
  useEffect(() => {
    const agentId = agentIdRef.current
    if (!isConnected || !agentId || historyLoadedRef.current) return

    historyLoadedRef.current = true
    loadHistoryMessages(agentId, config.contextWindowSize)
      .then((history) => {
        if (history.length > 0) {
          // Register history execution IDs for cross-path dedup
          for (const m of history) {
            if (m.id.startsWith('hist-') && !m.id.startsWith('hist-data-') && !m.id.startsWith('hist-user-')) {
              const execId = m.id.slice(5) // Remove 'hist-' prefix
              addSeenExecId(execId)
            }
          }
          // Merge: keep any live messages added while history was loading
          setMessages((prev) => {
            const liveMsgs = prev.filter(m => !m.id.startsWith('hist-'))
            if (liveMsgs.length === 0) return history
            return trimMessages([...history, ...liveMsgs])
          })
        }
      })
      .catch(() => {
        // History load failure is non-fatal
      })
  }, [isConnected, config.contextWindowSize])

  // Send image — merges into current round's data message (shared bubble with text).
  // If no data message exists yet, creates one with just the image.
  const sendImage = useCallback(
    (imageDataUrl: string, ds?: string) => {
      setMessages((prev) => {
        const lastCompletedAiIdx = findLastCompletedAi(prev)
        // Look for existing data message in current round to merge into
        for (let i = prev.length - 1; i > lastCompletedAiIdx; i--) {
          if (prev[i].type === 'data') {
            const existing = prev[i]
            return [
              ...prev.slice(0, i),
              { ...existing, images: [...(existing.images || []), imageDataUrl] },
              ...prev.slice(i + 1),
            ]
          }
        }
        // No data message in current round — create one with the image
        const newMsg: AnalystMessage = {
          id: nextId(),
          type: 'data',
          content: '',
          images: [imageDataUrl],
          timestamp: Date.now(),
          dataSource: ds,
        }
        const insertIdx = prev.findIndex((m, idx) => idx > lastCompletedAiIdx && (m.type === 'ai' || m.type === 'error'))
        if (insertIdx === -1) return [...prev, newMsg]
        return [...prev.slice(0, insertIdx), newMsg, ...prev.slice(insertIdx)]
      })
    },
    [],
  )

  // Send non-image data — merges into current round's data message (shared bubble with images).
  const sendData = useCallback(
    (value: unknown, ds?: string) => {
      const summary = summarizeData(value)
      // Skip if nothing meaningful to show after filtering metadata
      if (!summary) return

      setMessages((prev) => {
        const lastCompletedAiIdx = findLastCompletedAi(prev)

        // Look for a data message in the current round to merge into
        for (let i = prev.length - 1; i > lastCompletedAiIdx; i--) {
          if (prev[i].type === 'data') {
            const existing = prev[i]
            const newContent = existing.content ? existing.content + '\n' + summary : summary
            return [
              ...prev.slice(0, i),
              { ...existing, content: newContent, timestamp: Date.now(), dataSource: ds || existing.dataSource },
              ...prev.slice(i + 1),
            ]
          }
        }

        // No data message in current round — insert BEFORE any streaming AI message
        const insertIdx = prev.findIndex((m, idx) => idx > lastCompletedAiIdx && (m.type === 'ai' || m.type === 'error'))
        if (insertIdx === -1) {
          return [...prev, {
            id: nextId(),
            type: 'data' as const,
            content: summary,
            timestamp: Date.now(),
            dataSource: ds,
          }]
        }
        return [
          ...prev.slice(0, insertIdx),
          { id: nextId(), type: 'data' as const, content: summary, timestamp: Date.now(), dataSource: ds },
          ...prev.slice(insertIdx),
        ]
      })
    },
    [],
  )

  // Send text to agent via invoke (user-initiated, synchronous)
  const sendText = useCallback(
    (text: string) => {
      if (isStreamingRef.current) return // synchronous guard against concurrent invocations
      const agentId = agentIdRef.current
      if (!agentId) return

      const userMsg: AnalystMessage = {
        id: nextId(),
        type: 'user',
        content: text,
        timestamp: Date.now(),
      }
      setMessages((prev) => trimMessages([...prev, userMsg]))

      // Persist user message for history recovery
      api.addAgentUserMessage(agentId, text).catch(() => {})

      setIsStreaming(true)
      isStreamingRef.current = true
      const startTime = Date.now()

      api.invokeAgent(agentId, { input: text })
        .then((result) => {
          const duration = Date.now() - startTime
          const execId = result.execution_id as string | undefined

          // Register for WS event dedup
          if (execId) addSeenExecId(execId)

          // Stop streaming polling (WS AgentExecutionStarted may have started it)
          if (streamingPollRef.current) {
            clearInterval(streamingPollRef.current)
            streamingPollRef.current = null
          }

          // Clean up streaming placeholder (if WS AgentExecutionStarted created one)
          const placeholderId = streamingMsgIdRef.current
          streamingMsgIdRef.current = null
          setStreamingMsgId(null)

          // Use same ID format as WS handler for cross-path dedup
          const dedupKey = execId ? `exec-${execId}` : nextId()
          const aiMsg: AnalystMessage = {
            id: dedupKey,
            type: result.has_error ? 'error' : 'ai',
            content: result.conclusion || result.message || result.error || 'No result',
            timestamp: Date.now(),
            modelName: config.modelName,
            duration,
          }

          setMessages((prev) => {
            // Skip if WS event already added this execution
            if (prev.some((m) => m.id === dedupKey)) return prev
            const withoutStreaming = placeholderId
              ? prev.filter((m) => m.id !== placeholderId)
              : prev
            return trimMessages([...withoutStreaming, aiMsg])
          })
        })
        .catch((err) => {
          // Clean up WS state (polling + placeholder) for error/timeout cases
          if (streamingPollRef.current) {
            clearInterval(streamingPollRef.current)
            streamingPollRef.current = null
          }
          const placeholderId = streamingMsgIdRef.current
          streamingMsgIdRef.current = null
          setStreamingMsgId(null)

          // If WS already added AI message for this execution, skip error
          const errMsg: AnalystMessage = {
            id: nextId(),
            type: 'error',
            content: err instanceof Error ? err.message : 'Request failed',
            timestamp: Date.now(),
          }
          setMessages((prev) => {
            const withoutStreaming = placeholderId
              ? prev.filter((m) => m.id !== placeholderId)
              : prev
            // Avoid duplicate if WS event already added a completed AI message
            let lastAi = -1
            let lastUser = -1
            for (let i = withoutStreaming.length - 1; i >= 0; i--) {
              const m = withoutStreaming[i]
              if (lastAi === -1 && m.type === 'ai' && !m.isStreaming) lastAi = i
              if (lastUser === -1 && m.type === 'user') lastUser = i
              if (lastAi !== -1 && lastUser !== -1) break
            }
            if (lastAi > lastUser && lastAi !== -1) return withoutStreaming
            return trimMessages([...withoutStreaming, errMsg])
          })
        })
        .finally(() => {
          setIsStreaming(false)
          isStreamingRef.current = false
        })
    },
    [config.modelName],
  )

  return {
    messages,
    isStreaming,
    streamingContent,
    streamingMsgId,
    error,
    initializing,
    initSession,
    sendImage,
    sendData,
    sendText,
    isConnected,
  }
}

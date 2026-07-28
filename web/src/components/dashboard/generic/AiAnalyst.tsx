/**
 * AI Analyst — Main Dashboard Widget
 *
 * The top-level component that assembles the AI Analyst dashboard widget.
 * It integrates the header, timeline, input bar, and config panel,
 * binding together useAnalystSession and useDataSource.
 *
 * Backend handles agent execution via event triggers (schedule_type: 'event').
 * Frontend listens for WebSocket events to display results in the timeline.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  ScanEye,
  Loader2,
  Settings2,
  AlertCircle,
  Activity,
  Clock,
  MessageSquare,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { resolveImageSrc } from '@/lib/imageUtils'
import { useDataSource } from '@/hooks/useDataSource'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { dashboardCardBase } from '@/design-system/tokens/size'
import { textNano, textMini } from "@/design-system/tokens/typography"
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import { LoadingState } from '../shared'
import { AnalystTimeline } from './ai-analyst/AnalystTimeline'
import { AnalystInputBar } from './ai-analyst/AnalystInputBar'
import { AnalystConfigPanel } from './ai-analyst/AnalystConfigPanel'
import { useAnalystSession } from './ai-analyst/useAnalystSession'
import type { AiAnalystConfig } from './ai-analyst/types'
import { DEFAULT_SYSTEM_PROMPT } from './ai-analyst/types'

// ---------------------------------------------------------------------------
// Props
// ---------------------------------------------------------------------------

interface AiAnalystProps {
  className?: string
  editMode?: boolean
  agentId?: string
  sessionId?: string
  dataSource?: any
  /** Display title from dashboard component config */
  title?: string
  modelId?: string
  modelName?: string
  systemPrompt?: string
  contextWindowSize?: number
  /** Persist config changes back to dashboard component (survives refresh) */
  onConfigChange?: (config: Record<string, any>) => void
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export function AiAnalyst({
  className,
  editMode = false,
  title,
  agentId,
  sessionId: sessionIdProp,
  dataSource: dataSourceProp,
  modelId: modelIdProp,
  modelName: modelNameProp,
  systemPrompt: systemPromptProp,
  contextWindowSize: contextWindowSizeProp,
  onConfigChange,
}: AiAnalystProps) {
  const { t } = useTranslation('dashboardComponents')
  // Stable component ID — locked on first render so it doesn't change
  // when agentId is saved back as a prop (which would trigger cleanup and delete the agent)
  const componentIdRef = useRef<string | null>(null)
  if (!componentIdRef.current) {
    componentIdRef.current = agentId || sessionIdProp || `analyst-${crypto.randomUUID()}`
  }
  const componentId = componentIdRef.current

  // Config panel open state
  const [configOpen, setConfigOpen] = useState(false)

  /** Get a display label for the data source (handles both single and multi-source) */
  const dataSourceLabel = useMemo(() => {
    if (!dataSourceProp) return undefined
    const ds = dataSourceProp as any
    if (Array.isArray(ds)) {
      const labels = ds.map((s: any) =>
        s.extensionMetric?.replace('produce:', '')
        || s.metricId
        || s.property
        || s.extensionId
      ).filter(Boolean)
      return labels.length > 0 ? labels.join(', ') : undefined
    }
    return ds.extensionMetric?.replace('produce:', '')
      || ds.metricId
      || ds.property
      || ds.extensionId
      || ds.sourceId
  }, [dataSourceProp])

  // Config from props (persisted in dashboard store/localStorage), not Zustand memory
  const config: AiAnalystConfig = useMemo(
    () => ({
      agentId,
      modelId: modelIdProp,
      modelName: modelNameProp,
      systemPrompt: systemPromptProp || DEFAULT_SYSTEM_PROMPT,
      contextWindowSize: contextWindowSizeProp || 10,
    }),
    [agentId, modelIdProp, modelNameProp, systemPromptProp, contextWindowSizeProp],
  )

  // Persist config back to dashboard via onConfigChange (survives page refresh)
  const handleConfigUpdate = useCallback(
    (updates: Partial<AiAnalystConfig>) => {
      if (onConfigChange) {
        const newConfig: Record<string, any> = {}
        const pairs: [string, unknown][] = [
          ['agentId', updates.agentId ?? agentId],
          ['modelId', updates.modelId ?? modelIdProp],
          ['modelName', updates.modelName ?? modelNameProp],
          ['systemPrompt', updates.systemPrompt ?? systemPromptProp],
          ['contextWindowSize', updates.contextWindowSize ?? contextWindowSizeProp],
        ]
        for (const [key, val] of pairs) {
          if (val !== undefined && val !== null) newConfig[key] = val
        }
        onConfigChange(newConfig)
      }
    },
    [onConfigChange, modelIdProp, modelNameProp, systemPromptProp, contextWindowSizeProp, agentId],
  )

  const {
    messages,
    isStreaming,
    streamingContent,
    streamingMsgId,
    error: sessionError,
    initializing,
    initSession,
    sendImage,
    sendData,
    sendText,
    isConnected,
  } = useAnalystSession({
    componentId,
    config,
    dataSource: dataSourceProp,
    title,
    onConfigUpdate: handleConfigUpdate,
  })

  // ---- Data source binding ----
  // preserveMultiple keeps each data source separate (nested arrays) so
  // extractLatestValue can build key:value pairs per metric instead of
  // returning only the first metric's raw value.
  const { data: dsData } = useDataSource<string>(dataSourceProp, {
    preserveMultiple: Array.isArray(dataSourceProp) && dataSourceProp.length > 1,
  })

  // Show the latest data point in the timeline while LLM processes in background.
  // useDataSource returns different shapes depending on source type:
  //   - telemetry: DataPoint[] like [{timestamp, value}, ...]
  //   - device property: raw value (string, number, object)
  // We extract the latest value for display.
  const lastEnqueuedRef = useRef<string | null>(null)

  /** Extract the latest value from useDataSource output.
   *  Handles multiple shapes:
   *  - Single source data_points: [{timestamp, value}, ...] → latest .value
   *  - Multi-source array: [data_points_1, data_points_2, ...] → {metric_0: val, metric_1: val}
   *  - Raw scalar value (device property) → as-is
   */
  const extractLatestValue = useCallback(() => {
    if (dsData == null) return null

    // Multi-source: useDataSource returns [dataForSource1, dataForSource2, ...]
    // Each entry is either data_points[] or null
    if (Array.isArray(dsData) && dsData.length > 0 && Array.isArray(dsData[0])) {
      const summary: Record<string, unknown> = {}
      const dsList = Array.isArray(dataSourceProp) ? dataSourceProp : [dataSourceProp]
      dsData.forEach((sourceData: unknown, idx: number) => {
        const ds = dsList[idx] as any
        const label = ds?.extensionMetric?.replace('produce:', '')
          || ds?.metricId
          || ds?.property
          || `metric_${idx}`
        if (Array.isArray(sourceData) && sourceData.length > 0) {
          const latest = sourceData[0]
          summary[label] = latest && typeof latest === 'object' && 'value' in latest
            ? (latest as { value: unknown }).value
            : latest
        } else if (sourceData != null) {
          summary[label] = sourceData
        }
      })
      return Object.keys(summary).length > 0 ? summary : null
    }

    // Single source: data_points array
    if (Array.isArray(dsData) && dsData.length > 0) {
      const point = dsData[0]
      if (point && typeof point === 'object' && 'value' in point) {
        return (point as { value: unknown }).value
      }
      return point
    }

    // Raw scalar
    return dsData
  }, [dsData, dataSourceProp])

  // Send data to timeline during active execution rounds only.
  // Dedup is PERSISTENT across rounds — prevents the timing race where
  // AgentExecutionStarted WS event arrives BEFORE the telemetry update,
  // causing the stale previous-round image to be enqueued first and the
  // fresh image appended right after (resulting in two images per update).
  // Trade-off: if the same value repeats across rounds, it's only shown once
  // (which is desirable — repeated identical images are visual noise).
  useEffect(() => {
    if (editMode || dsData == null || !isStreaming) return

    const latestValue = extractLatestValue()
    if (latestValue == null) return

    const strVal = typeof latestValue === 'string' ? latestValue : JSON.stringify(latestValue)
    if (strVal === lastEnqueuedRef.current || strVal.length < 1) return
    lastEnqueuedRef.current = strVal

    const imgSrc = typeof latestValue === 'string' ? resolveImageSrc(latestValue) : null
    if (imgSrc) {
      sendImage(imgSrc, dataSourceLabel)
    } else {
      sendData(latestValue, dataSourceLabel)
    }
  }, [editMode, dsData, isStreaming, dataSourceProp, extractLatestValue, sendImage, sendData, dataSourceLabel])

  // ---- Auto-init agent when dataSource is set but no agentId ----
  const hasDataSource = dataSourceProp !== undefined && dataSourceProp !== null
  const hasAgent = !!config.agentId
  const initCalledRef = useRef(false)

  useEffect(() => {
    // Only auto-init in non-edit mode with a data source and no existing agent
    // Use ref guard to prevent double-call from StrictMode or state cascading
    if (!editMode && hasDataSource && !hasAgent && !sessionError && !initCalledRef.current) {
      initCalledRef.current = true
      initSession()
    }
  }, [editMode, hasDataSource, hasAgent, sessionError, initSession])

  // ---- Stats ----
  const aiMessages = useMemo(() => messages.filter((m) => m.type === 'ai'), [messages])
  const messageCount = messages.length
  const avgDuration = useMemo(() =>
    aiMessages.length > 0
      ? Math.round(
          aiMessages.reduce((sum, m) => sum + (m.duration ?? 0), 0) /
            aiMessages.length,
        )
      : 0,
    [aiMessages]
  )

  // ---- Render: Empty state ----
  if (!hasDataSource && !editMode) {
    return (
      <div
        className={cn(
          dashboardCardBase,
          'overflow-hidden flex items-center justify-center min-h-[200px]',
          className,
        )}
      >
        <div className="text-center p-6">
          <div className="w-12 h-12 rounded-lg bg-muted flex items-center justify-center mx-auto mb-3">
            <ScanEye className="h-6 w-6 text-primary" />
          </div>
          <p className="text-sm text-muted-foreground">
            {t('aiAnalyst.emptyState')}
          </p>
        </div>
      </div>
    )
  }

  // ---- Render: Initializing ----
  if (initializing && messageCount === 0) {
    return (
      <div
        className={cn(
          dashboardCardBase,
          'overflow-hidden flex items-center justify-center min-h-[200px]',
          className,
        )}
      >
        <LoadingState size="lg" />
      </div>
    )
  }

  // ---- Render: Error (no agent) ----
  if (sessionError && !hasAgent) {
    return (
      <div
        className={cn(
          dashboardCardBase,
          'overflow-hidden flex items-center justify-center min-h-[200px]',
          className,
        )}
      >
        <div className="text-center">
          <AlertCircle className="h-12 w-12 opacity-20 text-muted-foreground mx-auto mb-3" />
          <p className="text-sm text-muted-foreground mb-3">{sessionError}</p>
          <Button size="sm" variant="outline" onClick={() => initSession()}>
            {t('aiAnalyst.retry')}
          </Button>
        </div>
      </div>
    )
  }

  // ---- Render: Active layout ----
  return (
    <div
      className={cn(
        dashboardCardBase,
        'overflow-hidden flex flex-col w-full h-full',
        className,
      )}
    >
      {/* Header */}
      <div className="shrink-0 px-4 py-3 border-b border-border">
        <div className="flex items-start gap-3">
          {/* Avatar */}
          <div
            className={cn(
              'w-10 h-10 rounded-lg flex items-center justify-center shrink-0',
              isStreaming ? 'bg-info-light' : 'bg-muted',
            )}
          >
            {isStreaming ? (
              <Loader2 className="h-5 w-5 text-info animate-spin" />
            ) : (
              <ScanEye className="h-5 w-5 text-primary" />
            )}
          </div>

          {/* Info */}
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <h3 className={cn(indicatorFontWeight.title, 'truncate')}>{title || t('aiAnalyst.defaultTitle')}</h3>
              {isStreaming ? (
                <Badge
                  variant="default"
                  className={cn(textNano, "h-5 gap-0.5 px-1.5")}
                >
                  <Loader2 className="h-2.5 w-2.5 animate-spin" />
                  {t('aiAnalyst.analyzing')}
                </Badge>
              ) : isConnected ? (
                <Badge
                  variant="outline"
                  className={cn(textNano, "h-5 text-success border-success-light")}
                >
                  {t('aiAnalyst.live')}
                </Badge>
              ) : (
                <Badge variant="secondary" className={cn(textNano, "h-5")}>
                  {t('aiAnalyst.offline')}
                </Badge>
              )}
            </div>

            {/* Stats row */}
            <div className={cn("flex items-center gap-3", textMini, "text-muted-foreground")}>
              <span className="flex items-center gap-1">
                <MessageSquare className="h-4 w-4" />
                {t('aiAnalyst.messagesCount', { count: messageCount })}
              </span>
              {avgDuration > 0 && (
                <span className="flex items-center gap-1">
                  <Clock className="h-4 w-4" />
                  {t('aiAnalyst.avgDuration', { count: avgDuration })}
                </span>
              )}
              {config.modelName && (
                <span className="flex items-center gap-1">
                  <Activity className="h-4 w-4" />
                  {config.modelName}
                </span>
              )}
            </div>
          </div>

          {/* Settings button */}
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8 shrink-0"
            onClick={() => setConfigOpen(true)}
            aria-label={t('aiAnalyst.settingsAriaLabel')}
          >
            <Settings2 className="h-4 w-4" />
          </Button>
        </div>
      </div>
      <div className="flex-1 min-h-0 overflow-hidden">
        <AnalystTimeline
          messages={messages}
          streamingContent={streamingContent}
          streamingMsgId={streamingMsgId}
          contextWindowSize={config.contextWindowSize}
        />
      </div>

      {/* Footer: Input Bar */}
      <AnalystInputBar onSend={sendText} disabled={isStreaming || !isConnected} streaming={isStreaming} />

      {/* Config Panel */}
      <AnalystConfigPanel
        open={configOpen}
        onOpenChange={setConfigOpen}
        config={config}
        onSave={handleConfigUpdate}
        dataSource={dataSourceLabel}
      />
    </div>
  )
}

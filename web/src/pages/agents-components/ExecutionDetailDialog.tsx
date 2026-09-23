import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import {
  Clock,
  CheckCircle2,
  XCircle,
  AlertCircle,
  Sparkles,
  Image as ImageIcon,
  ChevronDown,
  ChevronUp,
} from "lucide-react"
import { api } from "@/lib/api"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { formatTimestamp } from "@/lib/utils/format"
import { cn } from "@/lib/utils"
import { MarkdownMessage } from "@/components/chat/MarkdownMessage"
import { textMicro, badgeSize, textNano } from "@/design-system/tokens/typography"
import type { AgentExecutionDetail, DataCollected } from "@/types"
import { FormSection, FormSectionGroup } from "@/components/ui/form-section"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"

// ============================================================================
// Pure helper functions (extracted from component for stable reference)
// ============================================================================

const isPureBase64 = (str: string): boolean => {
  if (!str || str.length < 100) return false
  const cleaned = str.trim()

  if (cleaned.startsWith('http://') || cleaned.startsWith('https://') || cleaned.startsWith('/')) {
    return false
  }
  if (cleaned.startsWith('data:')) {
    return false
  }

  const base64Regex = /^[A-Za-z0-9+/=_-]+$/
  if (!base64Regex.test(cleaned)) {
    return false
  }

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

  if (trimmed === '-' || trimmed === 'undefined' || trimmed === 'null' || trimmed === '') {
    return null
  }

  if (trimmed.startsWith('data:image/')) {
    return trimmed
  }

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

const extractImageData = (data: DataCollected) => {
  const values = data.values
  if (!values) return null

  if (Array.isArray(values)) {
    for (const item of values) {
      if (typeof item === 'object' && item !== null) {
        const obj = item as Record<string, unknown>
        for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src']) {
          const normalized = normalizeImageUrl(obj[key])
          if (normalized) {
            return { src: normalized, mimeType: obj.image_mime_type || obj.mimeType }
          }
        }
      }
    }
    for (const item of values) {
      if (typeof item === 'string') {
        const normalized = normalizeImageUrl(item)
        if (normalized) return { src: normalized, mimeType: null }
      }
    }
  }

  if (typeof values === 'object') {
    const obj = values as Record<string, unknown>
    for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src', 'value']) {
      const normalized = normalizeImageUrl(obj[key])
      if (normalized) {
        return { src: normalized, mimeType: obj.image_mime_type || obj.mimeType }
      }
    }
  }

  if (typeof values === 'string') {
    const normalized = normalizeImageUrl(values)
    if (normalized) return { src: normalized, mimeType: null }
  }

  return null
}

const formatValue = (value: unknown): string => {
  if (value === null || value === undefined) return '-'
  if (typeof value === 'string') return value
  if (typeof value === 'number') return String(value)
  if (typeof value === 'boolean') return value ? 'true' : 'false'
  if (Array.isArray(value)) return `[${value.length} items]`
  if (typeof value === 'object') {
    const str = JSON.stringify(value)
    return str.length > 100 ? str.slice(0, 100) + '...' : str
  }
  return String(value)
}

const getDataDisplayPairs = (data: DataCollected) => {
  const values = data.values
  const pairs: { key: string; value: string }[] = []

  if (!values) return pairs

  if (Array.isArray(values)) {
    values.forEach((item, idx) => {
      if (typeof item !== 'object' || item === null) {
        pairs.push({ key: `[${idx}]`, value: formatValue(item) })
      } else {
        const obj = item as Record<string, unknown>
        for (const [k, v] of Object.entries(obj)) {
          if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
            pairs.push({ key: k, value: formatValue(v) })
          }
        }
      }
    })
  } else if (typeof values === 'object') {
    const obj = values as Record<string, unknown>
    for (const [k, v] of Object.entries(obj)) {
      if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
        pairs.push({ key: k, value: formatValue(v) })
      }
    }
  } else {
    pairs.push({ key: 'value', value: formatValue(values) })
  }

  return pairs
}

interface ExecutionDetailDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  agentId: string
  executionId: string
}

export function ExecutionDetailDialog({
  open,
  onOpenChange,
  agentId,
  executionId,
}: ExecutionDetailDialogProps) {
  const { t } = useTranslation(['common', 'agents'])
  const { handleError } = useErrorHandler()

  const [execution, setExecution] = useState<AgentExecutionDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [expandedDataIndices, setExpandedDataIndices] = useState<Set<number>>(new Set())

  useEffect(() => {
    if (open && agentId && executionId) {
      loadExecution()
    }
  }, [open, agentId, executionId])

  const loadExecution = async () => {
    setLoading(true)
    try {
      const data = await api.getExecution(agentId, executionId)
      setExecution(data)
    } catch (error) {
      handleError(error, { operation: 'Load execution', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  const getStatusIcon = (status: string) => {
    switch (status) {
      case 'Completed':
        return <CheckCircle2 className="h-4 w-4 text-success" />
      case 'Failed':
        return <XCircle className="h-4 w-4 text-error" />
      case 'Running':
        return <Clock className="h-4 w-4 text-info" />
      default:
        return <AlertCircle className="h-4 w-4 text-muted-foreground" />
    }
  }

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

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('agents:execution.title', { defaultValue: 'Execution' }) + ' #' + executionId?.slice(-6)}
      icon={<Sparkles className="h-5 w-5 text-primary" />}
      width="xl"
      loading={loading && !execution}
      preventCloseOnSubmit={false}
      footer={
        <Button variant="outline" onClick={() => onOpenChange(false)}>
          {t('common:close')}
        </Button>
      }
    >
      {loading && !execution ? null : !execution ? (
        <div className="text-center py-12 text-muted-foreground">
          {t('agents:executionNotFound', { defaultValue: 'Execution not found' })}
        </div>
      ) : (
        <div className="space-y-3">
          {/* Status Bar */}
          <div className="flex items-center justify-between py-2 px-3 bg-muted-30 rounded-lg">
            <div className="flex items-center gap-2">
              {getStatusIcon(execution.status)}
              <span className="text-xs font-medium">{t(`agents:executionStatus.${execution.status.toLowerCase()}`)}</span>
            </div>
            <div className="flex items-center gap-3 text-xs text-muted-foreground">
              <span className="flex items-center gap-1">
                <Clock className="h-4 w-4" />
                {formatTimestamp(execution.timestamp, false)}
              </span>
              <span>{execution.duration_ms}ms</span>
            </div>
          </div>

          {execution.error && (
            <Card className="p-2 border-error bg-muted">
              <div className="flex items-start gap-1.5 text-error">
                <AlertCircle className="h-4 w-4 mt-0.5 shrink-0" />
                <div className="text-xs break-words">{execution.error}</div>
              </div>
            </Card>
          )}

          {execution.decision_process && (
            <FormSectionGroup>
              {/* Situation Analysis */}
              <FormSection
                title={t('agents:execution.analysis', { defaultValue: 'Situation Analysis' })}
                collapsible
              >
                <p className="text-xs leading-relaxed">{execution.decision_process.situation_analysis}</p>
              </FormSection>

              {/* Input Data */}
              {execution.decision_process.data_collected.length > 0 && (
                <FormSection
                  title={`${t('agents:execution.inputData', { defaultValue: 'Input Data' })} (${execution.decision_process.data_collected.length})`}
                  collapsible
                >
                  <div className="space-y-2">
                    {execution.decision_process.data_collected.map((data, idx) => {
                      const imageData = extractImageData(data)
                      const hasImage = imageData !== null
                      const dataPairs = getDataDisplayPairs(data)
                      const isExpanded = expandedDataIndices.has(idx)

                      return (
                        <div key={idx} className="border rounded-lg overflow-hidden">
                          <div
                            className="flex items-center justify-between p-2 bg-bg-50 cursor-pointer hover:bg-bg-80 transition-colors"
                            onClick={() => toggleDataExpanded(idx)}
                          >
                            <div className="flex items-center gap-2 min-w-0 flex-1">
                              {hasImage && <ImageIcon className="h-4 w-4 text-accent-purple shrink-0" />}
                              <span className={cn(textNano, "font-medium truncate")}>{data.source}</span>
                              <Badge variant="outline" className={cn(badgeSize.micro, "h-4 px-1 shrink-0")}>{data.data_type}</Badge>
                            </div>
                            {dataPairs.length > 0 && (
                              isExpanded ? (
                                <ChevronUp className="h-4 w-4 text-muted-foreground shrink-0" />
                              ) : (
                                <ChevronDown className="h-4 w-4 text-muted-foreground shrink-0" />
                              )
                            )}
                          </div>

                          {hasImage && (
                            <div className="p-2 bg-overlay-light">
                              <img
                                src={imageData!.src}
                                alt={`${data.source} - ${t('agents:execution.inputImage', { defaultValue: 'Input Image' })}`}
                                className="w-full max-h-[200px] object-contain rounded-md bg-background"
                              />
                            </div>
                          )}

                          {isExpanded && dataPairs.length > 0 && (
                            <div className="p-2 border-t">
                              <div className={cn("grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-x-3 gap-y-1", textNano)}>
                                {dataPairs.slice(0, 10).map((pair, pairIdx) => (
                                  <div key={pairIdx} className="flex items-baseline gap-1 min-w-0">
                                    <span className="text-muted-foreground shrink-0">{pair.key}:</span>
                                    <span className="truncate font-mono">{pair.value}</span>
                                  </div>
                                ))}
                                {dataPairs.length > 10 && (
                                  <div className={cn("col-span-2 text-muted-foreground", textMicro)}>
                                    {t('agents:execution.moreFields', { count: dataPairs.length - 10, defaultValue: `+${dataPairs.length - 10} more fields` })}
                                  </div>
                                )}
                              </div>
                            </div>
                          )}
                        </div>
                      )
                    })}
                  </div>
                </FormSection>
              )}

              {/* Reasoning Steps */}
              {execution.decision_process.reasoning_steps.length > 0 && (
                <FormSection
                  title={t('agents:execution.reasoningSteps', { defaultValue: 'Reasoning Steps' })}
                  collapsible
                >
                  <div className="space-y-2">
                    {execution.decision_process.reasoning_steps.map((step, idx, arr) => (
                      <div key={idx} className="flex gap-2">
                        <div className="flex flex-col items-center">
                          <div className={cn("w-5 h-5 rounded-full bg-muted text-primary", textNano, "flex items-center justify-center shrink-0")}>
                            {step.step_number}
                          </div>
                          {idx < arr.length - 1 && (
                            <div className="w-0.5 flex-1 bg-border my-0.5" />
                          )}
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="text-xs font-medium">{step.description}</div>
                          <div className="flex items-center gap-2 mt-1">
                            <Badge variant="outline" className={cn(badgeSize.micro, "h-4 px-1")}>{step.step_type}</Badge>
                            {/* Only when the step reported one. A missing
                                confidence is not a percentage, and rendering
                                `undefined * 100` would say "NaN%". */}
                            {step.confidence !== undefined && (
                              <span className={cn(textNano, "text-muted-foreground")}>
                                {Math.round(step.confidence * 100)}%
                              </span>
                            )}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </FormSection>
              )}

              {/* Decisions */}
              {execution.decision_process.decisions.length > 0 && (
                <FormSection
                  title={t('agents:execution.decisions', { defaultValue: 'Decisions' })}
                  collapsible
                >
                  <div className="space-y-1.5">
                    {execution.decision_process.decisions.map((decision, idx) => (
                      <div key={idx} className="p-2 bg-background rounded border">
                        <div className="text-xs font-medium mb-1">{decision.description}</div>
                        <div className="flex items-center justify-between">
                          <span className={cn(textNano, "text-muted-foreground truncate flex-1 mr-2")}>{decision.rationale}</span>
                          <Badge variant="secondary" className={cn(badgeSize.micro, "h-4 px-1 shrink-0")}>{decision.action}</Badge>
                        </div>
                      </div>
                    ))}
                  </div>
                </FormSection>
              )}

              {/* Conclusion */}
              <Card className="p-3 bg-muted border-border">
                <div className="text-xs font-semibold text-primary mb-2">{t('agents:execution.conclusionLabel', { defaultValue: 'Conclusion' })}</div>
                <MarkdownMessage content={execution.decision_process.conclusion} />
              </Card>

              {/* LLM Final Response */}
              {execution.result?.summary && (() => {
                const summary = execution.result.summary.trim()
                const conclusion = execution.decision_process?.conclusion?.trim() ?? ''
                const isGeneric = summary === 'Completed tool execution rounds.'
                  || summary === 'LLM generation failed during tool execution.'
                const normalize = (s: string) => s.replace(/\s+/g, ' ').trim()
                const isDuplicate = normalize(summary) === normalize(conclusion)
                  || (conclusion.length > 100 && normalize(summary).includes(normalize(conclusion).slice(0, 200)))
                  || (summary.length > 100 && normalize(conclusion).includes(normalize(summary).slice(0, 200)))
                if (!summary || isGeneric || isDuplicate) return null
                return (
                  <FormSection
                    title={t('agents:memory.llmResponse', 'LLM Response')}
                    collapsible
                  >
                    <pre className="text-xs whitespace-pre-wrap font-mono bg-muted-50 p-3 rounded-lg border max-h-60 overflow-auto break-words leading-relaxed">
                      {summary}
                    </pre>
                  </FormSection>
                )
              })()}

              {/* Actions Executed */}
              {execution.result?.actions_executed && execution.result.actions_executed.length > 0 && (
                <FormSection
                  title={t('agents:execution.actionsExecuted', { defaultValue: 'Actions Executed' })}
                  collapsible
                >
                  <div className="space-y-1">
                    {execution.result.actions_executed.map((action, idx) => (
                      <div key={idx} className="flex items-center justify-between p-1.5 bg-background rounded border">
                        <div className="flex-1 min-w-0 mr-2">
                          <div className="text-xs truncate">{action.description}</div>
                          <div className={cn(textNano, "text-muted-foreground truncate")}>{action.target}</div>
                        </div>
                        <Badge variant={action.success ? "default" : "destructive"} className={cn(badgeSize.micro, "h-4 px-1 shrink-0")}>
                          {action.success ? '✓' : '✗'}
                        </Badge>
                      </div>
                    ))}
                  </div>
                </FormSection>
              )}
            </FormSectionGroup>
          )}
        </div>
      )}
    </UnifiedFormDialog>
  )
}

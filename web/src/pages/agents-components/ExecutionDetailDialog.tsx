import { useState, useEffect, useMemo } from "react"
import { useTranslation } from "react-i18next"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Badge } from "@/components/ui/badge"
import { Card } from "@/components/ui/card"
import {
  Clock,
  CheckCircle2,
  XCircle,
  AlertCircle,
  Brain,
  Database,
  Play,
  FileText,
  ChevronRight,
  Sparkles,
  Image as ImageIcon,
  ChevronDown,
  ChevronUp,
  Monitor,
} from "lucide-react"
import { api } from "@/lib/api"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { formatTimestamp } from "@/lib/utils/format"
import type { AgentExecutionDetail, DataCollected, ReasoningStep, Decision } from "@/types"

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
        return <CheckCircle2 className="h-4 w-4 text-green-500" />
      case 'Failed':
        return <XCircle className="h-4 w-4 text-red-500" />
      case 'Running':
        return <Clock className="h-4 w-4 text-blue-500" />
      default:
        return <AlertCircle className="h-4 w-4 text-gray-500" />
    }
  }

  // State for expandable input data section
  const [expandedDataIndices, setExpandedDataIndices] = useState<Set<number>>(new Set())

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

  /**
   * Check if a string looks like pure base64 (no prefix)
   */
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

  /**
   * Detect image format from magic bytes
   */
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

  /**
   * Normalize image value to a displayable src
   */
  const normalizeImageUrl = (value: unknown): string | null => {
    if (!value) return null

    const valueStr = String(value)
    const trimmed = valueStr.trim()

    if (trimmed === '-' || trimmed === 'undefined' || trimmed === 'null' || trimmed === '') {
      return null
    }

    // Already a data URL
    if (trimmed.startsWith('data:image/')) {
      return trimmed
    }

    // Data URL without image/ prefix
    if (trimmed.startsWith('data:base64,')) {
      const base64Data = trimmed.slice(12)
      const formatInfo = detectImageFormat(base64Data) || { mime: 'image/png' }
      return `data:${formatInfo.mime};base64,${base64Data}`
    }

    // Pure base64
    if (isPureBase64(trimmed)) {
      const formatInfo = detectImageFormat(trimmed) || { mime: 'image/png' }
      return `data:${formatInfo.mime};base64,${trimmed}`
    }

    // HTTP/HTTPS URL or relative path
    return trimmed
  }

  /**
   * Extract image data from DataCollected values
   */
  const extractImageData = (data: DataCollected) => {
    const values = data.values

    if (!values) return null

    // Handle array values
    if (Array.isArray(values)) {
      for (const item of values) {
        if (typeof item === 'object' && item !== null) {
          const obj = item as Record<string, unknown>
          // Try various image field names
          for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src']) {
            const normalized = normalizeImageUrl(obj[key])
            if (normalized) {
              return { src: normalized, mimeType: obj.image_mime_type || obj.mimeType }
            }
          }
        }
      }
      // If array contains strings, try them
      for (const item of values) {
        if (typeof item === 'string') {
          const normalized = normalizeImageUrl(item)
          if (normalized) return { src: normalized, mimeType: null }
        }
      }
    }

    // Handle object values
    if (typeof values === 'object') {
      const obj = values as Record<string, unknown>
      for (const key of ['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src', 'value']) {
        const normalized = normalizeImageUrl(obj[key])
        if (normalized) {
          return { src: normalized, mimeType: obj.image_mime_type || obj.mimeType }
        }
      }
    }

    // Handle string values
    if (typeof values === 'string') {
      const normalized = normalizeImageUrl(values)
      if (normalized) return { src: normalized, mimeType: null }
    }

    return null
  }

  /**
   * Format data value for display
   */
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

  /**
   * Get displayable key-value pairs from data
   */
  const getDataDisplayPairs = (data: DataCollected) => {
    const values = data.values
    const pairs: { key: string; value: string }[] = []

    if (!values) return pairs

    if (Array.isArray(values)) {
      // For arrays, show index-based pairs
      values.forEach((item, idx) => {
        if (typeof item !== 'object' || item === null) {
          pairs.push({ key: `[${idx}]`, value: formatValue(item) })
        } else {
          const obj = item as Record<string, unknown>
          for (const [k, v] of Object.entries(obj)) {
            // Skip image fields as they're displayed separately
            if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
              pairs.push({ key: k, value: formatValue(v) })
            }
          }
        }
      })
    } else if (typeof values === 'object') {
      const obj = values as Record<string, unknown>
      for (const [k, v] of Object.entries(obj)) {
        // Skip image fields
        if (!['image_base64', 'imageBase64', 'base64', 'image_url', 'imageUrl', 'url', 'image', 'src'].includes(k)) {
          pairs.push({ key: k, value: formatValue(v) })
        }
      }
    } else {
      pairs.push({ key: 'value', value: formatValue(values) })
    }

    return pairs
  }

  if (!execution) {
    return null
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh]">
        <DialogHeader className="pb-2">
          <DialogTitle className="flex items-center gap-2 text-sm">
            <Sparkles className="h-4 w-4 text-primary" />
            Execution #{executionId.slice(-6)}
          </DialogTitle>
        </DialogHeader>

        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Clock className="h-5 w-5 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <ScrollArea className="max-h-[70vh] pr-3">
            <div className="space-y-3 pr-1">
              {/* Status Bar - Compact */}
              <div className="flex items-center justify-between py-2 px-3 bg-muted/30 rounded-lg">
                <div className="flex items-center gap-2">
                  {getStatusIcon(execution.status)}
                  <span className="text-xs font-medium">{t(`agents:executionStatus.${execution.status.toLowerCase()}`)}</span>
                </div>
                <div className="flex items-center gap-3 text-xs text-muted-foreground">
                  <span className="flex items-center gap-1">
                    <Clock className="h-3 w-3" />
                    {formatTimestamp(execution.timestamp, false)}
                  </span>
                  <span>{execution.duration_ms}ms</span>
                </div>
              </div>

              {execution.error && (
                <Card className="p-2 border-destructive/50 bg-destructive/5">
                  <div className="flex items-start gap-1.5 text-destructive">
                    <AlertCircle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
                    <div className="text-xs break-words">{execution.error}</div>
                  </div>
                </Card>
              )}

              {execution.decision_process && (
                <>
                  {/* Situation Analysis - Compact */}
                  <div className="p-2.5 bg-muted/20 rounded-lg border">
                    <div className="flex items-center gap-1.5 mb-1.5">
                      <Brain className="h-3.5 w-3.5 text-blue-500 shrink-0" />
                      <span className="text-xs font-semibold">{t('execution.analysis')}</span>
                    </div>
                    <p className="text-xs leading-relaxed">{execution.decision_process.situation_analysis}</p>
                  </div>

                  {/* Input Data - Detailed with Image Support */}
                  {execution.decision_process.data_collected.length > 0 && (
                    <div className="p-2.5 bg-muted/20 rounded-lg border">
                      <div className="flex items-center gap-1.5 mb-2">
                        <Monitor className="h-3.5 w-3.5 text-cyan-500 shrink-0" />
                        <span className="text-xs font-semibold">{t('execution.inputData')}</span>
                        <span className="text-[10px] text-muted-foreground">({execution.decision_process.data_collected.length})</span>
                      </div>
                      <div className="space-y-2">
                        {execution.decision_process.data_collected.map((data, idx) => {
                          const imageData = extractImageData(data)
                          const hasImage = imageData !== null
                          const dataPairs = getDataDisplayPairs(data)
                          const isExpanded = expandedDataIndices.has(idx)

                          return (
                            <div key={idx} className="border rounded-lg overflow-hidden">
                              {/* Header - clickable to expand/collapse */}
                              <div
                                className="flex items-center justify-between p-2 bg-background/50 cursor-pointer hover:bg-background/80 transition-colors"
                                onClick={() => toggleDataExpanded(idx)}
                              >
                                <div className="flex items-center gap-2 min-w-0 flex-1">
                                  {hasImage && <ImageIcon className="h-3 w-3 text-purple-500 shrink-0" />}
                                  <span className="text-[10px] font-medium truncate">{data.source}</span>
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
                                    alt={`${data.source} - ${t('execution.inputImage')}`}
                                    className="w-full max-h-[200px] object-contain rounded-md bg-background"
                                  />
                                </div>
                              )}

                              {/* Additional Data (expandable) */}
                              {isExpanded && dataPairs.length > 0 && (
                                <div className="p-2 border-t bg-background/30">
                                  <div className="grid grid-cols-2 gap-x-3 gap-y-1 text-[10px]">
                                    {dataPairs.slice(0, 10).map((pair, pairIdx) => (
                                      <div key={pairIdx} className="flex items-baseline gap-1 min-w-0">
                                        <span className="text-muted-foreground shrink-0">{pair.key}:</span>
                                        <span className="truncate font-mono">{pair.value}</span>
                                      </div>
                                    ))}
                                    {dataPairs.length > 10 && (
                                      <div className="col-span-2 text-muted-foreground text-[9px]">
                                        {t('execution.moreFields', { count: dataPairs.length - 10 })}
                                      </div>
                                    )}
                                  </div>
                                </div>
                              )}

                              {/* Expand hint for collapsed state */}
                              {!isExpanded && dataPairs.length > 0 && (
                                <div className="px-2 pb-1">
                                  <span className="text-[9px] text-muted-foreground">
                                    {t('execution.dataFieldsCount', { count: dataPairs.length })}
                                  </span>
                                </div>
                              )}
                            </div>
                          )
                        })}
                      </div>
                    </div>
                  )}

                  {/* Reasoning Steps - Compact Timeline */}
                  {execution.decision_process.reasoning_steps.length > 0 && (
                    <div className="p-2.5 bg-muted/20 rounded-lg border">
                      <div className="flex items-center gap-1.5 mb-2">
                        <Sparkles className="h-3.5 w-3.5 text-amber-500 shrink-0" />
                        <span className="text-xs font-semibold">{t('execution.reasoningSteps')}</span>
                      </div>
                      <div className="space-y-2">
                        {execution.decision_process.reasoning_steps.map((step, idx, arr) => (
                          <div key={idx} className="flex gap-2">
                            <div className="flex flex-col items-center">
                              <div className="w-5 h-5 rounded-full bg-primary/10 text-primary text-[10px] flex items-center justify-center shrink-0">
                                {step.step_number}
                              </div>
                              {idx < arr.length - 1 && (
                                <div className="w-0.5 flex-1 bg-border my-0.5" />
                              )}
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="text-xs font-medium">{step.description}</div>
                              <div className="flex items-center gap-2 mt-1">
                                <Badge variant="outline" className="text-[9px] h-4 px-1">{step.step_type}</Badge>
                                <span className="text-[10px] text-muted-foreground">
                                  {Math.round(step.confidence * 100)}%
                                </span>
                              </div>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Decisions - Compact */}
                  {execution.decision_process.decisions.length > 0 && (
                    <div className="p-2.5 bg-muted/20 rounded-lg border">
                      <div className="flex items-center gap-1.5 mb-2">
                        <Play className="h-3.5 w-3.5 text-green-500 shrink-0" />
                        <span className="text-xs font-semibold">{t('execution.decisions')}</span>
                      </div>
                      <div className="space-y-1.5">
                        {execution.decision_process.decisions.map((decision, idx) => (
                          <div key={idx} className="p-2 bg-background rounded border">
                            <div className="text-xs font-medium mb-1">{decision.description}</div>
                            <div className="flex items-center justify-between">
                              <span className="text-[10px] text-muted-foreground truncate flex-1 mr-2">{decision.rationale}</span>
                              <Badge variant="secondary" className="text-[9px] h-4 px-1 shrink-0">{decision.action}</Badge>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* Conclusion - Compact */}
                  <Card className="p-2 bg-primary/5 border-primary/20">
                    <div className="text-xs">
                      <span className="font-semibold text-primary">{t('execution.conclusionLabel')}</span>
                      <span className="ml-1">{execution.decision_process.conclusion}</span>
                    </div>
                  </Card>

                  {/* Actions Executed - Compact */}
                  {execution.result?.actions_executed && execution.result.actions_executed.length > 0 && (
                    <div className="p-2.5 bg-muted/20 rounded-lg border">
                      <div className="flex items-center gap-1.5 mb-2">
                        <Play className="h-3.5 w-3.5 text-green-500 shrink-0" />
                        <span className="text-xs font-semibold">{t('execution.actionsExecuted')}</span>
                      </div>
                      <div className="space-y-1">
                        {execution.result.actions_executed.map((action, idx) => (
                          <div key={idx} className="flex items-center justify-between p-1.5 bg-background rounded border">
                            <div className="flex-1 min-w-0 mr-2">
                              <div className="text-xs truncate">{action.description}</div>
                              <div className="text-[10px] text-muted-foreground truncate">{action.target}</div>
                            </div>
                            <Badge variant={action.success ? "default" : "destructive"} className="text-[9px] h-4 px-1 shrink-0">
                              {action.success ? '✓' : '✗'}
                            </Badge>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </>
              )}
            </div>
          </ScrollArea>
        )}

        <DialogFooter className="pt-2">
          <Button size="sm" onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

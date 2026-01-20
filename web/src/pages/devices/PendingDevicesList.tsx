import { useState, useEffect, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { EmptyStateInline, Pagination } from "@/components/shared"
import { Card } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Check, X, Eye, RefreshCw, Zap, Sparkles } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { useToast } from "@/hooks/use-toast"
import { useEvents } from "@/hooks/useEvents"
import { api } from "@/lib/api"
import type { DraftDevice, SuggestedDeviceType } from "@/types"

interface PendingDevicesListProps {
  onRefresh?: () => void
}

export function PendingDevicesList({ onRefresh }: PendingDevicesListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()

  const [drafts, setDrafts] = useState<DraftDevice[]>([])
  const [loading, setLoading] = useState(true)

  // Pagination state
  const [page, setPage] = useState(1)
  const itemsPerPage = 10

  const [selectedDraft, setSelectedDraft] = useState<DraftDevice | null>(null)
  const [processing, setProcessing] = useState<string | null>(null)

  // Approval dialog state
  const [showApproveDialog, setShowApproveDialog] = useState(false)
  const [selectedDraftForApproval, setSelectedDraftForApproval] = useState<DraftDevice | null>(null)
  const [suggestedTypes, setSuggestedTypes] = useState<SuggestedDeviceType[]>([])
  const [loadingSuggestions, setLoadingSuggestions] = useState(false)
  const [selectedTypeOption, setSelectedTypeOption] = useState<'existing' | 'new'>('existing')
  const [selectedExistingType, setSelectedExistingType] = useState<string>('')

  // New device type form state
  const [newTypeForm, setNewTypeForm] = useState({
    device_type: '',
    name: '',
    description: '',
    category: ''
  })

  // Filter out registered/rejected devices from main list (they should be in device list)
  const activeDrafts = drafts.filter(draft =>
    draft.status !== 'Registered' && draft.status !== 'Rejected'
  )
  const registeredCount = drafts.filter(d => d.status === 'Registered').length
  const rejectedCount = drafts.filter(d => d.status === 'Rejected').length

  // Paginated data
  const paginatedDrafts = activeDrafts.slice(
    (page - 1) * itemsPerPage,
    page * itemsPerPage
  )

  // Reset pagination when data changes
  useEffect(() => {
    setPage(1)
  }, [activeDrafts.length])

  // Fetch drafts
  const fetchDrafts = useCallback(async () => {
    setLoading(true)
    try {
      const response = await api.getDraftDevices()
      setDrafts(response.items || [])
    } catch (error) {
      console.error('Failed to fetch draft devices:', error)
      // Don't show error toast - endpoint might not be implemented yet
      setDrafts([])
    } finally {
      setLoading(false)
    }
  }, [])

  // Fetch type signatures for type reuse
  const fetchTypeSignatures = useCallback(async () => {
    // This is no longer needed - we use suggestDeviceTypes instead
  }, [])

  // Use WebSocket events for real-time updates
  const handleAutoOnboardEvent = useCallback((event: { type: string; data: unknown }) => {
    // Check if this is an auto_onboard event
    if (event.type === 'Custom' && (event.data as { event_type?: string })?.event_type === 'auto_onboard') {
      const eventData = event.data as { event?: string; device_id?: string }
      // Refresh drafts list when relevant auto_onboard events occur
      const eventType = eventData.event
      if (eventType === 'DraftCreated' ||
          eventType === 'SampleCollected' ||
          eventType === 'AnalysisStarted' ||
          eventType === 'AnalysisCompleted' ||
          eventType === 'DraftRegistered' ||
          eventType === 'DraftRejected') {
        fetchDrafts()
      }
    }
  }, [fetchDrafts])

  const { isConnected } = useEvents({
    enabled: true,
    eventTypes: ['Custom'],
    onEvent: handleAutoOnboardEvent,
  })

  // Initial fetch and fallback polling for connection issues
  useEffect(() => {
    fetchDrafts()
    fetchTypeSignatures()

    // Fallback polling only when not connected
    const interval = setInterval(() => {
      if (!isConnected) {
        fetchDrafts()
        fetchTypeSignatures()
      }
    }, 30000)

    return () => clearInterval(interval)
  }, [isConnected])

  // Approve draft device - open approval dialog with type suggestions
  const handleApproveClick = async (draft: DraftDevice) => {
    setSelectedDraftForApproval(draft)
    setShowApproveDialog(true)
    setLoadingSuggestions(true)
    setSuggestedTypes([])
    setSelectedExistingType('')
    setSelectedTypeOption('existing')

    // Initialize new type form from generated type
    if (draft.generated_type) {
      setNewTypeForm({
        device_type: draft.generated_type.device_type,
        name: draft.generated_type.name,
        description: draft.generated_type.description,
        category: draft.generated_type.category
      })
    }

    // Fetch suggested types
    try {
      const response = await api.suggestDeviceTypes(draft.device_id)
      setSuggestedTypes(response.suggestions || [])
      // Auto-select exact match if found
      if (response.exact_match) {
        setSelectedExistingType(response.exact_match)
      }
    } catch (error) {
      console.error('Failed to fetch suggested types:', error)
      setSuggestedTypes([])
    } finally {
      setLoadingSuggestions(false)
    }
  }

  // Handle final approval after type selection
  const handleFinalApprove = async () => {
    if (!selectedDraftForApproval) return

    setProcessing(selectedDraftForApproval.id)
    try {
      let result
      if (selectedTypeOption === 'existing' && selectedExistingType) {
        // Use existing type
        result = await api.approveDraftDeviceWithType(selectedDraftForApproval.device_id, selectedExistingType)
      } else {
        // Create new type first, then approve
        // For new type, we don't pass existing_type, so backend will create it
        result = await api.approveDraftDeviceWithType(selectedDraftForApproval.device_id, undefined)
      }

      // Show approval result with system device_id and recommended topic
      toast({
        title: t('common:success'),
        description: (
          <div className="space-y-1">
            <p>{result.message}</p>
            <div className="text-xs bg-muted p-2 rounded space-y-1">
              <p><span className="font-semibold">Original ID:</span> {result.original_device_id}</p>
              <p><span className="font-semibold">System ID:</span> {result.system_device_id}</p>
              <p><span className="font-semibold">Device Type:</span> {result.device_type}</p>
              <p><span className="font-semibold">Recommended Topic:</span> <code className="bg-background px-1 rounded">{result.recommended_topic}</code></p>
            </div>
          </div>
        ),
      })
      setShowApproveDialog(false)
      await fetchDrafts()
      onRefresh?.()
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.approveFailed'),
        variant: "destructive"
      })
    } finally {
      setProcessing(null)
      setSelectedDraftForApproval(null)
      setSelectedExistingType('')
    }
  }

  // Reject draft device
  const handleReject = async (draft: DraftDevice) => {
    const reason = prompt(t('devices:pending.rejectReason'))
    if (!reason) return

    setProcessing(draft.id)
    try {
      await api.rejectDraftDevice(draft.device_id, { reason })
      toast({
        title: t('common:success'),
        description: t('devices:pending.rejected', { deviceId: draft.device_id }),
      })
      await fetchDrafts()
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.rejectFailed'),
        variant: "destructive"
      })
    } finally {
      setProcessing(null)
    }
  }

  // Trigger manual analysis
  const handleAnalyze = async (draft: DraftDevice) => {
    setProcessing(draft.id)
    try {
      await api.triggerDraftAnalysis(draft.device_id)
      toast({
        title: t('common:success'),
        description: t('devices:pending.analysisTriggered'),
      })
      await fetchDrafts()
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.analysisFailed'),
        variant: "destructive"
      })
    } finally {
      setProcessing(null)
    }
  }

  // Enhance draft with LLM (Chinese names, descriptions, units)
  const handleEnhance = async (draft: DraftDevice) => {
    setProcessing(draft.id)
    try {
      await api.enhanceDraftWithLLM(draft.device_id)
      toast({
        title: t('common:success'),
        description: t('devices:pending.enhanceTriggered'),
      })
      // Poll for updates since enhancement happens in background
      setTimeout(() => fetchDrafts(), 2000)
      setTimeout(() => fetchDrafts(), 5000)
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.enhanceFailed'),
        variant: "destructive"
      })
    } finally {
      setProcessing(null)
    }
  }

  // Normalize status string for consistent comparison
  const normalizeStatus = (status: string): string => {
    return status.toLowerCase().replace(/[^a-z]/g, '_')
  }

  // Check if draft can be approved
  const canApprove = (draft: DraftDevice): boolean => {
    const normalized = normalizeStatus(draft.status)
    return normalized === 'waiting_processing' && !!draft.generated_type
  }

  // Check if draft can be rejected
  const canReject = (draft: DraftDevice): boolean => {
    const normalized = normalizeStatus(draft.status)
    return normalized === 'waiting_processing' || normalized === 'collecting' || normalized === 'analyzing'
  }

  // Check if draft can be analyzed
  const canAnalyze = (draft: DraftDevice): boolean => {
    const normalized = normalizeStatus(draft.status)
    return normalized === 'collecting' || normalized === 'failed'
  }

  // Check if draft can be enhanced with LLM
  const canEnhance = (draft: DraftDevice): boolean => {
    return !!draft.generated_type && normalizeStatus(draft.status) === 'waiting_processing'
  }

  // Get status badge
  const getStatusBadge = (status: string) => {
    const statusMap: Record<string, { color: string; label: string }> = {
      collecting: { color: "bg-blue-500/20 text-blue-700", label: t('devices:pending.status.collecting') },
      analyzing: { color: "bg-purple-500/20 text-purple-700", label: t('devices:pending.status.analyzing') },
      waiting_processing: { color: "bg-yellow-500/20 text-yellow-700", label: t('devices:pending.status.waitingProcessing') },
      registered: { color: "bg-green-500/20 text-green-700", label: t('devices:pending.status.registered') },
      rejected: { color: "bg-red-500/20 text-red-700", label: t('devices:pending.status.rejected') },
      failed: { color: "bg-red-500/20 text-red-700", label: t('devices:pending.status.failed') },
    }

    const key = normalizeStatus(status)
    const info = statusMap[key] || { color: "bg-gray-500/20 text-gray-700", label: status }

    return (
      <Badge className={cn("font-normal", info.color)}>
        {info.label}
      </Badge>
    )
  }

  // Get confidence badge
  const getConfidenceBadge = (confidence?: number) => {
    if (!confidence) return null

    if (confidence >= 0.85) {
      return <Badge variant="default" className="bg-green-500/20 text-green-700">
        {t('devices:pending.confidence.high')} ({Math.round(confidence * 100)}%)
      </Badge>
    } else if (confidence >= 0.6) {
      return <Badge variant="default" className="bg-yellow-500/20 text-yellow-700">
        {t('devices:pending.confidence.medium')} ({Math.round(confidence * 100)}%)
      </Badge>
    } else {
      return <Badge variant="default" className="bg-red-500/20 text-red-700">
        {t('devices:pending.confidence.low')} ({Math.round(confidence * 100)}%)
      </Badge>
    }
  }

  return (
    <>
      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('devices:pending.headers.deviceId')}</TableHead>
              <TableHead>{t('devices:pending.headers.source')}</TableHead>
              <TableHead>{t('devices:pending.headers.status')}</TableHead>
              <TableHead>{t('devices:pending.headers.samples')}</TableHead>
              <TableHead>{t('devices:pending.headers.confidence')}</TableHead>
              <TableHead>{t('devices:pending.headers.category')}</TableHead>
              <TableHead className="text-right">{t('devices:pending.headers.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <EmptyStateInline title={t('common:loading')} colSpan={7} />
            ) : activeDrafts.length === 0 ? (
              <EmptyStateInline
                title={t('devices:pending.noPending')}
                colSpan={7}
              />
            ) : (
              paginatedDrafts.map((draft) => (
                <TableRow key={draft.id}>
                  <TableCell className="font-mono text-xs">
                    {draft.device_id}
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="text-xs">
                      {draft.source}
                    </Badge>
                  </TableCell>
                  <TableCell>{getStatusBadge(draft.status)}</TableCell>
                  <TableCell>
                    {draft.sample_count} / {draft.max_samples}
                  </TableCell>
                  <TableCell>
                    {getConfidenceBadge(draft.generated_type?.confidence)}
                  </TableCell>
                  <TableCell>
                    {draft.generated_type?.category || "-"}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex justify-end gap-1">
                      {/* View details */}
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={() => setSelectedDraft(draft)}
                        title={t('devices:pending.viewDetails')}
                      >
                        <Eye className="h-4 w-4" />
                      </Button>

                      {/* Trigger analysis - for collecting or failed status */}
                      {canAnalyze(draft) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8"
                          onClick={() => handleAnalyze(draft)}
                          disabled={processing === draft.id}
                          title={t('devices:pending.triggerAnalysis')}
                        >
                          <Zap className="h-4 w-4" />
                        </Button>
                      )}

                      {/* LLM Enhance - for waiting_processing with generated_type */}
                      {canEnhance(draft) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-purple-600 hover:text-purple-700 hover:bg-purple-50"
                          onClick={() => handleEnhance(draft)}
                          disabled={processing === draft.id}
                          title={t('devices:pending.enhanceWithLLM')}
                        >
                          <Sparkles className="h-4 w-4" />
                        </Button>
                      )}

                      {/* Refresh - for any status */}
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-8 w-8"
                        onClick={() => fetchDrafts()}
                        title={t('common:refresh')}
                      >
                        <RefreshCw className="h-4 w-4" />
                      </Button>

                      {/* Approve - for waiting_processing with generated_type */}
                      {canApprove(draft) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-green-600 hover:text-green-700 hover:bg-green-50"
                          onClick={() => handleApproveClick(draft)}
                          disabled={processing === draft.id}
                          title={t('devices:pending.approve')}
                        >
                          <Check className="h-4 w-4" />
                        </Button>
                      )}

                      {/* Reject - for waiting_processing, collecting, or analyzing */}
                      {canReject(draft) && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-8 w-8 text-red-600 hover:text-red-700 hover:bg-red-50"
                          onClick={() => handleReject(draft)}
                          disabled={processing === draft.id}
                          title={t('devices:pending.reject')}
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      )}
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      {activeDrafts.length > itemsPerPage && (
        <div className="pt-4">
          <Pagination
            total={activeDrafts.length}
            pageSize={itemsPerPage}
            currentPage={page}
            onPageChange={setPage}
          />
        </div>
      )}

      {/* Summary footer showing registered/rejected counts */}
      {(registeredCount > 0 || rejectedCount > 0) && (
        <div className="mt-4 flex items-center justify-center gap-4 text-sm text-muted-foreground">
          {registeredCount > 0 && (
            <span className="flex items-center gap-1">
              <Badge variant="outline" className="bg-green-500/10 text-green-700">
                {registeredCount}
              </Badge>
              <span>{t('devices:pending.registeredHidden')}</span>
            </span>
          )}
          {rejectedCount > 0 && (
            <span className="flex items-center gap-1">
              <Badge variant="outline" className="bg-red-500/10 text-red-700">
                {rejectedCount}
              </Badge>
              <span>{t('devices:pending.rejectedHidden')}</span>
            </span>
          )}
        </div>
      )}

      {/* Approve Draft Dialog - Two-step flow */}
      {showApproveDialog && selectedDraftForApproval && (
        <Dialog open={showApproveDialog} onOpenChange={setShowApproveDialog}>
          <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
            <DialogHeader>
              <DialogTitle>{t('devices:pending.approveTitle')}</DialogTitle>
              <DialogDescription>
                {t('devices:pending.approveDesc')}
              </DialogDescription>
            </DialogHeader>

            <div className="space-y-6 py-4">
              {/* Device info */}
              <div className="flex items-center gap-4 p-4 bg-muted/50 rounded-lg">
                <div>
                  <p className="text-sm text-muted-foreground">{t('devices:pending.headers.deviceId')}</p>
                  <p className="font-mono font-medium">{selectedDraftForApproval.device_id}</p>
                </div>
                {selectedDraftForApproval.generated_type && (
                  <div>
                    <p className="text-sm text-muted-foreground">{t('devices:pending.headers.source')}</p>
                    <p className="font-medium">{selectedDraftForApproval.source}</p>
                  </div>
                )}
              </div>

              {/* Discovered metrics preview */}
              {selectedDraftForApproval.generated_type && (
                <div>
                  <h3 className="text-sm font-medium mb-2">{t('devices:pending.metrics')} ({selectedDraftForApproval.generated_type.metrics.length})</h3>
                  <div className="flex flex-wrap gap-2">
                    {selectedDraftForApproval.generated_type.metrics.slice(0, 6).map((m, i) => (
                      <Badge key={i} variant="secondary" className="text-xs">
                        {m.display_name || m.name}
                      </Badge>
                    ))}
                    {selectedDraftForApproval.generated_type.metrics.length > 6 && (
                      <Badge variant="secondary" className="text-xs">
                        +{selectedDraftForApproval.generated_type.metrics.length - 6}
                      </Badge>
                    )}
                  </div>
                </div>
              )}

              {/* Type selection */}
              <div className="space-y-4">
                <h3 className="text-sm font-medium">{t('devices:pending.selectExistingType')}</h3>

                {/* Toggle buttons for existing vs new */}
                <div className="flex gap-2 p-1 bg-muted rounded-lg">
                  <button
                    onClick={() => setSelectedTypeOption('existing')}
                    className={cn(
                      "flex-1 py-2 px-4 rounded-md text-sm font-medium transition-colors",
                      selectedTypeOption === 'existing'
                        ? "bg-background text-foreground shadow-sm"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {t('devices:pending.selectExistingType')}
                  </button>
                  <button
                    onClick={() => setSelectedTypeOption('new')}
                    className={cn(
                      "flex-1 py-2 px-4 rounded-md text-sm font-medium transition-colors",
                      selectedTypeOption === 'new'
                        ? "bg-background text-foreground shadow-sm"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {t('devices:pending.createNewType')}
                  </button>
                </div>

                {/* Option 1: Use existing type */}
                {selectedTypeOption === 'existing' && (
                  <div className="space-y-3">
                    {loadingSuggestions ? (
                      <div className="text-sm text-muted-foreground py-2">
                        {t('common:loading')}...
                      </div>
                    ) : suggestedTypes.length > 0 ? (
                      <div className="space-y-2">
                        {suggestedTypes.slice(0, 5).map((type) => (
                          <div
                            key={type.device_type}
                            className={cn(
                              "p-3 border rounded-lg cursor-pointer transition-colors",
                              selectedExistingType === type.device_type
                                ? "border-primary bg-primary/10"
                                : "hover:bg-muted"
                            )}
                            onClick={() => setSelectedExistingType(type.device_type)}
                          >
                            <div className="flex items-center justify-between">
                              <div className="flex-1">
                                <div className="flex items-center gap-2">
                                  <span className="font-medium">{type.name}</span>
                                  {type.is_exact_match && (
                                    <Badge variant="default" className="text-xs">
                                      {t('devices:pending.confidence.high')}
                                    </Badge>
                                  )}
                                </div>
                                <p className="text-sm text-muted-foreground">{type.description}</p>
                                <p className="text-xs text-muted-foreground mt-1">
                                  {type.device_type} • {type.metric_count} {t('devices:pending.metrics')}
                                </p>
                              </div>
                              <div className="text-right">
                                <Badge
                                  variant={type.match_score >= 80 ? "default" : type.match_score >= 50 ? "secondary" : "outline"}
                                  className="ml-2"
                                >
                                  {type.match_score}%
                                </Badge>
                              </div>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="text-sm text-muted-foreground py-2">
                        No matching device types found
                      </div>
                    )}
                  </div>
                )}

                {/* Option 2: Create new type */}
                {selectedTypeOption === 'new' && (
                  <div className="p-4 border rounded-lg space-y-3">
                    <div className="grid grid-cols-2 gap-3">
                      <div>
                        <Label htmlFor="new-type-id" className="text-xs">{t('devices:types.headers.id')}</Label>
                        <Input
                          id="new-type-id"
                          value={newTypeForm.device_type}
                          onChange={(e) => setNewTypeForm({ ...newTypeForm, device_type: e.target.value })}
                          placeholder="e.g. dht22_sensor"
                          className="h-8"
                        />
                      </div>
                      <div>
                        <Label htmlFor="new-type-name" className="text-xs">{t('devices:types.headers.name')}</Label>
                        <Input
                          id="new-type-name"
                          value={newTypeForm.name}
                          onChange={(e) => setNewTypeForm({ ...newTypeForm, name: e.target.value })}
                          placeholder="e.g. DHT22 Sensor"
                          className="h-8"
                        />
                      </div>
                    </div>
                    <div>
                      <Label htmlFor="new-type-desc" className="text-xs">{t('devices:types.headers.description')}</Label>
                      <Textarea
                        id="new-type-desc"
                        value={newTypeForm.description}
                        onChange={(e) => setNewTypeForm({ ...newTypeForm, description: e.target.value })}
                        placeholder="Describe this device type..."
                        rows={2}
                      />
                    </div>
                  </div>
                )}
              </div>
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={() => setShowApproveDialog(false)}>
                {t('common:cancel')}
              </Button>
              <Button
                onClick={handleFinalApprove}
                disabled={processing === selectedDraftForApproval.id || (selectedTypeOption === 'existing' && !selectedExistingType)}
              >
                {processing === selectedDraftForApproval.id ? t('common:processing') : t('devices:pending.approve')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}

      {/* Draft Details Dialog */}
      {selectedDraft && (
        <DraftDetailDialog
          draft={selectedDraft}
          open={!!selectedDraft}
          onOpenChange={(open) => !open && setSelectedDraft(null)}
          onApprove={handleApproveClick}
          onReject={handleReject}
          onAnalyze={handleAnalyze}
          onEnhance={handleEnhance}
          processing={processing}
        />
      )}
    </>
  )
}

// Draft detail dialog component
interface DraftDetailDialogProps {
  draft: DraftDevice
  open: boolean
  onOpenChange: (open: boolean) => void
  onApprove?: (draft: DraftDevice) => void
  onReject?: (draft: DraftDevice) => void
  onAnalyze?: (draft: DraftDevice) => void
  onEnhance?: (draft: DraftDevice) => void
  processing?: string | null
}

interface EditingMetric {
  name: string
  display_name: string
  path: string
  data_type: string
  unit: string
  semantic_type: string
  confidence: number
}

function DraftDetailDialog({ draft, open, onOpenChange, onApprove, onReject, onAnalyze, onEnhance, processing }: DraftDetailDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()

  const genType = draft.generated_type
  const [activeTab, setActiveTab] = useState<'overview' | 'samples' | 'metrics'>('overview')
  const [editingMetrics, setEditingMetrics] = useState<EditingMetric[]>([])
  const [isEditingMetrics, setIsEditingMetrics] = useState(false)
  const [selectedSampleIndex, setSelectedSampleIndex] = useState(0)

  // Initialize editing metrics when genType changes
  useEffect(() => {
    if (genType?.metrics) {
      setEditingMetrics(genType.metrics.map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        path: m.path,
        data_type: m.data_type || 'unknown',
        unit: m.unit || '',
        semantic_type: m.semantic_type,
        confidence: m.confidence
      })))
    }
  }, [genType])

  // Normalize status for consistent display
  const normalizeStatus = (status: string): string => {
    return status.toLowerCase().replace(/[^a-z]/g, '_')
  }

  const normalizedStatus = normalizeStatus(draft.status)
  const canApprove = normalizedStatus === 'waiting_processing' && !!genType
  const canReject = normalizedStatus === 'waiting_processing' || normalizedStatus === 'collecting' || normalizedStatus === 'analyzing'
  const canAnalyze = normalizedStatus === 'collecting' || normalizedStatus === 'failed'
  const canEnhance = !!genType && normalizedStatus === 'waiting_processing'

  const handleApproveClick = () => {
    onApprove?.(draft)
    onOpenChange(false)
  }

  const handleRejectClick = () => {
    onReject?.(draft)
    onOpenChange(false)
  }

  const handleAnalyzeClick = () => {
    onAnalyze?.(draft)
    onOpenChange(false)
  }

  const handleEnhanceClick = () => {
    onEnhance?.(draft)
    // Don't close dialog - enhancement happens in background
  }

  // Save metrics edits
  const handleSaveMetrics = async () => {
    try {
      await api.updateDraftDevice(draft.device_id, {
        name: draft.user_name || draft.device_id,
        description: draft.user_description || genType?.description || ''
      })
      toast({
        title: t('common:success'),
        description: t('devices:pending.saveMetrics'),
      })
      setIsEditingMetrics(false)
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.createTypeFailed'),
        variant: 'destructive'
      })
    }
  }

  // Create device type from draft
  const handleCreateDeviceType = async () => {
    if (!genType) return

    try {
      const deviceType = {
        device_type: genType.device_type,
        name: genType.name,
        description: genType.description,
        categories: [genType.category],
        metrics: editingMetrics.map(m => ({
          name: m.name,
          display_name: m.display_name,
          data_type: m.data_type,
          unit: m.unit,
          path: m.path
        })),
        commands: []
      }

      await api.addDeviceType(deviceType as any)
      toast({
        title: t('common:success'),
        description: t('devices:pending.createTypeSuccess', { name: genType.name }),
      })
      onOpenChange(false)
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.createTypeFailed'),
        variant: 'destructive'
      })
    }
  }

  // Update metric field during editing
  const updateMetric = (index: number, field: keyof EditingMetric, value: string | number) => {
    const updated = [...editingMetrics]
    updated[index] = { ...updated[index], [field]: value }
    setEditingMetrics(updated)
  }

  // Format timestamp
  const formatTime = (timestamp: number) => {
    return new Date(timestamp * 1000).toLocaleTimeString()
  }

  // Get current sample
  const currentSample = draft.samples?.[selectedSampleIndex]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl max-h-[90vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span className="font-mono text-sm">{draft.device_id}</span>
            <Badge variant="outline" className="text-xs">
              {draft.source}
            </Badge>
          </DialogTitle>
          <DialogDescription>
            {t('devices:pending.detailDesc')}
          </DialogDescription>
        </DialogHeader>

        {/* Tabs */}
        <div className="border-b">
          <div className="flex gap-4">
            <button
              onClick={() => setActiveTab('overview')}
              className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
                activeTab === 'overview'
                  ? 'border-primary text-primary'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              {t('devices:pending.status')}
            </button>
            <button
              onClick={() => setActiveTab('samples')}
              className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
                activeTab === 'samples'
                  ? 'border-primary text-primary'
                  : 'border-transparent text-muted-foreground hover:text-foreground'
              }`}
            >
              {t('devices:pending.originalData')}
              {draft.samples && draft.samples.length > 0 && (
                <Badge variant="secondary" className="ml-2 text-xs">{draft.samples.length}</Badge>
              )}
            </button>
            {genType && (
              <button
                onClick={() => setActiveTab('metrics')}
                className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
                  activeTab === 'metrics'
                    ? 'border-primary text-primary'
                    : 'border-transparent text-muted-foreground hover:text-foreground'
                }`}
              >
                {t('devices:pending.metrics')}
                <Badge variant="secondary" className="ml-2 text-xs">{genType.metrics.length}</Badge>
              </button>
            )}
          </div>
        </div>

        {/* Tab Content */}
        <div className="flex-1 overflow-y-auto py-4">
          {activeTab === 'overview' && (
            <div className="space-y-6">
              {/* Status info */}
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="text-sm font-medium text-muted-foreground">{t('devices:pending.headers.status')}</label>
                  <div className="mt-1">
                    <span className="capitalize">{draft.status.replace(/_/g, ' ')}</span>
                  </div>
                </div>
                <div>
                  <label className="text-sm font-medium text-muted-foreground">{t('devices:pending.headers.samples')}</label>
                  <div className="mt-1">
                    {draft.sample_count} / {draft.max_samples}
                  </div>
                </div>
              </div>

              {/* Generated type info */}
              {genType ? (
                <>
                  <div className="border-t pt-4">
                    <h3 className="font-semibold mb-3">{t('devices:pending.generatedType')}</h3>
                    <div className="space-y-3">
                      <div>
                        <label className="text-sm font-medium text-muted-foreground">{t('devices:types.headers.name')}</label>
                        <p className="mt-1">{genType.name}</p>
                      </div>
                      <div>
                        <label className="text-sm font-medium text-muted-foreground">{t('devices:types.headers.description')}</label>
                        <p className="mt-1 text-sm text-muted-foreground">{genType.description}</p>
                      </div>
                      <div className="grid grid-cols-2 gap-4">
                        <div>
                          <label className="text-sm font-medium text-muted-foreground">{t('devices:pending.headers.category')}</label>
                          <p className="mt-1 capitalize">{genType.category.toLowerCase()}</p>
                        </div>
                        <div>
                          <label className="text-sm font-medium text-muted-foreground">{t('devices:pending.headers.confidence')}</label>
                          <p className="mt-1">{Math.round(genType.confidence * 100)}%</p>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* Summary */}
                  {genType.summary && (
                    <div className="border-t pt-4">
                      <h4 className="font-semibold mb-3">{t('devices:pending.analysisSummary')}</h4>
                      <div className="space-y-2 text-sm">
                        <div className="flex justify-between">
                          <span className="text-muted-foreground">{t('devices:pending.samplesAnalyzed')}:</span>
                          <span>{genType.summary.samples_analyzed}</span>
                        </div>
                        <div className="flex justify-between">
                          <span className="text-muted-foreground">{t('devices:pending.fieldsDiscovered')}:</span>
                          <span>{genType.summary.fields_discovered}</span>
                        </div>
                        {genType.summary.warnings.length > 0 && (
                          <div className="rounded-md bg-yellow-500/10 p-3">
                            <p className="font-medium text-yellow-700 mb-1">{t('devices:pending.warnings')}:</p>
                            <ul className="list-disc list-inside text-sm text-yellow-600 space-y-1">
                              {genType.summary.warnings.map((warning, i) => (
                                <li key={i}>{warning}</li>
                              ))}
                            </ul>
                          </div>
                        )}
                      </div>
                    </div>
                  )}
                </>
              ) : (
                <div className="text-center py-8 text-muted-foreground">
                  <p>{t('devices:pending.noGeneratedType')}</p>
                  <p className="text-sm">{t('devices:pending.waitingForSamples')}</p>
                </div>
              )}

              {/* Error message */}
              {draft.error_message && (
                <div className="rounded-md bg-red-500/10 p-3">
                  <p className="font-medium text-red-700">{t('devices:pending.error')}:</p>
                  <p className="text-sm text-red-600">{draft.error_message}</p>
                </div>
              )}
            </div>
          )}

          {activeTab === 'samples' && (
            <div className="space-y-4">
              {!draft.samples || draft.samples.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground">
                  <p>{t('devices:pending.noSamples')}</p>
                </div>
              ) : (
                <>
                  {/* Sample selector */}
                  <div className="flex items-center gap-4">
                    <span className="text-sm text-muted-foreground">{t('devices:pending.originalDataDesc')}:</span>
                    <div className="flex gap-1">
                      {draft.samples.map((_, index) => (
                        <button
                          key={index}
                          onClick={() => setSelectedSampleIndex(index)}
                          className={`w-8 h-8 text-xs rounded ${
                            selectedSampleIndex === index
                              ? 'bg-primary text-primary-foreground'
                              : 'bg-muted text-muted-foreground hover:bg-muted-foreground/20'
                          }`}
                        >
                          {index + 1}
                        </button>
                      ))}
                    </div>
                  </div>

                  {/* Sample details */}
                  {currentSample && (
                    <div className="space-y-4">
                      <div className="flex items-center justify-between">
                        <span className="text-sm text-muted-foreground">
                          {t('devices:pending.headers.source')}: {currentSample.source}
                        </span>
                        <span className="text-sm text-muted-foreground">
                          {formatTime(currentSample.timestamp)}
                        </span>
                      </div>

                      {/* Parsed data */}
                      {currentSample.parsed ? (
                        <div>
                          <h4 className="text-sm font-medium mb-2">{t('devices:pending.parsedData')}</h4>
                          <pre className="bg-muted p-4 rounded-lg text-xs overflow-x-auto">
                            {JSON.stringify(currentSample.parsed, null, 2)}
                          </pre>
                        </div>
                      ) : null}

                      {/* Raw data */}
                      {currentSample.raw_data && currentSample.raw_data.length > 0 ? (
                        <div>
                          <h4 className="text-sm font-medium mb-2">{t('devices:pending.rawData')}</h4>
                          <pre className="bg-muted p-4 rounded-lg text-xs overflow-x-auto break-all">
                            {currentSample.raw_data.map(b => b.toString(16).padStart(2, '0')).join(' ')}
                          </pre>
                        </div>
                      ) : null}
                    </div>
                  )}
                </>
              )}
            </div>
          )}

          {activeTab === 'metrics' && genType && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h3 className="font-semibold">{t('devices:pending.metrics')}</h3>
                {!isEditingMetrics ? (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => setIsEditingMetrics(true)}
                  >
                    {t('devices:pending.editMetrics')}
                  </Button>
                ) : (
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setIsEditingMetrics(false)
                        // Reset to original values
                        setEditingMetrics(genType.metrics.map(m => ({
                          name: m.name,
                          display_name: m.display_name || m.name,
                          path: m.path,
                          data_type: m.data_type || 'unknown',
                          unit: m.unit || '',
                          semantic_type: m.semantic_type,
                          confidence: m.confidence
                        })))
                      }}
                    >
                      {t('common:cancel')}
                    </Button>
                    <Button
                      variant="default"
                      size="sm"
                      onClick={handleSaveMetrics}
                    >
                      {t('devices:pending.saveMetrics')}
                    </Button>
                  </div>
                )}
              </div>

              <div className="border rounded-lg overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>{t('devices:pending.metricName')}</TableHead>
                      <TableHead>{t('devices:types.headers.displayName')}</TableHead>
                      <TableHead>{t('devices:pending.path')}</TableHead>
                      <TableHead>{t('devices:types.headers.dataType')}</TableHead>
                      <TableHead>{t('devices:types.headers.unit')}</TableHead>
                      <TableHead>{t('devices:pending.semanticType')}</TableHead>
                      <TableHead>{t('devices:pending.confidence')}</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {editingMetrics.map((metric, index) => (
                      <TableRow key={index}>
                        <TableCell className="font-mono text-xs">
                          {isEditingMetrics ? (
                            <input
                              type="text"
                              value={metric.name}
                              onChange={(e) => updateMetric(index, 'name', e.target.value)}
                              className="w-full bg-transparent border-b border-border focus:border-primary outline-none px-1"
                            />
                          ) : (
                            metric.name
                          )}
                        </TableCell>
                        <TableCell className="text-xs">
                          {isEditingMetrics ? (
                            <input
                              type="text"
                              value={metric.display_name}
                              onChange={(e) => updateMetric(index, 'display_name', e.target.value)}
                              className="w-full bg-transparent border-b border-border focus:border-primary outline-none px-1"
                            />
                          ) : (
                            metric.display_name
                          )}
                        </TableCell>
                        <TableCell className="font-mono text-xs text-muted-foreground">
                          {metric.path}
                        </TableCell>
                        <TableCell className="text-xs">
                          <select
                            value={metric.data_type}
                            onChange={(e) => isEditingMetrics ? updateMetric(index, 'data_type', e.target.value) : undefined}
                            disabled={!isEditingMetrics}
                            className={`bg-transparent ${isEditingMetrics ? 'border-b border-border focus:border-primary outline-none' : ''}`}
                          >
                            <option value="integer">Integer</option>
                            <option value="float">Float</option>
                            <option value="string">String</option>
                            <option value="boolean">Boolean</option>
                            <option value="unknown">Unknown</option>
                          </select>
                        </TableCell>
                        <TableCell className="text-xs">
                          {isEditingMetrics ? (
                            <input
                              type="text"
                              value={metric.unit}
                              onChange={(e) => updateMetric(index, 'unit', e.target.value)}
                              className="w-16 bg-transparent border-b border-border focus:border-primary outline-none px-1"
                            />
                          ) : (
                            metric.unit || '-'
                          )}
                        </TableCell>
                        <TableCell className="capitalize text-xs">
                          {metric.semantic_type.toLowerCase()}
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={metric.confidence >= 0.7 ? "default" : "secondary"}
                            className={metric.confidence >= 0.7 ? "bg-green-500/20 text-green-700" : "bg-yellow-500/20 text-yellow-700"}
                          >
                            {Math.round(metric.confidence * 100)}%
                          </Badge>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>

              {/* Create device type button */}
              {isEditingMetrics && (
                <div className="flex justify-end pt-4 border-t">
                  <Button
                    variant="default"
                    onClick={handleCreateDeviceType}
                    className="bg-purple-600 hover:bg-purple-700"
                  >
                    {t('devices:pending.createDeviceType')}
                  </Button>
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter className="flex justify-between gap-2">
          <div className="flex gap-2">
            {canAnalyze && (
              <Button
                variant="outline"
                onClick={handleAnalyzeClick}
                disabled={processing === draft.id}
              >
                <Zap className="mr-2 h-4 w-4" />
                {t('devices:pending.triggerAnalysis')}
              </Button>
            )}
            {canEnhance && (
              <Button
                variant="outline"
                className="text-purple-600 hover:text-purple-700 border-purple-200 hover:bg-purple-50"
                onClick={handleEnhanceClick}
                disabled={processing === draft.id}
              >
                <Sparkles className="mr-2 h-4 w-4" />
                {t('devices:pending.enhanceWithLLM')}
              </Button>
            )}
          </div>
          <div className="flex gap-2">
            <Button
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              {t('common:close')}
            </Button>
            {canReject && (
              <Button
                variant="destructive"
                onClick={handleRejectClick}
                disabled={processing === draft.id}
              >
                <X className="mr-2 h-4 w-4" />
                {t('devices:pending.reject')}
              </Button>
            )}
            {canApprove && (
              <Button
                variant="default"
                className="bg-green-600 hover:bg-green-700"
                onClick={handleApproveClick}
                disabled={processing === draft.id}
              >
                <Check className="mr-2 h-4 w-4" />
                {t('devices:pending.approve')}
              </Button>
            )}
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

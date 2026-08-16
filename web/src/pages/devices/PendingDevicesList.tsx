import { useState, useEffect, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { Badge } from "@/components/ui/badge"
import { ResponsiveTable, EmptyState } from "@/components/shared"
import { Cpu, Globe, Badge as BadgeIcon, Clock, Activity, Check, ChevronDown, X, Loader2, Search as SearchIcon, Hourglass, CheckCircle2, XCircle, AlertTriangle, BarChart3, CircleDot } from "lucide-react"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableHeader,
  TableRow,
  TableHead,
  TableBody,
  TableCell,
} from "@/components/ui/table"
import { cn } from "@/lib/utils"
import { formatTimestamp } from "@/lib/utils/format"
import { useToast } from "@/hooks/use-toast"
import { useEvents } from "@/hooks/useEvents"
import { api } from "@/lib/api"
import { useIsMobile } from "@/hooks/useMobile"
import type { DraftDevice, SuggestedDeviceType } from "@/types"

interface PendingDevicesListProps {
  onRefresh?: () => void
  page?: number
  onPageChange?: (page: number) => void
  itemsPerPage?: number
  onDraftsCountChange?: (count: number) => void
}

export function PendingDevicesList({
  onRefresh,
  page: externalPage,
  onPageChange: externalOnPageChange,
  itemsPerPage: externalItemsPerPage,
  onDraftsCountChange
}: PendingDevicesListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const isMobile = useIsMobile()

  const [drafts, setDrafts] = useState<DraftDevice[]>([])
  const [loading, setLoading] = useState(true)
  const navigate = useNavigate()
  const openSettings = useStore((s) => s.openSettings)
  const [hasBroker, setHasBroker] = useState<boolean | null>(null)

  // Use external pagination state if provided, otherwise use internal state
  const [internalPage, setInternalPage] = useState(externalPage || 1)
  const page = externalPage ?? internalPage
  const setPage = externalOnPageChange ?? setInternalPage
  const itemsPerPage = externalItemsPerPage || 10

  const [processing, setProcessing] = useState<string | null>(null)

  // Reject confirmation dialog state
  const [rejectDialogDraft, setRejectDialogDraft] = useState<DraftDevice | null>(null)

  // Unified dialog state
  const [showApproveDialog, setShowApproveDialog] = useState(false)
  const [selectedDraftForApproval, setSelectedDraftForApproval] = useState<DraftDevice | null>(null)
  const [selectedSampleIndex, setSelectedSampleIndex] = useState(0)
  const [suggestedTypes, setSuggestedTypes] = useState<SuggestedDeviceType[]>([])
  const [loadingSuggestions, setLoadingSuggestions] = useState(false)

  // Type selection state - unified approach (can select existing or create new)
  const [selectedDeviceType, setSelectedDeviceType] = useState('')
  const [showTypeDropdown, setShowTypeDropdown] = useState(false)
  const [typeInputValue, setTypeInputValue] = useState('')

  // New type additional fields (only shown when creating a new type)
  const [newTypeFields, setNewTypeFields] = useState({
    name: '',  // Device instance name
    type_name: '',  // Device type display name
    description: '',
    device_type: ''
  })

  // Metrics editing state
  const [isEditingMetrics, setIsEditingMetrics] = useState(false)
  const [editingMetrics, setEditingMetrics] = useState<Array<{
    name: string
    display_name: string
    path: string
    unit: string
    data_type: string
  }>>([])

  // Check if selected type is an existing type or a new one
  const isNewType = useMemo(() => {
    if (!selectedDeviceType) return false
    return !suggestedTypes.some(t => t.device_type === selectedDeviceType)
  }, [selectedDeviceType, suggestedTypes])

  // Registered/Rejected devices are removed from drafts and won't appear here
  const activeDrafts = drafts.filter(draft =>
    draft.status === 'waiting_processing'
  )
  const registeredCount = drafts.filter(d => d.status === 'registered').length

  // Paginated data
  // On mobile: show cumulative data (all pages up to current) for infinite scroll
  // On desktop: show only current page
  const paginatedDrafts = isMobile
    ? activeDrafts.slice(0, page * itemsPerPage)
    : activeDrafts.slice(
        (page - 1) * itemsPerPage,
        page * itemsPerPage
      )

  // Reset pagination when data changes (only if using internal pagination)
  useEffect(() => {
    if (!externalOnPageChange) {
      setPage(1)
    }
  }, [activeDrafts.length, externalOnPageChange])

  // Notify parent component of drafts count
  useEffect(() => {
    onDraftsCountChange?.(activeDrafts.length)
  }, [activeDrafts.length, onDraftsCountChange])

  // Fetch drafts
  const fetchDrafts = useCallback(async () => {
    setLoading(true)
    try {
      const response = await api.getDraftDevices()
      const updatedDrafts = response.items || []
      // Sort by updated_at descending (newest first)
      const sortedDrafts = updatedDrafts.sort((a, b) => b.updated_at - a.updated_at)
      setDrafts(sortedDrafts)

      // Update selectedDraftForApproval if dialog is open
      if (selectedDraftForApproval) {
        const updatedDraft = sortedDrafts.find(d => d.id === selectedDraftForApproval.id)
        if (updatedDraft) {
          setSelectedDraftForApproval(updatedDraft)
        }
      }
    } catch (error) {
      handleError(error, { operation: 'Fetch draft devices', showToast: false })
      // Don't show error toast - endpoint might not be implemented yet
      setDrafts([])
    } finally {
      setLoading(false)
    }
  }, [selectedDraftForApproval])

  // Check broker status when drafts are empty
  useEffect(() => {
    if (loading || activeDrafts.length > 0) return

    let cancelled = false
    const checkBroker = async () => {
      try {
        const { status } = await api.getMqttStatus()
        if (!cancelled) {
          const hasBuiltIn = status.connected
          const hasExternal = !!status.external_brokers && status.external_brokers.length > 0
          setHasBroker(hasBuiltIn || hasExternal)
        }
      } catch {
        if (!cancelled) setHasBroker(false)
      }
    }
    checkBroker()
    return () => { cancelled = true }
  }, [loading, activeDrafts.length])

  // Fetch type signatures for type reuse
  const fetchTypeSignatures = useCallback(async () => {
    // This is no longer needed - we use suggestDeviceTypes instead
  }, [])

  // Use WebSocket events for real-time updates
  const handleAutoOnboardEvent = useCallback((event: { type: string; data: unknown }) => {
    // Custom events from backend have: { custom_type: "auto_onboard", data: { event_type: "draft_created", ... } }
    if (event.type === 'Custom') {
      const customData = event.data as { custom_type?: string; data?: { event_type?: string; device_id?: string } }
      if (customData.custom_type === 'auto_onboard') {
        const innerEventType = customData.data?.event_type
        if (innerEventType === 'draft_created' ||
            innerEventType === 'sample_collected' ||
            innerEventType === 'analysis_started' ||
            innerEventType === 'analysis_completed' ||
            innerEventType === 'device_registered' ||
            innerEventType === 'device_rejected') {
          fetchDrafts()
        }
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
    setSelectedSampleIndex(0)
    setLoadingSuggestions(true)
    setSuggestedTypes([])
    setSelectedDeviceType('')
    setTypeInputValue('')
    setNewTypeFields({ name: '', type_name: '', description: '', device_type: '' })
    setIsEditingMetrics(false)

    // Initialize new type form from generated type
    if (draft.generated_type) {
      setNewTypeFields({
        device_type: draft.generated_type.device_type,
        name: draft.generated_type.name,
        type_name: draft.generated_type.name,  // Default type name to generated name
        description: draft.generated_type.description,
      })
      // Initialize editing metrics
      setEditingMetrics((draft.generated_type.metrics || []).map(m => ({
        name: m.name,
        display_name: m.display_name || m.name,
        path: m.path,
        unit: m.unit || '',
        data_type: m.data_type || 'string'
      })))
    }

    // Fetch suggested types
    try {
      const response = await api.suggestDeviceTypes(draft.device_id)
      setSuggestedTypes(response.suggestions || [])
      // Auto-select exact match if found
      if (response.exact_match) {
        setSelectedDeviceType(response.exact_match)
        setTypeInputValue(response.exact_match)
      } else {
        // Auto-select type with match_score > 50%
        const highMatch = response.suggestions?.find(s => s.match_score > 50)
        if (highMatch) {
          setSelectedDeviceType(highMatch.device_type)
          setTypeInputValue(highMatch.device_type)
        }
      }
    } catch (error) {
      handleError(error, { operation: 'Fetch suggested types', showToast: false })
      // Show empty state on error
      setSuggestedTypes([])
    } finally {
      setLoadingSuggestions(false)
    }
  }

  // Validate form before submission
  const validateForm = (): boolean => {
    if (!selectedDeviceType.trim()) {
      toast({
        title: t('common:warning'),
        description: t('devices:pending.pleaseSelectType'),
        variant: 'destructive'
      })
      return false
    }

    // Name is always required (whether creating new type or using existing)
    if (!newTypeFields.name.trim()) {
      toast({
        title: t('common:warning'),
        description: t('devices:pending.pleaseEnterDeviceName'),
        variant: 'destructive'
      })
      return false
    }

    return true
  }

  // Handle final approval after type selection
  const handleFinalApprove = async () => {
    if (!selectedDraftForApproval) return

    if (!validateForm()) {
      return
    }
    setProcessing(selectedDraftForApproval.id)
    try {
      let result
      if (isNewType) {
        // Create new type - pass the new type details and device name
        result = await api.approveDraftDeviceWithType(
          selectedDraftForApproval.device_id,
          undefined, // undefined means create new type
          {
            device_type: selectedDeviceType,
            name: newTypeFields.type_name || newTypeFields.name, // Type name (for the device type)
            description: newTypeFields.description,
          },
          newTypeFields.name // Device instance name
        )
      } else {
        // Use existing type - pass device name
        result = await api.approveDraftDeviceWithType(
          selectedDraftForApproval.device_id,
          selectedDeviceType,
          undefined, // No new type info needed
          newTypeFields.name // Device instance name
        )
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
      setSelectedDeviceType('')
      setTypeInputValue('')
      setNewTypeFields({ name: '', type_name: '', description: '', device_type: '' })
    }
  }

  // Reject draft device
  const handleReject = async (draft: DraftDevice) => {
    setRejectDialogDraft(draft)
  }

  // Confirm rejection
  const confirmReject = async () => {
    if (!rejectDialogDraft) return

    setProcessing(rejectDialogDraft.id)
    try {
      await api.rejectDraftDevice(rejectDialogDraft.device_id, { reason: 'User rejected' })
      toast({
        title: t('common:success'),
        description: t('devices:pending.rejected', { deviceId: rejectDialogDraft.device_id }),
      })
      await fetchDrafts()
      onRefresh?.()  // Also refresh device and device type lists
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.rejectFailed'),
        variant: "destructive"
      })
    } finally {
      setProcessing(null)
      setRejectDialogDraft(null)
    }
  }

  // Normalize status string for consistent comparison
  const normalizeStatus = (status: string): string => {
    return status.toLowerCase().replace(/[^a-z]/g, '_')
  }

  // Get status badge
  const getStatusBadge = (status: string) => {
    const statusMap: Record<string, { color: string; label: string; icon: React.ReactNode }> = {
      collecting: { color: "bg-info-light text-info", label: t('devices:pending.status.collecting'), icon: <Loader2 className="h-4 w-4" /> },
      analyzing: { color: "bg-accent-purple-light text-accent-purple", label: t('devices:pending.status.analyzing'), icon: <SearchIcon className="h-4 w-4" /> },
      waiting_processing: { color: "bg-warning-light text-warning", label: t('devices:pending.status.waitingProcessing'), icon: <Hourglass className="h-4 w-4" /> },
      registered: { color: "bg-success-light text-success", label: t('devices:pending.status.registered'), icon: <CheckCircle2 className="h-4 w-4" /> },
      rejected: { color: "bg-error-light text-error", label: t('devices:pending.status.rejected'), icon: <XCircle className="h-4 w-4" /> },
      failed: { color: "bg-error-light text-error", label: t('devices:pending.status.failed'), icon: <AlertTriangle className="h-4 w-4" /> },
    }

    const key = normalizeStatus(status)
    const info = statusMap[key] || { color: "bg-muted text-foreground", label: status, icon: <Activity className="h-4 w-4" /> }

    return (
      <Badge className={cn("font-normal gap-1", info.color)}>
        {info.icon}
        {info.label}
      </Badge>
    )
  }

  return (
    <>
      <ResponsiveTable
        columns={[
          {
            key: 'deviceId',
            label: t('devices:pending.headers.deviceId'),
          },
          {
            key: 'source',
            label: t('devices:pending.headers.source'),
          },
          {
            key: 'deviceType',
            label: t('devices:pending.deviceType'),
          },
          {
            key: 'status',
            label: t('devices:pending.headers.status'),
            align: 'center',
          },
          {
            key: 'metrics',
            label: t('devices:pending.metrics'),
            align: 'center',
          },
          {
            key: 'discoveredAt',
            label: t('devices:pending.headers.discoveredAt'),
            align: 'center',
          },
        ]}
        data={paginatedDrafts}
        rowKey={(draft: DraftDevice) => draft.id}
        loading={loading}
        emptyState={
          hasBroker === false ? (
            <EmptyState
              icon="settings"
              title={t('devices:pending.noBrokerTitle')}
              description={t('devices:pending.noBrokerDesc')}
              action={{
                label: t('devices:pending.goToSettings'),
                onClick: () => openSettings('connections'),
              }}
            />
          ) : (
            <EmptyState
              icon="inbox"
              title={t('devices:pending.noDraftsTitle')}
              description={t('devices:pending.noDraftsDesc')}
            />
          )
        }
        renderCell={(columnKey, rowData) => {
          const draft = rowData
          const hasGeneratedType = draft.generated_type && draft.status === 'waiting_processing'
          const confidence = draft.generated_type?.confidence

          switch (columnKey) {
            case 'deviceId':
              return (
                <div className="flex items-center gap-3">
                  <div className={cn(
                    "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                    draft.status === 'waiting_processing'
                      ? "bg-warning-light text-warning"
                      : draft.status === 'analyzing'
                        ? "bg-accent-purple-light text-accent-purple"
                        : "bg-muted text-muted-foreground"
                  )}>
                    <Cpu className="h-4 w-4" />
                  </div>
                  <div className="min-w-0">
                    <code className="text-xs text-muted-foreground font-mono block truncate">
                      {draft.device_id}
                    </code>
                    {draft.user_name && (
                      <div className="text-xs font-medium text-foreground truncate">
                        {draft.user_name}
                      </div>
                    )}
                  </div>
                </div>
              )

            case 'source':
              return (
                <Badge variant="outline" className="text-xs">
                  {draft.source.includes(':') ? draft.source.split(':')[0] : draft.source}
                </Badge>
              )

            case 'deviceType':
              return hasGeneratedType ? (
                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium truncate">
                      {draft.generated_type?.name}
                    </span>
                    {confidence !== undefined && (
                      <Badge
                        variant={confidence >= 80 ? "default" : "outline"}
                        className={cn(
                          "text-xs",
                          confidence >= 80
                            ? "bg-success-light text-success border-success"
                            : "bg-warning-light text-warning border-warning"
                        )}
                      >
                        {confidence}%
                      </Badge>
                    )}
                  </div>
                  <code className="text-xs text-muted-foreground font-mono truncate block">
                    {draft.generated_type?.device_type}
                  </code>
                </div>
              ) : draft.status === 'analyzing' ? (
                <span className="text-xs text-muted-foreground">{t('devices:pending.analyzing')}</span>
              ) : (
                <span className="text-xs text-muted-foreground">-</span>
              )

            case 'status':
              return (
                <div className="flex justify-center">
                  {getStatusBadge(draft.status)}
                </div>
              )

            case 'metrics':
              return hasGeneratedType ? (
                <div className="flex justify-center">
                  <Badge variant="outline" className="text-xs bg-info-light text-info border-info">
                    {draft.generated_type?.metrics?.length || 0}
                  </Badge>
                </div>
              ) : (
                <span className="text-sm">{draft.sample_count} / {draft.max_samples}</span>
              )

            case 'discoveredAt':
              return (
                <span className="text-xs text-muted-foreground">
                  {formatTimestamp(draft.discovered_at, false)}
                </span>
              )

            default:
              return null
          }
        }}
        mobileFlatHeader
        renderMobileHeaderExtra={(rowData) =>
          getStatusBadge((rowData).status)
        }
        renderMobileBody={(rowData) => {
          const draft = rowData
          const hasGeneratedType = draft.generated_type && draft.status === 'waiting_processing'
          const confidence = draft.generated_type?.confidence
          const sourceLabel = draft.source.includes(':') ? draft.source.split(':')[0] : draft.source

          return (
            <div className="space-y-1.5">
              {/* Type section — only shown when analysis is done and the
                  draft is ready for review. Confidence is plain muted text
                  (no colored badge) so the status badge in the header is the
                  only strong color signal on the card. */}
              {hasGeneratedType && (
                <div className="flex items-baseline justify-between gap-2">
                  <span className="text-sm font-medium text-foreground truncate">
                    {draft.generated_type?.name}
                  </span>
                  {confidence !== undefined && (
                    <span className={cn(
                      "shrink-0 text-xs",
                      confidence >= 80 ? "text-success" : "text-warning"
                    )}>
                      {confidence}%
                    </span>
                  )}
                </div>
              )}

              {/* Secondary meta line — device_type code (when available) and
                  source · time on a single row so the bottom of the card
                  reads as one context strip rather than two stacked lines.
                  Status badge lives in the header's top-right slot. */}
              <div className="flex items-center gap-2 text-xs text-muted-foreground min-w-0">
                {hasGeneratedType && (
                  <code className="font-mono truncate shrink-0">
                    {draft.generated_type?.device_type}
                  </code>
                )}
                <span className="truncate ml-auto">
                  {sourceLabel} · {formatTimestamp(draft.discovered_at, false)}
                </span>
              </div>
            </div>
          )
        }}
        onRowClick={(rowData) => {
          const draft = rowData
          handleApproveClick(draft)
        }}
      />

      {/* Summary footer showing registered count */}
      {registeredCount > 0 && (
        <div className="mt-4 flex items-center justify-center gap-4 text-sm text-muted-foreground">
          <span className="flex items-center gap-1">
            <Badge variant="outline" className="bg-success-light text-success">
              {registeredCount}
            </Badge>
            <span>{t('devices:pending.registeredHidden')}</span>
          </span>
        </div>
      )}

      {/* Unified Approval/Details Dialog */}
      {showApproveDialog && selectedDraftForApproval && (
        <UnifiedFormDialog
          open={showApproveDialog}
          onOpenChange={setShowApproveDialog}
          title={t('devices:pending.approveTitle')}
          width="2xl"
          contentClassName="overflow-y-auto"
          onSubmit={handleFinalApprove}
          isSubmitting={processing === selectedDraftForApproval.id}
          submitLabel={t('devices:pending.confirmRegister')}
          submitDisabled={processing === selectedDraftForApproval.id || !selectedDeviceType.trim()}
          footer={
            <div className="flex flex-wrap items-center gap-2 justify-end">
              <Button
                variant="ghost"
                size="sm"
                className="text-error hover:text-error hover:bg-error-light"
                onClick={() => {
                  setShowApproveDialog(false)
                  if (selectedDraftForApproval) handleReject(selectedDraftForApproval)
                }}
                disabled={processing === selectedDraftForApproval.id}
              >
                {t('devices:pending.reject')}
              </Button>
              <Button variant="outline" size="sm" onClick={() => setShowApproveDialog(false)}>
                {t('common:cancel')}
              </Button>
              <Button
                size="sm"
                onClick={handleFinalApprove}
                disabled={processing === selectedDraftForApproval.id || !selectedDeviceType.trim()}
              >
                {processing === selectedDraftForApproval.id ? t('common:processing') : t('devices:pending.confirmRegister')}
              </Button>
            </div>
          }
        >
          <div className="space-y-6">
              {/* Device Info Section */}
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
                  {t('devices:pending.deviceInfo')}
                </h3>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-x-4 sm:gap-x-6 gap-y-2 text-sm bg-muted-30 rounded-lg p-3 sm:p-4">
                  <div>
                    <span className="text-muted-foreground">{t('devices:pending.headers.deviceId')}: </span>
                    <span className="font-mono font-medium">{selectedDraftForApproval.device_id}</span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t('devices:pending.headers.source')}: </span>
                    <Badge variant="outline" className="ml-1 font-mono">
                      {selectedDraftForApproval.source.includes(':')
                        ? selectedDraftForApproval.source.split(':').slice(1).join(':')
                        : selectedDraftForApproval.source}
                    </Badge>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t('devices:pending.headers.status')}: </span>
                    <Badge variant={selectedDraftForApproval.status === 'waiting_processing' ? 'default' : 'secondary'} className="ml-1">
                      {selectedDraftForApproval.status === 'waiting_processing'
                        ? t('devices:pending.status.waitingProcessing')
                        : selectedDraftForApproval.status.replace(/_/g, ' ')}
                    </Badge>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t('devices:pending.headers.samples')}: </span>
                    <span className="font-medium">{selectedDraftForApproval.sample_count} / {selectedDraftForApproval.max_samples}</span>
                  </div>
                </div>
              </div>

              {/* Metrics Section */}
              {selectedDraftForApproval.generated_type ? (
                <>
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-xs text-muted-foreground">{t('devices:pending.metrics')} ({selectedDraftForApproval.generated_type.metrics.length})</span>
                    {!isEditingMetrics ? (
                      <Button variant="ghost" size="xs" onClick={() => setIsEditingMetrics(true)}>
                        {t('common:edit')}
                      </Button>
                    ) : (
                      <div className="flex gap-1">
                        <Button variant="ghost" size="xs" onClick={() => {
                          setIsEditingMetrics(false)
                          // Reset to original values
                          if (selectedDraftForApproval.generated_type) {
                            setEditingMetrics((selectedDraftForApproval.generated_type.metrics || []).map(m => ({
                              name: m.name,
                              display_name: m.display_name || m.name,
                              path: m.path,
                              unit: m.unit || '',
                              data_type: m.data_type || 'string'
                            })))
                          }
                        }}>
                          {t('common:cancel')}
                        </Button>
                        <Button variant="default" size="xs" onClick={() => setIsEditingMetrics(false)}>
                          {t('common:save')}
                        </Button>
                      </div>
                    )}
                  </div>
                  <div className="border rounded-lg overflow-x-auto -mx-1 px-1">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>{t('devices:types.headers.path')}</TableHead>
                          <TableHead>{t('devices:types.headers.displayName')}</TableHead>
                          <TableHead>{t('devices:types.headers.dataType')}</TableHead>
                          <TableHead>{t('devices:types.headers.unit')}</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {(isEditingMetrics ? editingMetrics : (selectedDraftForApproval.generated_type.metrics || [])).map((metric, idx) => (
                          <TableRow key={metric.name}>
                            <TableCell className="font-mono text-xs">{metric.path}</TableCell>
                            <TableCell>
                              {isEditingMetrics ? (
                                <Input
                                  value={metric.display_name}
                                  onChange={(e) => {
                                    const updated = [...editingMetrics]
                                    updated[idx].display_name = e.target.value
                                    setEditingMetrics(updated)
                                  }}
                                  className="h-7 text-xs"
                                />
                              ) : (
                                metric.display_name
                              )}
                            </TableCell>
                            <TableCell>
                              {isEditingMetrics ? (
                                <Select
                                  value={metric.data_type}
                                  onValueChange={(value) => {
                                    const updated = [...editingMetrics]
                                    updated[idx].data_type = value
                                    setEditingMetrics(updated)
                                  }}
                                >
                                  <SelectTrigger className="h-7 text-xs">
                                    <SelectValue />
                                  </SelectTrigger>
                                  <SelectContent>
                                    <SelectItem value="string">String</SelectItem>
                                    <SelectItem value="integer">Integer</SelectItem>
                                    <SelectItem value="float">Float</SelectItem>
                                    <SelectItem value="boolean">Boolean</SelectItem>
                                  </SelectContent>
                                </Select>
                              ) : (
                                <span className="text-xs capitalize">{metric.data_type || 'string'}</span>
                              )}
                            </TableCell>
                            <TableCell>
                              {isEditingMetrics ? (
                                <Input
                                  value={metric.unit}
                                  onChange={(e) => {
                                    const updated = [...editingMetrics]
                                    updated[idx].unit = e.target.value
                                    setEditingMetrics(updated)
                                  }}
                                  placeholder="-"
                                  className="h-7 text-xs w-16"
                                />
                              ) : (
                                metric.unit || '-'
                              )}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                </>
              ) : (
                <div className="text-center py-4 text-muted-foreground text-sm">
                  {t('devices:pending.noGeneratedType')}
                </div>
              )}

              {/* Original Data Section */}
              {selectedDraftForApproval.samples && selectedDraftForApproval.samples.length > 0 && (
                <div className="space-y-3">
                  <h3 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
                    {t('devices:pending.originalData')}
                  </h3>
                  <div className="bg-muted-30 rounded-lg p-3">
                    <div className="flex gap-1 mb-3">
                      {selectedDraftForApproval.samples.slice(0, 5).map((_, index) => (
                        <button
                          key={index}
                          onClick={() => setSelectedSampleIndex(index)}
                          onTouchEnd={(e) => {
                            e.preventDefault()
                            setSelectedSampleIndex(index)
                          }}
                          className={`w-6 h-6 text-xs rounded ${
                            selectedSampleIndex === index
                              ? 'bg-primary text-primary-foreground'
                              : 'bg-background hover:bg-muted'
                          }`}
                          style={{ touchAction: 'manipulation' }}
                        >
                          {index + 1}
                        </button>
                      ))}
                    </div>
                    {selectedDraftForApproval.samples[selectedSampleIndex]?.parsed && (
                      <pre className="text-xs bg-background p-3 rounded overflow-x-auto">
                        {JSON.stringify(selectedDraftForApproval.samples[selectedSampleIndex].parsed, null, 2)}
                      </pre>
                    )}
                  </div>
                </div>
              )}

              {/* Device Information Section */}
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
                  {t('devices:pending.deviceInfo')}
                </h3>

                {/* Device Name Field */}
                <div>
                  <Label className="text-xs text-muted-foreground">
                    {t('devices:pending.deviceName')} <span className="text-error">*</span>
                  </Label>
                  <Input
                    value={newTypeFields.name}
                    onChange={(e) => setNewTypeFields({ ...newTypeFields, name: e.target.value })}
                    placeholder={t('devices:pending.deviceNamePlaceholder')}
                    className="h-9 mt-1"
                  />
                </div>
              </div>

              {/* Device Type Selection Section */}
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
                  {t('devices:pending.deviceTypeSelection')}
                  <span className="text-error ml-1">*</span>
                </h3>

                {/* Unified Type Input with Dropdown */}
                <div className="relative">
                  <Label className="text-xs text-muted-foreground mb-1.5 block">
                    {t('devices:pending.selectOrCreateType')}
                  </Label>
                  <div className="relative">
                    <Input
                      value={typeInputValue}
                      onChange={(e) => {
                        const value = e.target.value
                        setTypeInputValue(value)
                        setSelectedDeviceType(value)
                        setShowTypeDropdown(value.length > 0)
                      }}
                      onFocus={() => {
                        if (typeInputValue.length === 0) {
                          setShowTypeDropdown(true)
                        }
                      }}
                      onBlur={() => {
                        // Delay to allow clicking on dropdown items
                        setTimeout(() => setShowTypeDropdown(false), 200)
                      }}
                      placeholder={t('devices:pending.typeInputPlaceholder')}
                      className="pr-10"
                    />
                    <button
                      type="button"
                      onClick={() => setShowTypeDropdown(!showTypeDropdown)}
                      className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    >
                      <ChevronDown className="h-4 w-4" />
                    </button>
                  </div>

                  {/* Dropdown with suggested types */}
                  {showTypeDropdown && (
                    <div className="absolute z-10 w-full mt-1 bg-background border border-border rounded-lg shadow-lg max-h-64 overflow-y-auto">
                      {loadingSuggestions ? (
                        <div className="p-3 text-sm text-muted-foreground text-center">
                          {t('common:loading')}...
                        </div>
                      ) : suggestedTypes.length > 0 ? (
                        <>
                          {suggestedTypes.map((type) => (
                            <div
                              key={type.device_type}
                              onClick={() => {
                                setSelectedDeviceType(type.device_type)
                                setTypeInputValue(type.device_type)
                                setShowTypeDropdown(false)
                              }}
                              onTouchEnd={(e) => {
                                e.preventDefault()
                                setSelectedDeviceType(type.device_type)
                                setTypeInputValue(type.device_type)
                                setShowTypeDropdown(false)
                              }}
                              className={`p-3 cursor-pointer transition-colors border-b last:border-b-0 ${
                                selectedDeviceType === type.device_type
                                  ? 'bg-muted border-primary'
                                  : 'hover:bg-muted-50 border-transparent'
                              }`}
                              style={{ touchAction: 'manipulation' }}
                            >
                              <div className="flex items-center justify-between">
                                <div className="flex-1 min-w-0">
                                  <div className="flex items-center gap-2">
                                    <span className="font-medium truncate">{type.name}</span>
                                    {type.is_exact_match && (
                                      <Badge variant="default" className="text-xs h-5 shrink-0">
                                        {t('devices:pending.exactMatch')}
                                      </Badge>
                                    )}
                                  </div>
                                  <p className="text-xs text-muted-foreground truncate">{type.description}</p>
                                  <p className="text-xs text-muted-foreground mt-0.5">
                                    {type.device_type} · {type.metric_count} {t('devices:pending.metrics')}
                                  </p>
                                </div>
                                <div className="flex items-center gap-2 shrink-0 ml-3">
                                  <Badge
                                    variant={type.match_score >= 80 ? "default" : "outline"}
                                    className={type.match_score >= 80 ? "" : "border-border"}
                                  >
                                    {type.match_score}%
                                  </Badge>
                                  {selectedDeviceType === type.device_type && (
                                    <Check className="h-4 w-4 text-primary" />
                                  )}
                                </div>
                              </div>
                            </div>
                          ))}
                        </>
                      ) : (
                        <div className="p-3 text-sm text-muted-foreground text-center">
                          {t('devices:pending.noDeviceTypes')}
                        </div>
                      )}
                    </div>
                  )}
                </div>

                {/* Type selection status indicator */}
                {selectedDeviceType && (
                  <div className={`rounded-lg p-3 flex items-center gap-2 text-sm ${
                    isNewType
                      ? 'bg-warning-light border-warning text-warning'
                      : 'bg-success-light border border-success text-success'
                  }`}>
                    {isNewType ? (
                      <>
                        <span className="bg-accent-orange-light text-accent-orange text-xs px-2 py-0.5 rounded">
                          {t('devices:pending.newType')}
                        </span>
                        <span>{t('devices:pending.willCreateNewType', { type: selectedDeviceType })}</span>
                      </>
                    ) : (
                      <>
                        <Check className="h-4 w-4" />
                        <span>{t('devices:pending.usingExistingType', { type: selectedDeviceType })}</span>
                      </>
                    )}
                  </div>
                )}

                {/* Type Description field - only shown when creating a new type */}
                {selectedDeviceType && isNewType && (
                  <div>
                    <Label className="text-xs text-muted-foreground">
                      {t('devices:pending.deviceTypeName')} <span className="text-error">*</span>
                    </Label>
                    <Input
                      value={newTypeFields.type_name}
                      onChange={(e) => setNewTypeFields({ ...newTypeFields, type_name: e.target.value })}
                      placeholder={t('devices:pending.typeNamePlaceholder')}
                      className="h-9 mt-1"
                    />
                    <p className="text-xs text-muted-foreground mt-1">
                      {t('devices:pending.typeNameHint')}
                    </p>
                  </div>
                )}

                {/* Type Description field - only shown when creating a new type */}
                {selectedDeviceType && isNewType && (
                  <div>
                    <Label className="text-xs text-muted-foreground">
                      {t('devices:types.headers.description')}
                    </Label>
                    <Textarea
                      value={newTypeFields.description}
                      onChange={(e) => setNewTypeFields({ ...newTypeFields, description: e.target.value })}
                      placeholder={t('devices:pending.typeDescPlaceholder')}
                      rows={2}
                      className="mt-1"
                    />
                  </div>
                )}
              </div>
          </div>
        </UnifiedFormDialog>
      )}

      {/* Reject confirmation dialog */}
      <UnifiedFormDialog
        open={!!rejectDialogDraft}
        onOpenChange={(open) => { if (!open) setRejectDialogDraft(null) }}
        title={t('devices:pending.reject')}
        description={rejectDialogDraft ? t('devices:pending.rejectConfirm', { deviceId: rejectDialogDraft.device_id }) : undefined}
        width="sm"
        footer={
          <>
            <Button
              variant="outline"
              onClick={() => setRejectDialogDraft(null)}
              disabled={processing === rejectDialogDraft?.id}
            >
              {t('common:cancel')}
            </Button>
            <Button
              variant="destructive"
              onClick={confirmReject}
              disabled={processing === rejectDialogDraft?.id}
            >
              {processing === rejectDialogDraft?.id ? t('common:processing') : t('common:confirm')}
            </Button>
          </>
        }
      >
        <></>
      </UnifiedFormDialog>
    </>
  )
}

import { useState, useEffect, useCallback, useMemo, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { Badge } from "@/components/ui/badge"
import { ResponsiveTable, EmptyState } from "@/components/shared"
import { Cpu, Activity, Check, ChevronDown, ChevronRight, Copy, Loader2, Search as SearchIcon, Hourglass, CheckCircle2, XCircle, AlertTriangle } from "lucide-react"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleTrigger, CollapsibleContent } from "@/components/ui/collapsible"
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
import { copyToClipboard } from "@/lib/clipboard"
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
  // Keyboard-highlighted option in the type dropdown (combobox navigation)
  const [highlightedTypeIndex, setHighlightedTypeIndex] = useState(0)

  // New type additional fields (only shown when creating a new type)
  const [newTypeFields, setNewTypeFields] = useState({
    name: '',  // Device instance name
    type_name: '',  // Device type display name
    description: '',
    device_type: ''
  })

  // Inline field validation (replaces destructive toasts — the fields are
  // right in front of the user, errors belong under them)
  const [formErrors, setFormErrors] = useState<{ name?: string; type?: string }>({})

  // Registration success — shown in-dialog instead of a transient toast: the
  // recommended topic is the single most important output of this flow and
  // must survive longer than a toast, copyable.
  const [approveResult, setApproveResult] = useState<Awaited<ReturnType<typeof api.approveDraftDeviceWithType>> | null>(null)

  // Review sections (metrics / raw samples) start collapsed — the decision
  // form leads; the review content is reference material.
  const [metricsOpen, setMetricsOpen] = useState(false)
  const [samplesOpen, setSamplesOpen] = useState(false)

  const [copiedField, setCopiedField] = useState<string | null>(null)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  // Guards suggestDeviceTypes responses against interleaved dialog opens
  const suggestTokenRef = useRef(0)

  const handleCopy = useCallback(async (field: string, value: string) => {
    try {
      await copyToClipboard(value)
      setCopiedField(field)
      if (copyTimer.current) clearTimeout(copyTimer.current)
      copyTimer.current = setTimeout(() => setCopiedField(null), 1500)
    } catch {
      // clipboard blocked — silent, same policy as CopyMessageButton
    }
  }, [])

  useEffect(() => () => {
    if (copyTimer.current) clearTimeout(copyTimer.current)
  }, [])

  // Check if selected type is an existing type or a new one
  const isNewType = useMemo(() => {
    if (!selectedDeviceType) return false
    return !suggestedTypes.some(t => t.device_type === selectedDeviceType)
  }, [selectedDeviceType, suggestedTypes])

  // Combobox options: suggestions filtered by the input text, plus a
  // "create new type" entry when the typed id doesn't match an existing
  // suggestion — typing a novel id previously showed an unfiltered list that
  // ignored the query entirely.
  const typeOptions = useMemo<Array<
    | { kind: 'existing'; type: SuggestedDeviceType }
    | { kind: 'create'; value: string }
  >>(() => {
    const q = typeInputValue.trim().toLowerCase()
    const filtered = q
      ? suggestedTypes.filter(t =>
          t.name?.toLowerCase().includes(q) ||
          t.device_type.toLowerCase().includes(q) ||
          t.description?.toLowerCase().includes(q))
      : suggestedTypes
    const options: Array<
      | { kind: 'existing'; type: SuggestedDeviceType }
      | { kind: 'create'; value: string }
    > = filtered.map(type => ({ kind: 'existing', type }))
    const trimmed = typeInputValue.trim()
    if (trimmed && !suggestedTypes.some(t => t.device_type === trimmed)) {
      options.unshift({ kind: 'create', value: trimmed })
    }
    return options
  }, [typeInputValue, suggestedTypes])

  // Keep the keyboard highlight inside the (shrinking) option list
  useEffect(() => {
    setHighlightedTypeIndex(i => Math.min(i, Math.max(typeOptions.length - 1, 0)))
  }, [typeOptions.length])

  const commitTypeSelection = useCallback((option: { kind: 'existing'; type: SuggestedDeviceType } | { kind: 'create'; value: string }) => {
    const value = option.kind === 'existing' ? option.type.device_type : option.value
    setSelectedDeviceType(value)
    setTypeInputValue(value)
    setShowTypeDropdown(false)
    setFormErrors(errors => ({ ...errors, type: undefined }))
  }, [])

  const handleTypeInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!showTypeDropdown) {
      if (e.key === 'ArrowDown' || e.key === 'Enter') {
        e.preventDefault()
        setShowTypeDropdown(true)
      }
      return
    }
    if (typeOptions.length === 0) return
    const lastIndex = typeOptions.length - 1
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setHighlightedTypeIndex(i => Math.min(i + 1, lastIndex))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setHighlightedTypeIndex(i => Math.max(i - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      commitTypeSelection(typeOptions[Math.min(highlightedTypeIndex, lastIndex)])
    } else if (e.key === 'Escape') {
      e.preventDefault()
      setShowTypeDropdown(false)
    }
  }

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
    setHighlightedTypeIndex(0)
    setNewTypeFields({ name: '', type_name: '', description: '', device_type: '' })
    setFormErrors({})
    setApproveResult(null)
    setMetricsOpen(false)
    setSamplesOpen(false)

    // Initialize new type form from generated type
    if (draft.generated_type) {
      setNewTypeFields({
        device_type: draft.generated_type.device_type,
        name: draft.generated_type.name,
        type_name: draft.generated_type.name,  // Default type name to generated name
        description: draft.generated_type.description,
      })
    }

    // Fetch suggested types. Guard against interleaved opens: a slow
    // response for draft A must not auto-select A's type while draft B's
    // dialog is showing (auto-selection enables submit, so the wrong
    // draft's type would silently register). Each open bumps the token;
    // only the newest request may apply its response.
    const suggestToken = ++suggestTokenRef.current
    const applyIfCurrent = () => suggestTokenRef.current === suggestToken
    try {
      const response = await api.suggestDeviceTypes(draft.device_id)
      if (!applyIfCurrent()) return
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
      if (!applyIfCurrent()) return
      handleError(error, { operation: 'Fetch suggested types', showToast: false })
      // Show empty state on error
      setSuggestedTypes([])
    } finally {
      if (applyIfCurrent()) {
        setLoadingSuggestions(false)
      }
    }
  }

  // Validate form before submission — inline field errors, not toasts
  const validateForm = (): boolean => {
    const errors: { name?: string; type?: string } = {}
    if (!selectedDeviceType.trim()) {
      errors.type = t('devices:pending.pleaseSelectType')
    }

    // Name is always required (whether creating new type or using existing)
    if (!newTypeFields.name.trim()) {
      errors.name = t('devices:pending.pleaseEnterDeviceName')
    }

    setFormErrors(errors)
    return Object.keys(errors).length === 0
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

      // Keep the dialog open on the result — system id + recommended topic
      // are the outputs the user needs to carry away (the toast used to
      // evaporate after a few seconds with no copy affordance).
      setApproveResult(result)
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
    }
  }

  // Close the approve dialog and clear the result state
  const closeApproveDialog = useCallback(() => {
    setShowApproveDialog(false)
    setApproveResult(null)
    setSelectedDraftForApproval(null)
    setSelectedDeviceType('')
    setTypeInputValue('')
    setNewTypeFields({ name: '', type_name: '', description: '', device_type: '' })
  }, [])

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

  // Get status badge — "waiting for the user" is a calm, informational
  // state, not a warning: orange read as an error cue. Blue keeps the
  // reserved warning color for states that actually need attention
  // (offline devices, failures).
  const getStatusBadge = (status: string) => {
    const statusMap: Record<string, { color: string; label: string; icon: React.ReactNode }> = {
      collecting: { color: "bg-info-light text-info", label: t('devices:pending.status.collecting'), icon: <Loader2 className="h-4 w-4" /> },
      analyzing: { color: "bg-accent-purple-light text-accent-purple", label: t('devices:pending.status.analyzing'), icon: <SearchIcon className="h-4 w-4" /> },
      waiting_processing: { color: "bg-info-light text-info", label: t('devices:pending.status.waitingProcessing'), icon: <Hourglass className="h-4 w-4" /> },
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
          {
            key: 'actions',
            label: '',
            align: 'right',
          },
        ]}
        data={paginatedDrafts}
        rowKey={(draft: DraftDevice) => draft.id}
        loading={loading}
        emptyState={
          // The server always runs its built-in MQTT broker + webhook ingest,
          // so "no connection source" is not a real state — an empty list just
          // means nothing unknown has reported yet. (The old broker-status
          // probe also misfired whenever the broker port was merely occupied.)
          <EmptyState
            icon="inbox"
            title={t('devices:pending.noDraftsTitle')}
            description={t('devices:pending.noDraftsDesc')}
          />
        }
        renderCell={(columnKey, rowData) => {
          const draft = rowData
          const hasGeneratedType = draft.generated_type && draft.status === 'waiting_processing'
          const confidence = draft.generated_type?.confidence

          switch (columnKey) {
            case 'deviceId':
              return (
                <div className="flex items-center gap-3">
                  {/* Neutral tile — the status badge stays the single strong
                      color signal on the row (same principle as the mobile
                      card's confidence rendering). Purple marks an in-flight
                      analysis; waiting is not a colored state of the device
                      itself. */}
                  <div className={cn(
                    "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                    draft.status === 'analyzing'
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

            case 'actions':
              // Row-level actions — reject used to be reachable only from
              // inside the approve dialog's footer (the least visible spot).
              if (draft.status !== 'waiting_processing') return null
              return (
                <div className="flex items-center justify-end gap-1" onClick={(e) => e.stopPropagation()}>
                  <Button
                    size="sm"
                    onClick={() => handleApproveClick(draft)}
                    disabled={processing === draft.id}
                  >
                    {t('devices:pending.register')}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-error hover:text-error hover:bg-error-light"
                    onClick={() => handleReject(draft)}
                    disabled={processing === draft.id}
                  >
                    {t('devices:pending.reject')}
                  </Button>
                </div>
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

              {/* Row-level actions — same affordance as the desktop table's
                  actions column (the card itself opens the approve dialog). */}
              {draft.status === 'waiting_processing' && (
                <div className="flex gap-2 pt-1">
                  <Button
                    size="sm"
                    className="flex-1"
                    onClick={(e) => { e.stopPropagation(); handleApproveClick(draft) }}
                    disabled={processing === draft.id}
                  >
                    {t('devices:pending.register')}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1 text-error hover:text-error hover:bg-error-light"
                    onClick={(e) => { e.stopPropagation(); handleReject(draft) }}
                    disabled={processing === draft.id}
                  >
                    {t('devices:pending.reject')}
                  </Button>
                </div>
              )}
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
          onOpenChange={(open) => { if (!open) closeApproveDialog() }}
          title={approveResult
            ? t('devices:pending.registrationSuccess')
            : t('devices:pending.approveTitle')}
          width="2xl"
          contentClassName="overflow-y-auto"
          onSubmit={handleFinalApprove}
          isSubmitting={processing === selectedDraftForApproval.id}
          submitLabel={t('devices:pending.confirmRegister')}
          submitDisabled={!!approveResult || processing === selectedDraftForApproval.id || !selectedDeviceType.trim()}
          footer={
            approveResult ? (
              <div className="flex flex-wrap items-center gap-2 justify-end">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => { navigate('/devices/types'); closeApproveDialog() }}
                >
                  {t('devices:pending.editDeviceType')}
                </Button>
                <Button size="sm" onClick={closeApproveDialog}>
                  {t('common:done')}
                </Button>
              </div>
            ) : (
              <div className="flex flex-wrap items-center gap-2 justify-end">
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-error hover:text-error hover:bg-error-light"
                  onClick={() => {
                    closeApproveDialog()
                    if (selectedDraftForApproval) handleReject(selectedDraftForApproval)
                  }}
                  disabled={processing === selectedDraftForApproval.id}
                >
                  {t('devices:pending.reject')}
                </Button>
                <Button variant="outline" size="sm" onClick={closeApproveDialog}>
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
            )
          }
        >
          {approveResult ? (
            <div className="space-y-4">
              <div className="flex items-center gap-3">
                <div className="h-10 w-10 rounded-full bg-success-light text-success flex items-center justify-center shrink-0">
                  <CheckCircle2 className="h-5 w-5" />
                </div>
                <div className="min-w-0">
                  <p className="font-medium">{t('devices:pending.registrationSuccess')}</p>
                  <p className="text-xs text-muted-foreground truncate">{approveResult.message}</p>
                </div>
              </div>

              {/* The backend registers the device under its original id —
                  system_device_id === original_device_id by contract. Show
                  one row; split only if a future backend ever diverges. */}
              <div className="rounded-lg border border-border bg-muted-30 p-3 space-y-2.5">
                {approveResult.original_device_id === approveResult.system_device_id ? (
                  <div className="flex items-center justify-between gap-3 text-sm">
                    <span className="text-muted-foreground shrink-0">{t('devices:pending.headers.deviceId')}</span>
                    <span className="font-mono text-xs truncate text-right" title={approveResult.system_device_id}>
                      {approveResult.system_device_id}
                    </span>
                  </div>
                ) : (
                  <>
                    <div className="flex items-center justify-between gap-3 text-sm">
                      <span className="text-muted-foreground shrink-0">{t('devices:pending.originalId')}</span>
                      <span className="font-mono text-xs truncate text-right" title={approveResult.original_device_id}>
                        {approveResult.original_device_id}
                      </span>
                    </div>
                    <div className="flex items-center justify-between gap-3 text-sm">
                      <span className="text-muted-foreground shrink-0">{t('devices:pending.systemId')}</span>
                      <span className="font-mono text-xs truncate text-right" title={approveResult.system_device_id}>
                        {approveResult.system_device_id}
                      </span>
                    </div>
                  </>
                )}
                <div className="flex items-center justify-between gap-3 text-sm">
                  <span className="text-muted-foreground shrink-0">{t('devices:pending.deviceType')}</span>
                  <span className="font-mono text-xs truncate text-right" title={approveResult.device_type}>
                    {approveResult.device_type}
                  </span>
                </div>
                {/* The platform subscribes to the topic the device was found
                    publishing on — reference information for debugging, not an
                    action item: the device changes nothing. Webhook-sourced
                    devices have no real topic (the backend returns the literal
                    "webhook"), so the row is MQTT-only. */}
                {selectedDraftForApproval.source.startsWith('mqtt') && (
                  <div className="flex items-center justify-between gap-3 text-sm">
                    <span className="text-muted-foreground shrink-0">{t('devices:pending.recommendedTopic')}</span>
                    <span className="flex items-center gap-1.5 min-w-0">
                      <span className="font-mono text-xs truncate" title={approveResult.recommended_topic}>
                        {approveResult.recommended_topic}
                      </span>
                      <button
                        type="button"
                        onClick={() => handleCopy('topic', approveResult.recommended_topic)}
                        className="text-muted-foreground hover:text-foreground shrink-0"
                        aria-label={t('common:copy')}
                      >
                        {copiedField === 'topic'
                          ? <Check className="h-3.5 w-3.5 text-success" />
                          : <Copy className="h-3.5 w-3.5" />}
                      </button>
                    </span>
                  </div>
                )}
              </div>

              {selectedDraftForApproval.source.startsWith('mqtt') && (
                <p className="text-xs text-muted-foreground">
                  {t('devices:pending.recommendedTopicHint')}
                </p>
              )}
            </div>
          ) : (
          <div className="space-y-5">
            {/* ── Registration form — the decision fields lead; review
                  context follows below, collapsed by default ── */}

            {/* Device Name */}
            <div>
              <Label className="text-xs text-muted-foreground">
                {t('devices:pending.deviceName')} <span className="text-error">*</span>
              </Label>
              <Input
                value={newTypeFields.name}
                onChange={(e) => {
                  setNewTypeFields({ ...newTypeFields, name: e.target.value })
                  if (formErrors.name) setFormErrors(errors => ({ ...errors, name: undefined }))
                }}
                placeholder={t('devices:pending.deviceNamePlaceholder')}
                aria-invalid={!!formErrors.name}
                className="h-9 mt-1"
              />
              {formErrors.name && (
                <p className="text-xs text-error mt-1">{formErrors.name}</p>
              )}
            </div>

            {/* Device Type Selection */}
            <div className="space-y-2.5">
              <Label className="text-xs text-muted-foreground">
                {t('devices:pending.deviceTypeSelection')} <span className="text-error">*</span>
              </Label>

              {/* Filterable, keyboard-navigable combobox. Container-level blur
                  (relatedTarget check) replaces the old setTimeout(200) race. */}
              <div
                className="relative"
                onBlur={(e) => {
                  if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
                    setShowTypeDropdown(false)
                  }
                }}
              >
                <div className="relative">
                  <Input
                    value={typeInputValue}
                    role="combobox"
                    aria-expanded={showTypeDropdown}
                    aria-autocomplete="list"
                    aria-controls="device-type-options"
                    aria-activedescendant={showTypeDropdown && typeOptions.length > 0
                      ? `device-type-option-${Math.min(highlightedTypeIndex, typeOptions.length - 1)}`
                      : undefined}
                    onChange={(e) => {
                      const value = e.target.value
                      setTypeInputValue(value)
                      setSelectedDeviceType(value)
                      setShowTypeDropdown(true)
                      setHighlightedTypeIndex(0)
                      setFormErrors(errors => ({ ...errors, type: undefined }))
                    }}
                    onFocus={() => setShowTypeDropdown(true)}
                    onKeyDown={handleTypeInputKeyDown}
                    placeholder={t('devices:pending.typeInputPlaceholder')}
                    aria-invalid={!!formErrors.type}
                    className="pr-10"
                  />
                  <button
                    type="button"
                    tabIndex={-1}
                    onClick={() => setShowTypeDropdown(!showTypeDropdown)}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    aria-label={t('devices:pending.deviceTypeSelection')}
                  >
                    <ChevronDown className={cn(
                      'h-4 w-4 transition-transform',
                      showTypeDropdown && 'rotate-180'
                    )} />
                  </button>
                </div>

                {showTypeDropdown && (
                  <div
                    id="device-type-options"
                    role="listbox"
                    className="absolute z-10 w-full mt-1 bg-background border border-border rounded-lg shadow-lg max-h-64 overflow-y-auto"
                  >
                    {loadingSuggestions ? (
                      <div className="p-3 text-sm text-muted-foreground text-center">
                        {t('common:loading')}...
                      </div>
                    ) : typeOptions.length > 0 ? (
                      typeOptions.map((option, index) => {
                        const highlighted = index === Math.min(highlightedTypeIndex, typeOptions.length - 1)
                        return (
                          <div
                            key={option.kind === 'existing' ? option.type.device_type : `create-${option.value}`}
                            id={`device-type-option-${index}`}
                            role="option"
                            aria-selected={highlighted}
                            // Keep focus in the input so the container blur
                            // check can't race the click.
                            onMouseDown={(e) => e.preventDefault()}
                            onTouchEnd={(e) => e.preventDefault()}
                            onClick={() => commitTypeSelection(option)}
                            onMouseEnter={() => setHighlightedTypeIndex(index)}
                            className={cn(
                              'p-3 cursor-pointer transition-colors border-b last:border-b-0',
                              highlighted ? 'bg-muted border-primary' : 'hover:bg-muted-50 border-transparent'
                            )}
                            style={{ touchAction: 'manipulation' }}
                          >
                            {option.kind === 'create' ? (
                              <div className="flex items-center gap-2">
                                <Check className="h-4 w-4 text-primary shrink-0" />
                                <span className="text-sm">
                                  {t('devices:pending.createTypeOption', { value: option.value })}
                                </span>
                              </div>
                            ) : (
                              <div className="flex items-center justify-between">
                                <div className="flex-1 min-w-0">
                                  <div className="flex items-center gap-2">
                                    <span className="font-medium truncate">{option.type.name}</span>
                                    {option.type.is_exact_match && (
                                      <Badge variant="default" className="text-xs h-5 shrink-0">
                                        {t('devices:pending.exactMatch')}
                                      </Badge>
                                    )}
                                  </div>
                                  <p className="text-xs text-muted-foreground truncate">{option.type.description}</p>
                                  <p className="text-xs text-muted-foreground mt-0.5">
                                    {option.type.device_type} · {option.type.metric_count} {t('devices:pending.metrics')}
                                  </p>
                                </div>
                                <div className="flex items-center gap-2 shrink-0 ml-3">
                                  <Badge
                                    variant={option.type.match_score >= 80 ? 'default' : 'outline'}
                                    className={option.type.match_score >= 80 ? '' : 'border-border'}
                                  >
                                    {option.type.match_score}%
                                  </Badge>
                                  {selectedDeviceType === option.type.device_type && (
                                    <Check className="h-4 w-4 text-primary" />
                                  )}
                                </div>
                              </div>
                            )}
                          </div>
                        )
                      })
                    ) : (
                      <div className="p-3 text-sm text-muted-foreground text-center">
                        {t('devices:pending.noDeviceTypes')}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {formErrors.type && (
                <p className="text-xs text-error">{formErrors.type}</p>
              )}

              {/* Type selection status indicator */}
              {selectedDeviceType && (
                <div className={`rounded-lg p-3 flex items-center gap-2 text-sm ${
                  isNewType
                    ? 'bg-warning-light border border-warning text-warning'
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

              {/* Type display name — only when creating a new type */}
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

              {/* Type description — only when creating a new type */}
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

            {/* ── Context: device facts (single occurrence of this header —
                  it used to appear twice, once per section) ── */}
            <div className="space-y-2">
              <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
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

            {/* ── Review: AI-inferred metrics — reference for the type
                  decision; edits belong to the type manager after
                  registration (the in-dialog editor never reached the
                  API, so it is gone instead of lying) ── */}
            {selectedDraftForApproval.generated_type && (
              <Collapsible open={metricsOpen} onOpenChange={setMetricsOpen}>
                <CollapsibleTrigger className="w-full flex items-center justify-between py-1 text-left">
                  <span className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                    {t('devices:pending.metrics')} ({selectedDraftForApproval.generated_type.metrics.length})
                  </span>
                  <ChevronRight className={cn(
                    'h-4 w-4 text-muted-foreground transition-transform',
                    metricsOpen && 'rotate-90'
                  )} />
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <div className="pt-2 space-y-2">
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
                          {(selectedDraftForApproval.generated_type.metrics || []).map((metric) => (
                            <TableRow key={metric.name}>
                              <TableCell className="font-mono text-xs">{metric.path}</TableCell>
                              <TableCell>{metric.display_name}</TableCell>
                              <TableCell>
                                <span className="text-xs capitalize">{metric.data_type || 'string'}</span>
                              </TableCell>
                              <TableCell>{metric.unit || '-'}</TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {t('devices:pending.metricsEditHint')}
                    </p>
                  </div>
                </CollapsibleContent>
              </Collapsible>
            )}

            {/* ── Review: raw samples ── */}
            {selectedDraftForApproval.samples && selectedDraftForApproval.samples.length > 0 && (
              <Collapsible open={samplesOpen} onOpenChange={setSamplesOpen}>
                <CollapsibleTrigger className="w-full flex items-center justify-between py-1 text-left">
                  <span className="text-xs font-semibold text-muted-foreground uppercase tracking-wide">
                    {t('devices:pending.originalData')} ({selectedDraftForApproval.samples.length})
                  </span>
                  <ChevronRight className={cn(
                    'h-4 w-4 text-muted-foreground transition-transform',
                    samplesOpen && 'rotate-90'
                  )} />
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <div className="pt-2">
                    <div className="bg-muted-30 rounded-lg p-3">
                      <div className="flex items-center gap-2 mb-3">
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
                        {selectedDraftForApproval.samples[selectedSampleIndex] && (
                          <span className="text-xs text-muted-foreground ml-auto">
                            {formatTimestamp(selectedDraftForApproval.samples[selectedSampleIndex].timestamp, false)}
                          </span>
                        )}
                      </div>
                      {selectedDraftForApproval.samples[selectedSampleIndex]?.parsed && (
                        <div className="relative">
                          <button
                            type="button"
                            onClick={() => handleCopy(
                              'sample',
                              JSON.stringify(selectedDraftForApproval.samples?.[selectedSampleIndex]?.parsed ?? '', null, 2)
                            )}
                            className="absolute right-2 top-2 text-muted-foreground hover:text-foreground z-10"
                            aria-label={t('common:copy')}
                          >
                            {copiedField === 'sample'
                              ? <Check className="h-3.5 w-3.5 text-success" />
                              : <Copy className="h-3.5 w-3.5" />}
                          </button>
                          <pre className="text-xs bg-background p-3 rounded overflow-x-auto">
                            {JSON.stringify(selectedDraftForApproval.samples[selectedSampleIndex].parsed, null, 2)}
                          </pre>
                        </div>
                      )}
                    </div>
                  </div>
                </CollapsibleContent>
              </Collapsible>
            )}
          </div>
          )}

        </UnifiedFormDialog>
      )}

      {/* Reject confirmation dialog */}
      <UnifiedFormDialog
        open={!!rejectDialogDraft}
        onOpenChange={(open) => { if (!open) setRejectDialogDraft(null) }}
        title={t('devices:pending.reject')}
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
        {/* The confirm sentence lives in the body — it used to ride the
            header `description` prop, which the desktop dialog dropped
            entirely, leaving this dialog visually empty. */}
        {rejectDialogDraft ? (
          <p className="text-sm text-muted-foreground">
            {t('devices:pending.rejectConfirm', { deviceId: rejectDialogDraft.device_id })}
          </p>
        ) : null}
      </UnifiedFormDialog>
    </>
  )
}

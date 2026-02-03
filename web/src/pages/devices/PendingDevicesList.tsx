import { useState, useEffect, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { Badge } from "@/components/ui/badge"
import { ResponsiveTable } from "@/components/shared"
import { Eye, Cpu, Globe, Badge as BadgeIcon, Clock, Activity, Check, ChevronDown } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Button } from "@/components/ui/button"
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
import type { DraftDevice, SuggestedDeviceType } from "@/types"

interface PendingDevicesListProps {
  onRefresh?: () => void
}

export function PendingDevicesList({ onRefresh }: PendingDevicesListProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { handleError } = useErrorHandler()
  const { toast } = useToast()

  const [drafts, setDrafts] = useState<DraftDevice[]>([])
  const [loading, setLoading] = useState(true)

  // Pagination state
  const [page, setPage] = useState(1)
  const itemsPerPage = 10

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
      setEditingMetrics(draft.generated_type.metrics.map(m => ({
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
      console.log('Suggested types response:', response)
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

  return (
    <>
      <ResponsiveTable
        columns={[
          {
            key: 'index',
            label: '#',
            width: 'w-10',
            align: 'center',
          },
          {
            key: 'deviceId',
            label: (
              <div className="flex items-center gap-2">
                <Cpu className="h-4 w-4" />
                {t('devices:pending.headers.deviceId')}
              </div>
            ),
          },
          {
            key: 'source',
            label: (
              <div className="flex items-center gap-2">
                <Globe className="h-4 w-4" />
                {t('devices:pending.headers.source')}
              </div>
            ),
          },
          {
            key: 'deviceType',
            label: (
              <div className="flex items-center gap-2">
                <Activity className="h-4 w-4" />
                {t('devices:pending.deviceType')}
              </div>
            ),
          },
          {
            key: 'status',
            label: (
              <div className="flex items-center gap-2">
                <BadgeIcon className="h-4 w-4" />
                {t('devices:pending.headers.status')}
              </div>
            ),
            align: 'center',
          },
          {
            key: 'metrics',
            label: (
              <div className="flex items-center gap-2">
                <BadgeIcon className="h-4 w-4" />
                {t('devices:pending.metrics')}
              </div>
            ),
            align: 'center',
          },
          {
            key: 'discoveredAt',
            label: (
              <div className="flex items-center gap-2">
                <Clock className="h-4 w-4" />
                {t('devices:pending.headers.discoveredAt')}
              </div>
            ),
            align: 'center',
          },
        ]}
        data={paginatedDrafts as unknown as Record<string, unknown>[]}
        rowKey={(draft) => (draft as unknown as DraftDevice).id}
        loading={loading}
        emptyState={
          <div className="flex items-center justify-center py-12">
            <p className="text-muted-foreground">{t('devices:pending.noPending')}</p>
          </div>
        }
        renderCell={(columnKey, rowData) => {
          const draft = rowData as unknown as DraftDevice
          const index = paginatedDrafts.indexOf(draft)
          const hasGeneratedType = draft.generated_type && draft.status === 'waiting_processing'
          const confidence = draft.generated_type?.confidence

          switch (columnKey) {
            case 'index':
              return (
                <span className="text-xs text-muted-foreground font-medium">
                  {index + 1}
                </span>
              )

            case 'deviceId':
              return (
                <div className="flex items-center gap-3">
                  <div className={cn(
                    "w-9 h-9 rounded-lg flex items-center justify-center transition-colors",
                    draft.status === 'waiting_processing'
                      ? "bg-amber-500/10 text-amber-600"
                      : draft.status === 'analyzing'
                        ? "bg-purple-500/10 text-purple-600"
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
                            ? "bg-green-500/20 text-green-700 border-green-200"
                            : "bg-amber-500/20 text-amber-700 border-amber-200"
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
              return getStatusBadge(draft.status)

            case 'metrics':
              return hasGeneratedType ? (
                <div className="flex justify-center">
                  <Badge variant="outline" className="text-xs bg-blue-50 text-blue-700 border-blue-200 dark:bg-blue-950/30 dark:text-blue-400 dark:border-blue-800">
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
        actions={[
          {
            label: t('devices:pending.process'),
            icon: <Eye className="h-4 w-4" />,
            onClick: (rowData) => {
              const draft = rowData as unknown as DraftDevice
              handleApproveClick(draft)
            },
          },
        ]}
      />

      {/* Summary footer showing registered count */}
      {registeredCount > 0 && (
        <div className="mt-4 flex items-center justify-center gap-4 text-sm text-muted-foreground">
          <span className="flex items-center gap-1">
            <Badge variant="outline" className="bg-green-500/10 text-green-700">
              {registeredCount}
            </Badge>
            <span>{t('devices:pending.registeredHidden')}</span>
          </span>
        </div>
      )}

      {/* Unified Approval/Details Dialog */}
      {showApproveDialog && selectedDraftForApproval && (
        <Dialog open={showApproveDialog} onOpenChange={setShowApproveDialog}>
          <DialogContent className="max-w-3xl max-h-[85vh] overflow-hidden flex flex-col">
            <DialogHeader className="pb-2">
              <DialogTitle>{t('devices:pending.approveTitle')}</DialogTitle>
            </DialogHeader>

            <div className="flex-1 overflow-y-auto space-y-6 py-2">
              {/* Device Info Section */}
              <div className="space-y-3">
                <h3 className="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
                  {t('devices:pending.deviceInfo')}
                </h3>
                <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm bg-muted/30 rounded-lg p-4">
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
                      <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => setIsEditingMetrics(true)}>
                        {t('common:edit')}
                      </Button>
                    ) : (
                      <div className="flex gap-1">
                        <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => {
                          setIsEditingMetrics(false)
                          // Reset to original values
                          if (selectedDraftForApproval.generated_type) {
                            setEditingMetrics(selectedDraftForApproval.generated_type.metrics.map(m => ({
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
                        <Button variant="default" size="sm" className="h-7 text-xs" onClick={() => setIsEditingMetrics(false)}>
                          {t('common:save')}
                        </Button>
                      </div>
                    )}
                  </div>
                  <div className="border rounded-lg overflow-hidden">
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
                        {(isEditingMetrics ? editingMetrics : selectedDraftForApproval.generated_type.metrics).map((metric, idx) => (
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
                                <select
                                  value={metric.data_type}
                                  onChange={(e) => {
                                    const updated = [...editingMetrics]
                                    updated[idx].data_type = e.target.value
                                    setEditingMetrics(updated)
                                  }}
                                  className="h-7 text-xs bg-background border rounded px-2"
                                >
                                  <option value="string">String</option>
                                  <option value="integer">Integer</option>
                                  <option value="float">Float</option>
                                  <option value="boolean">Boolean</option>
                                </select>
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
                  <div className="bg-muted/30 rounded-lg p-3">
                    <div className="flex gap-1 mb-3">
                      {selectedDraftForApproval.samples.slice(0, 5).map((_, index) => (
                        <button
                          key={index}
                          onClick={() => setSelectedSampleIndex(index)}
                          className={`w-7 h-7 text-xs rounded ${
                            selectedSampleIndex === index
                              ? 'bg-primary text-primary-foreground'
                              : 'bg-background hover:bg-muted'
                          }`}
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
                    {t('devices:pending.deviceName')} <span className="text-red-500">*</span>
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
                  <span className="text-red-500 ml-1">*</span>
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
                              onMouseDown={() => {
                                setSelectedDeviceType(type.device_type)
                                setTypeInputValue(type.device_type)
                                setShowTypeDropdown(false)
                              }}
                              className={`p-3 cursor-pointer transition-colors border-b last:border-b-0 ${
                                selectedDeviceType === type.device_type
                                  ? 'bg-primary/10 border-primary'
                                  : 'hover:bg-muted/50 border-transparent'
                              }`}
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
                                  <p className="text-xs text-muted-foreground/70 mt-0.5">
                                    {type.device_type} · {type.metric_count} {t('devices:pending.metrics')}
                                  </p>
                                </div>
                                <div className="flex items-center gap-2 shrink-0 ml-3">
                                  <Badge
                                    variant={type.match_score >= 80 ? "default" : "outline"}
                                    className={type.match_score >= 80 ? "" : "border-muted-foreground/30"}
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
                      ? 'bg-amber-50 dark:bg-amber-950/20 border border-amber-200 dark:border-amber-800 text-amber-700 dark:text-amber-400'
                      : 'bg-green-50 dark:bg-green-950/20 border border-green-200 dark:border-green-800 text-green-700 dark:text-green-400'
                  }`}>
                    {isNewType ? (
                      <>
                        <span className="bg-amber-200 dark:bg-amber-800 text-amber-900 dark:text-amber-100 text-xs px-2 py-0.5 rounded">
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
                      {t('devices:pending.deviceTypeName')} <span className="text-red-500">*</span>
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

            <DialogFooter className="gap-2 pt-4 border-t">
              <Button
                variant="ghost"
                className="text-red-600 hover:text-red-700 hover:bg-red-50"
                onClick={() => {
                  setShowApproveDialog(false)
                  if (selectedDraftForApproval) handleReject(selectedDraftForApproval)
                }}
                disabled={processing === selectedDraftForApproval.id}
              >
                {t('devices:pending.reject')}
              </Button>
              <Button variant="outline" onClick={() => setShowApproveDialog(false)}>
                {t('common:cancel')}
              </Button>
              <Button
                onClick={handleFinalApprove}
                disabled={processing === selectedDraftForApproval.id || !selectedDeviceType.trim()}
              >
                {processing === selectedDraftForApproval.id ? t('common:processing') : t('devices:pending.confirmRegister')}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}

      {/* Reject confirmation dialog */}
      <Dialog open={!!rejectDialogDraft} onOpenChange={(open: boolean) => !open && setRejectDialogDraft(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t('devices:pending.reject')}</DialogTitle>
            <DialogDescription>
              {rejectDialogDraft && t('devices:pending.rejectConfirm', { deviceId: rejectDialogDraft.device_id })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2">
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
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

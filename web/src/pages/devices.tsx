import { useEffect, useState, useRef, useCallback, useMemo } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { shallow } from "zustand/shallow"
import { useToast } from "@/hooks/use-toast"
import { useEvents } from "@/hooks/useEvents"
import { useVisiblePolling } from "@/hooks/useVisiblePolling"
import { useIsMobile } from "@/hooks/useMobile"
import { confirm } from "@/hooks/use-confirm"
import { fetchCache } from "@/lib/utils/async"
import { useNavigate, useLocation, useParams } from "react-router-dom"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabsBar, PageTabsContent, PageTabsBottomNav, Pagination, type TabAction } from "@/components/shared"
import { Upload, Download, Settings, Server, Layers, FileEdit, Cloud, ChevronLeft } from "lucide-react"
import { LoadingState } from "@/components/shared/LoadingState"
import { Button } from "@/components/ui/button"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { Switch } from "@/components/ui/switch"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { api } from "@/lib/api"
import type { Device, DeviceType } from "@/types"
import {
  DeviceList,
  DeviceDetail,
  AddDeviceDialog,
  EditDeviceDialog,
  DeviceTypeList,
  AddDeviceTypeDialog,
  ViewDeviceTypeDialog,
  EditDeviceTypeDialog,
} from "./devices/index"
import { CloudImportDialog } from "@/pages/devices/DeviceTypeDialogs"
import { DeviceTypeGeneratorDialog } from "@/components/devices/DeviceTypeGeneratorDialog"
import { PendingDevicesList } from "./devices/PendingDevicesList"
import { AddDeviceGlobalDialog } from "./devices/AddDeviceGlobalDialog"
import { useErrorHandler } from "@/hooks/useErrorHandler"

type DeviceTabValue = "devices" | "types" | "drafts"

export function DevicesPage() {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const { handleError, withErrorHandling } = useErrorHandler()
  const { id: urlDeviceId } = useParams<{ id?: string }>()
  const isMobile = useIsMobile()

  // Group device data selectors (change together)
  const { devices, devicesLoading } = useStore((s) => ({
    devices: s.devices,
    devicesLoading: s.devicesLoading,
  }), shallow)

  // Group device type data selectors (change together)
  const { deviceTypes, deviceTypesLoading } = useStore((s) => ({
    deviceTypes: s.deviceTypes,
    deviceTypesLoading: s.deviceTypesLoading,
  }), shallow)

  // Group dialog selectors
  const { addDeviceDialogOpen, setAddDeviceDialogOpen } = useStore((s) => ({
    addDeviceDialogOpen: s.addDeviceDialogOpen,
    setAddDeviceDialogOpen: s.setAddDeviceDialogOpen,
  }), shallow)

  // Group detail view data selectors (change together when viewing a device)
  const { deviceTypeDetails, deviceDetails, telemetryData, telemetrySummary, deviceCurrentState, telemetryLoading } = useStore((s) => ({
    deviceTypeDetails: s.deviceTypeDetails,
    deviceDetails: s.deviceDetails,
    telemetryData: s.telemetryData,
    telemetrySummary: s.telemetrySummary,
    deviceCurrentState: s.deviceCurrentState,
    telemetryLoading: s.telemetryLoading,
  }), shallow)

  // Action selectors (stable references, no need for shallow)
  const fetchDevices = useStore((s) => s.fetchDevices)
  const fetchDeviceDetails = useStore((s) => s.fetchDeviceDetails)
  const fetchDeviceTypeDetails = useStore((s) => s.fetchDeviceTypeDetails)
  const addDevice = useStore((s) => s.addDevice)
  const updateDevice = useStore((s) => s.updateDevice)
  const deleteDevice = useStore((s) => s.deleteDevice)
  const fetchDeviceTypes = useStore((s) => s.fetchDeviceTypes)
  const addDeviceType = useStore((s) => s.addDeviceType)
  const deleteDeviceType = useStore((s) => s.deleteDeviceType)
  const validateDeviceType = useStore((s) => s.validateDeviceType)
  const sendCommand = useStore((s) => s.sendCommand)
  const fetchTelemetryData = useStore((s) => s.fetchTelemetryData)
  const fetchTelemetrySummary = useStore((s) => s.fetchTelemetrySummary)
  const fetchDeviceCurrentState = useStore((s) => s.fetchDeviceCurrentState)
  const clearDeviceDetails = useStore((s) => s.clearDeviceDetails)
  const updateDeviceStatus = useStore((s) => s.updateDeviceStatus)
  const updateDeviceTransportStatus = useStore((s) => s.updateDeviceTransportStatus)

  // Pagination state
  const [devicePage, setDevicePage] = useState(1)
  const devicesPerPage = 10

  // Device type pagination state
  const [deviceTypePage, setDeviceTypePage] = useState(1)
  const deviceTypesPerPage = 10

  // Draft devices pagination state
  const [draftPage, setDraftPage] = useState(1)
  const draftsPerPage = 10
  const [draftsCount, setDraftsCount] = useState(0)

  // Auto-onboarding configuration (simplified to 3 fields)
  interface OnboardConfig {
    enabled: boolean
    max_samples: number
    draft_retention_secs: number
  }
  const [onboardConfig, setOnboardConfig] = useState<OnboardConfig>({
    enabled: true,
    max_samples: 10,
    draft_retention_secs: 86400, // 24 hours
  })
  const [pendingOnboardConfig, setPendingOnboardConfig] = useState<OnboardConfig>(onboardConfig)
  const [showOnboardConfigDialog, setShowOnboardConfigDialog] = useState(false)
  const [savingOnboardConfig, setSavingOnboardConfig] = useState(false)

  // Fetch auto-onboarding configuration
  const fetchOnboardConfig = async () => {
    const result = await withErrorHandling(
      () => api.getOnboardConfig(),
      { operation: 'Fetch onboard config', showToast: false }
    )
    if (result) {
      setOnboardConfig(result)
      setPendingOnboardConfig(result)
    }
  }

  // Save auto-onboarding configuration
  const saveOnboardConfig = async () => {
    setSavingOnboardConfig(true)
    try {
      await api.updateOnboardConfig(pendingOnboardConfig)
      setOnboardConfig(pendingOnboardConfig)
      toast({
        title: t('common:success'),
        description: t('devices:pending.configSaved'),
      })
      setShowOnboardConfigDialog(false)
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: t('devices:pending.configSaveFailed'),
        variant: 'destructive'
      })
    } finally {
      setSavingOnboardConfig(false)
    }
  }

  // Open config dialog and fetch current config
  const openOnboardConfigDialog = async () => {
    await fetchOnboardConfig()
    setShowOnboardConfigDialog(true)
  }

  // Router integration
  const navigate = useNavigate()
  const location = useLocation()

  // Get tab from URL path
  const getTabFromPath = (): DeviceTabValue => {
    const pathSegments = location.pathname.split('/')
    const lastSegment = pathSegments[pathSegments.length - 1]

    // If there's a deviceId parameter (detail view), always return 'devices' tab
    if (urlDeviceId) {
      return 'devices'
    }

    // Otherwise check for known tab values
    if (lastSegment === 'types' || lastSegment === 'drafts') {
      return lastSegment as DeviceTabValue
    }
    return 'devices'
  }

  // Active tab state - sync with URL
  const [activeTab, setActiveTab] = useState<DeviceTabValue>(getTabFromPath)

  // Update tab when URL changes
  useEffect(() => {
    const tabFromPath = getTabFromPath()
    setActiveTab(tabFromPath)
  }, [location.pathname])

  // Update URL when tab changes
  const handleTabChange = (tab: DeviceTabValue) => {
    setActiveTab(tab)
    if (tab === 'devices') {
      navigate('/devices')
    } else {
      navigate(`/devices/${tab}`)
    }
  }

  // Reset device type pagination when data changes
  useEffect(() => {
    setDeviceTypePage(1)
  }, [deviceTypes.length])

  // Paginated device types
  // On mobile: show cumulative data (all pages up to current)
  // On desktop: show only current page
  const paginatedDeviceTypes = useMemo(() => {
    if (isMobile) {
      return deviceTypes.slice(0, deviceTypePage * deviceTypesPerPage)
    } else {
      return deviceTypes.slice(
        (deviceTypePage - 1) * deviceTypesPerPage,
        deviceTypePage * deviceTypesPerPage
      )
    }
  }, [deviceTypes, deviceTypePage, deviceTypesPerPage, isMobile])

  // Reset pagination when data changes
  useEffect(() => {
    setDevicePage(1)
  }, [devices.length])

  // Reset drafts pagination when switching to drafts tab
  useEffect(() => {
    if (activeTab === 'drafts') {
      setDraftPage(1)
    }
  }, [activeTab])

  // Paginated devices
  // On mobile: show cumulative data (all pages up to current)
  // On desktop: show only current page
  const paginatedDevices = useMemo(() => {
    if (isMobile) {
      return devices.slice(0, devicePage * devicesPerPage)
    } else {
      return devices.slice(
        (devicePage - 1) * devicesPerPage,
        devicePage * devicesPerPage
      )
    }
  }, [devices, devicePage, devicesPerPage, isMobile])

  // Device detail view state
  const [deviceDetailView, setDeviceDetailView] = useState<string | null>(null)
  const [selectedMetric, setSelectedMetric] = useState<string | null>(null)

  // Fetch devices when component mounts
  // Store-level TTL cache prevents redundant API calls on remount
  useEffect(() => {
    fetchDevices()
  }, [fetchDevices])

  // Fetch device types when types tab is accessed
  useEffect(() => {
    if (activeTab === 'types') {
      fetchDeviceTypes()
    }
  }, [activeTab, fetchDeviceTypes])

  // Load device from URL parameter
  useEffect(() => {
    if (!urlDeviceId) {
      // Clear detail view when URL has no deviceId
      setDeviceDetailView((prev) => {
        if (prev) setSelectedMetric(null)
        return null
      })
      return
    }

    let cancelled = false

    const loadDevice = async () => {
      // Clear stale data immediately to prevent flash of previous device
      clearDeviceDetails()

      // Find device in list first (synchronous)
      let device = devices.find(d => d.id === urlDeviceId)

      // Fetch from API if not in list
      if (!device && !devicesLoading) {
        device = await withErrorHandling(
          () => api.getDevice(urlDeviceId),
          { operation: 'Load device from URL', showToast: false }
        ) ?? undefined
      }

      if (cancelled) return

      if (device) {
        setSelectedMetric(null)
        setDeviceDetailView(urlDeviceId)
        await Promise.all([
          fetchDeviceDetails(urlDeviceId),
          fetchDeviceTypeDetails(device.device_type),
          fetchDeviceCurrentState(urlDeviceId),
        ])
      } else {
        // Device not found but still set view to show error state
        setDeviceDetailView(urlDeviceId)
      }
    }

    loadDevice()

    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [urlDeviceId])

  // Debounced refresh to prevent excessive API calls
  const refreshDevicesRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const refreshDeviceTypesRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const debouncedFetchDevices = useCallback(() => {
    if (refreshDevicesRef.current) {
      clearTimeout(refreshDevicesRef.current)
    }
    refreshDevicesRef.current = setTimeout(() => {
      fetchCache.invalidate('devices')
      fetchDevices()
    }, 500) // 500ms debounce
  }, [fetchDevices])

  const debouncedFetchDeviceTypes = useCallback(() => {
    if (refreshDeviceTypesRef.current) {
      clearTimeout(refreshDeviceTypesRef.current)
    }
    refreshDeviceTypesRef.current = setTimeout(() => {
      fetchCache.invalidate('deviceTypes')
      fetchDeviceTypes()
    }, 300) // 300ms debounce for device types
  }, [fetchDeviceTypes])

  // Cleanup timeouts on unmount
  useEffect(() => {
    return () => {
      if (refreshDevicesRef.current) clearTimeout(refreshDevicesRef.current)
      if (refreshDeviceTypesRef.current) clearTimeout(refreshDeviceTypesRef.current)
    }
  }, [])

  // WebSocket event handler for device status changes
  const handleDeviceEvent = useCallback((event: { type: string; data: unknown }) => {
    const data = event.data as { device_id?: string; id?: string }
    const deviceId = data?.device_id || data?.id
    switch (event.type) {
      case 'DeviceOnline':
        // Optimistic update for instant UI feedback
        if (deviceId) updateDeviceStatus(deviceId, 'online')
        break
      case 'DeviceOffline':
        // Optimistic update for instant UI feedback
        if (deviceId) updateDeviceStatus(deviceId, 'offline')
        break
      case 'DeviceTransportOnline':
        if (deviceId) updateDeviceTransportStatus(deviceId, true)
        break
      case 'DeviceTransportOffline':
        if (deviceId) updateDeviceTransportStatus(deviceId, false)
        break
      case 'DeviceRegistered':
      case 'DeviceUnregistered':
        // Device list changed - force refresh (bypass TTL cache)
        debouncedFetchDevices()
        break
      case 'DeviceTypeRegistered':
      case 'DeviceTypeUnregistered':
        // Device type list changed - force refresh
        debouncedFetchDeviceTypes()
        break
      case 'DeviceMetric':
        // Don't refresh on every metric - too frequent
        break
      case 'DeviceCommandResult':
        // Command completed - force refresh to see updated state
        debouncedFetchDevices()
        break
    }
  }, [updateDeviceStatus, updateDeviceTransportStatus, debouncedFetchDevices, debouncedFetchDeviceTypes])

  // Subscribe to device events for real-time updates
  const { isConnected: deviceEventsConnected } = useEvents({
    enabled: !deviceDetailView, // Only when not in detail view
    category: 'device',
    onEvent: handleDeviceEvent,
  })

  // Fallback polling when WebSocket is not connected (only when not in detail view)
  // Pauses when tab is hidden, resumes with immediate refresh when visible
  useVisiblePolling(
    fetchDevices,
    30000,
    !deviceDetailView && !deviceEventsConnected,
  )

  // Refresh device details periodically in detail view (keeps status badge fresh)
  useVisiblePolling(
    async () => {
      if (deviceDetailView) {
        await Promise.all([
          fetchDeviceDetails(deviceDetailView),
          fetchDeviceCurrentState(deviceDetailView),
        ])
      }
    },
    15000,
    !!deviceDetailView,
  )

  // Handlers
  const handleAddDevice = async (request: import('@/types').AddDeviceRequest) => {
    setAddingDevice(true)
    try {
      return await addDevice(request)
    } finally {
      setAddingDevice(false)
    }
  }

  const handleDeleteDevice = async (id: string) => {
    const confirmed = await confirm({
      title: t('common:delete'),
      description: t('devices:deleteConfirm'),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    await deleteDevice(id)
    toast({ title: t('common:success'), description: t('devices:deviceDeleted') })
  }

  const handleOpenDeviceDetails = (device: Device) => {
    // Only navigate — the [urlDeviceId] useEffect is the single source of truth
    // for loading device data. Doing fetches here too causes a race that can
    // leave the detail view blank on the first click.
    navigate(`/devices/${device.id}`)
  }

  const handleCloseDeviceDetail = () => {
    // Navigate back to devices list
    navigate('/devices')
    setDeviceDetailView(null)
    setSelectedMetric(null)
    // Clear stale detail data so reopening doesn't flash the previous device
    clearDeviceDetails()
  }

  const handleRefreshDeviceDetail = async () => {
    if (deviceDetailView) {
      if (selectedMetric) {
        const end = Math.floor(Date.now() / 1000)
        const start = end - 30 * 24 * 60 * 60
        await Promise.all([
          fetchDeviceDetails(deviceDetailView),
          fetchDeviceCurrentState(deviceDetailView),
          fetchTelemetryData(deviceDetailView, selectedMetric, start, end, 1000),
        ])
      } else {
        await Promise.all([
          fetchDeviceDetails(deviceDetailView),
          fetchDeviceCurrentState(deviceDetailView),
        ])
      }
    }
  }

  const handleMetricClick = async (metricName: string, offset?: number, limit?: number) => {
    if (!deviceDetailView) return
    setSelectedMetric(metricName)
    // Use current timestamp as end to ensure we get the latest data
    const end = Math.floor(Date.now() / 1000)
    // Max 30 days to match backend limit (MAX_TIME_RANGE_SECS = 30 * 86400)
    const start = end - 30 * 24 * 60 * 60
    // Fetch with pagination support
    await fetchTelemetryData(deviceDetailView, metricName, start, end, limit ?? 50, offset ?? 0)
  }

  const handleSendCommand = async (commandName: string, paramsJson: string) => {
    if (!deviceDetailView) return

    try {
      let params: Record<string, unknown> = {}
      if (paramsJson.trim()) {
        try {
          params = JSON.parse(paramsJson)
        } catch {
          toast({ title: t('devices:paramsError'), variant: "destructive" })
          return
        }
      }
      const success = await sendCommand(deviceDetailView, commandName, params)
      if (success) {
        toast({ title: t('devices:commandSent') })
      } else {
        toast({ title: t('devices:sendCommandFailed'), variant: "destructive" })
      }
    } catch {
      toast({ title: t('devices:sendCommandFailed'), variant: "destructive" })
    }
  }

  const [addingDevice, setAddingDevice] = useState(false)

  // Device edit dialog states
  const [editDeviceOpen, setEditDeviceOpen] = useState(false)
  const [editingDevice, setEditingDevice] = useState<Device | null>(null)
  const [updatingDevice, setUpdatingDevice] = useState(false)

  // Device edit handlers
  const handleEditDevice = async (device: Device) => {
    // Open dialog immediately with list data for instant feedback
    setEditingDevice(device)
    setEditDeviceOpen(true)
    // Fetch full details (including connection_config) in background
    const details = await fetchDeviceDetails(device.id)
    if (details) {
      // Update form data if dialog is still open
      setEditingDevice(details)
    }
  }

  const handleEditDeviceSubmit = async (id: string, data: Partial<{ name: string; adapter_type: string; connection_config: Record<string, unknown>; offline_timeout_secs: number | null }>) => {
    setUpdatingDevice(true)
    try {
      const success = await updateDevice(id, data)
      if (success) {
        setEditDeviceOpen(false)
        setEditingDevice(null)
      }
      return success
    } finally {
      setUpdatingDevice(false)
    }
  }

  // Device Type dialog states
  const [addDeviceTypeOpen, setAddDeviceTypeOpen] = useState(false)
  const [viewDeviceTypeOpen, setViewDeviceTypeOpen] = useState(false)
  const [editDeviceTypeOpen, setEditDeviceTypeOpen] = useState(false)
  const [generatorOpen, setGeneratorOpen] = useState(false)
  const [cloudImportOpen, setCloudImportOpen] = useState(false)
  const [importingDeviceType, setImportingDeviceType] = useState(false)
  const deviceTypeImportRef = useRef<HTMLInputElement>(null)
  const [selectedDeviceType, setSelectedDeviceType] = useState<DeviceType | null>(null)
  const [editingDeviceType, setEditingDeviceType] = useState<DeviceType | null>(null)
  const [addingType, setAddingType] = useState(false)
  const [validatingType, setValidatingType] = useState(false)

  // Device Type handlers
  const handleRefreshDeviceTypes = () => {
    fetchDeviceTypes()
  }

  const handleViewDeviceType = async (type: DeviceType) => {
    // Fetch full device type details with metrics and commands
    const details = await fetchDeviceTypeDetails(type.device_type)
    if (details) {
      setSelectedDeviceType(details)
      setViewDeviceTypeOpen(true)
    } else {
      toast({
        title: t('devices:loadFailed'),
        description: t('devices:failedToLoadDetails'),
        variant: "destructive",
      })
    }
  }

  const handleEditDeviceType = async (type: DeviceType) => {
    // Fetch full device type details with metrics and commands
    const details = await fetchDeviceTypeDetails(type.device_type)
    if (details) {
      setEditingDeviceType(details)
      setEditDeviceTypeOpen(true)
    } else {
      toast({
        title: t('devices:loadFailed'),
        description: t('devices:failedToLoadDetails'),
        variant: "destructive",
      })
    }
  }

  const handleDeleteDeviceType = async (id: string) => {
    const confirmed = await confirm({
      title: t('common:delete'),
      description: t('devices:deleteTypeConfirm'),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    await deleteDeviceType(id)
    toast({ title: t('common:success'), description: t('devices:deviceTypeDeleted') })
  }

  const handleAddDeviceType = async (definition: DeviceType) => {
    setAddingType(true)
    try {
      return await addDeviceType(definition)
    } finally {
      setAddingType(false)
    }
  }

  const handleValidateDeviceType = async (definition: DeviceType) => {
    setValidatingType(true)
    try {
      return await validateDeviceType(definition)
    } finally {
      setValidatingType(false)
    }
  }

  const handleEditDeviceTypeSubmit = async (data: DeviceType) => {
    return await handleAddDeviceType(data)
  }

  // Device Type import/export/generator handlers
  const handleDeviceTypeImportClick = () => {
    deviceTypeImportRef.current?.click()
  }

  const handleDeviceTypeImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    setImportingDeviceType(true)
    try {
      const text = await file.text()
      const imported = JSON.parse(text)
      const typesToImport = Array.isArray(imported) ? imported : [imported]

      let successCount = 0
      let errorCount = 0

      for (const type of typesToImport) {
        const result = await withErrorHandling(
          () => addDeviceType(type),
          { operation: `Import ${type.device_type}`, showToast: false }
        )
        if (result) {
          successCount++
        } else {
          errorCount++
        }
      }

      if (successCount > 0) {
        toast({
          title: t('common:success'),
          description: `Imported ${successCount} device type${successCount > 1 ? 's' : ''}${errorCount > 0 ? ` (${errorCount} failed)` : ''}`
        })
        fetchDeviceTypes()
      } else {
        toast({
          title: t('common:failed'),
          description: 'No device types were imported',
          variant: 'destructive'
        })
      }
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: 'Failed to parse JSON file',
        variant: 'destructive'
      })
    } finally {
      setImportingDeviceType(false)
      if (deviceTypeImportRef.current) {
        deviceTypeImportRef.current.value = ''
      }
    }
  }

  const handleDeviceTypeExportAll = async () => {
    try {
      const fullTypes = await Promise.all(
        deviceTypes.map(t => api.getDeviceType(t.device_type))
      )
      const data = JSON.stringify(fullTypes, null, 2)
      const blob = new Blob([data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const link = document.createElement('a')
      link.href = url
      link.download = `all-device-types.json`
      document.body.appendChild(link)
      link.click()
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
      toast({ title: t('common:success'), description: `Exported ${deviceTypes.length} device types` })
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: 'Failed to export device types',
        variant: 'destructive'
      })
    }
  }

  // Per-tab actions split into primary (visible buttons) and secondary
  // (collapsed into "More" overflow). Only the "+ Add" action stays
  // top-level on each tab; Import/Export/From Cloud are demoted to
  // secondary because they're low-frequency bulk operations.
  const tabActions = useMemo<{ primary: TabAction[]; secondary: TabAction[] }>(() => {
    if (activeTab === 'devices') {
      return {
        primary: [{ label: t('devices:addDevice'), onClick: () => setAddDeviceDialogOpen(true) }],
        secondary: [],
      }
    }
    if (activeTab === 'types') {
      return {
        primary: [{ label: t('devices:addDeviceType'), onClick: () => setAddDeviceTypeOpen(true) }],
        secondary: [
          {
            label: t('common:import'),
            icon: <Upload className="h-4 w-4" />,
            onClick: handleDeviceTypeImportClick,
            disabled: importingDeviceType,
          },
          {
            label: t('devices:cloud.fromCloud'),
            icon: <Cloud className="h-4 w-4" />,
            onClick: () => setCloudImportOpen(true),
          },
          {
            label: t('common:export') + ' All',
            icon: <Download className="h-4 w-4" />,
            onClick: handleDeviceTypeExportAll,
            disabled: deviceTypes.length === 0,
          },
        ],
      }
    }
    if (activeTab === 'drafts') {
      return {
        primary: [
          {
            label: t('devices:pending.config'),
            icon: <Settings className="h-4 w-4" />,
            variant: 'outline',
            onClick: openOnboardConfigDialog,
          },
        ],
        secondary: [],
      }
    }
    return { primary: [], secondary: [] }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab, t, deviceTypes.length, importingDeviceType])

  return (
    <>
      <PageLayout
        title={deviceDetailView ? undefined : t('devices:title')}
        subtitle={deviceDetailView ? undefined : t('devices:subtitle')}
        noPadding={!!deviceDetailView}
        hideFooterOnMobile
        hasBottomNav
        mobileHeader={
          deviceDetailView
            ? {
                // Show the device name as the MobilePageHeader title instead of
                // an empty header. Lift the back chevron into the header's
                // leftExtra and hide the hamburger — the in-page header still
                // has its own back button for redundancy, but the top-of-screen
                // header is the primary touch target.
                titleOverride: deviceDetails?.name || t('devices:detailTitle', 'Device detail'),
                hideMenu: true,
                leftExtra: (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-9 w-9 shrink-0"
                    onClick={handleCloseDeviceDetail}
                    aria-label={t('common:back', 'Back')}
                  >
                    <ChevronLeft className="h-5 w-5" />
                  </Button>
                ),
              }
            : undefined
        }
        headerContent={
          !deviceDetailView ? (
            <PageTabsBar
              tabs={[
                { value: 'devices', label: t('devices:deviceList'), icon: <Server className="h-4 w-4" /> },
                { value: 'types', label: t('devices:deviceTypes'), icon: <Layers className="h-4 w-4" /> },
                { value: 'drafts', label: t('devices:pending.tab'), icon: <FileEdit className="h-4 w-4" /> },
              ]}
              activeTab={activeTab}
              onTabChange={(v) => handleTabChange(v as DeviceTabValue)}
              actions={tabActions.primary}
              secondaryActions={tabActions.secondary}
            />
          ) : undefined
        }
        footer={
          !deviceDetailView && (
            activeTab === 'devices' && devices.length > devicesPerPage ? (
              <Pagination
                total={devices.length}
                pageSize={devicesPerPage}
                currentPage={devicePage}
                onPageChange={setDevicePage}
              />
            ) : activeTab === 'types' && deviceTypes.length > deviceTypesPerPage ? (
              <Pagination
                total={deviceTypes.length}
                pageSize={deviceTypesPerPage}
                currentPage={deviceTypePage}
                onPageChange={setDeviceTypePage}
              />
            ) : activeTab === 'drafts' && draftsCount > draftsPerPage ? (
              <Pagination
                total={draftsCount}
                pageSize={draftsPerPage}
                currentPage={draftPage}
                onPageChange={setDraftPage}
              />
            ) : undefined
          )
        }
      >
        {deviceDetailView ? (
          // Device Detail View
          deviceDetails ? (
            <DeviceDetail
              device={deviceDetails}
              deviceType={deviceTypeDetails}
              deviceCurrentState={deviceCurrentState}
              telemetryData={telemetryData}
              telemetryLoading={telemetryLoading}
              selectedMetric={selectedMetric}
              onBack={handleCloseDeviceDetail}
              onRefresh={handleRefreshDeviceDetail}
              onMetricClick={handleMetricClick}
              onMetricBack={() => setSelectedMetric(null)}
              onSendCommand={handleSendCommand}
            />
          ) : (
            // Loading state for device detail
            <LoadingState size="lg" className="h-64" text={t('common:loading')} />
          )
        ) : (
          // Tabbed View - Content only (tabs are in headerContent)
          <>
            {/* Devices Tab */}
            <PageTabsContent value="devices" activeTab={activeTab}>
              <DeviceList
                devices={devices}
                loading={devicesLoading}
                paginatedDevices={paginatedDevices}
                devicePage={devicePage}
                devicesPerPage={devicesPerPage}
                onRefresh={fetchDevices}
                onViewDetails={handleOpenDeviceDetails}
                onEdit={handleEditDevice}
                onDelete={handleDeleteDevice}
                onPageChange={setDevicePage}
                onAddDevice={() => {
                  if (deviceTypes.length === 0) fetchDeviceTypes()
                  setAddDeviceDialogOpen(true)
                }}
                addDeviceDialog={
                  <AddDeviceGlobalDialog
                    open={addDeviceDialogOpen}
                    onOpenChange={setAddDeviceDialogOpen}
                    deviceTypes={deviceTypes}
                    onAdd={handleAddDevice}
                    adding={addingDevice}
                  />
                }
              />
            </PageTabsContent>

            {/* Device Types Tab */}
            <PageTabsContent value="types" activeTab={activeTab}>
              <DeviceTypeList
                deviceTypes={deviceTypes}
                loading={deviceTypesLoading}
                paginatedDeviceTypes={paginatedDeviceTypes}
                deviceTypePage={deviceTypePage}
                deviceTypesPerPage={deviceTypesPerPage}
                onRefresh={handleRefreshDeviceTypes}
                onViewDetails={handleViewDeviceType}
                onEdit={handleEditDeviceType}
                onDelete={handleDeleteDeviceType}
                onPageChange={setDeviceTypePage}
                addTypeDialog={
                  <AddDeviceTypeDialog
                    open={addDeviceTypeOpen}
                    onOpenChange={setAddDeviceTypeOpen}
                    onAdd={handleAddDeviceType}
                    onValidate={handleValidateDeviceType}
                    adding={addingType}
                    validating={validatingType}
                  />
                }
              />
            </PageTabsContent>

            {/* Draft Devices Tab (Auto-onboarding) */}
            <PageTabsContent value="drafts" activeTab={activeTab}>
              <PendingDevicesList
                page={draftPage}
                onPageChange={setDraftPage}
                itemsPerPage={draftsPerPage}
                onDraftsCountChange={setDraftsCount}
                onRefresh={() => {
                  fetchDevices()
                  fetchDeviceTypes()
                }}
              />
            </PageTabsContent>
          </>
        )}
      </PageLayout>

      {/* Mobile: Bottom navigation bar */}
      {!deviceDetailView && (
        <PageTabsBottomNav
          tabs={[
            { value: 'devices', label: t('devices:deviceList'), icon: <Server className="h-4 w-4" /> },
            { value: 'types', label: t('devices:deviceTypes'), icon: <Layers className="h-4 w-4" /> },
            { value: 'drafts', label: t('devices:pending.tab'), icon: <FileEdit className="h-4 w-4" /> },
          ]}
          activeTab={activeTab}
          onTabChange={(v) => handleTabChange(v as DeviceTabValue)}
        />
      )}

      {/* Device Edit Dialog */}
      <EditDeviceDialog
        open={editDeviceOpen}
        onOpenChange={setEditDeviceOpen}
        device={editingDevice}
        deviceTypes={deviceTypes}
        onEdit={handleEditDeviceSubmit}
        editing={updatingDevice}
      />

      {/* Device Type Dialogs */}
      <ViewDeviceTypeDialog
        open={viewDeviceTypeOpen}
        onOpenChange={setViewDeviceTypeOpen}
        deviceType={selectedDeviceType}
      />

      <EditDeviceTypeDialog
        open={editDeviceTypeOpen}
        onOpenChange={setEditDeviceTypeOpen}
        deviceType={editingDeviceType}
        onEdit={handleEditDeviceTypeSubmit}
        editing={addingType}
      />

      {/* Hidden file input for device type import */}
      <input
        ref={deviceTypeImportRef}
        type="file"
        accept=".json"
        className="hidden"
        onChange={handleDeviceTypeImport}
      />

      {/* Device Type Generator Dialog */}
      <DeviceTypeGeneratorDialog
        open={generatorOpen}
        onOpenChange={setGeneratorOpen}
        onDeviceTypeCreated={() => {
          fetchDeviceTypes()
          setGeneratorOpen(false)
        }}
      />

      {/* Cloud Import Dialog */}
      <CloudImportDialog
        open={cloudImportOpen}
        onOpenChange={setCloudImportOpen}
        onImportComplete={() => {
          // Cloud import overwrites device types in storage; invalidate the
          // fetchCache so the list refetches instead of showing stale data
          // within the 10s TTL (otherwise imported updates look like no-ops).
          fetchCache.invalidate('deviceTypes')
          fetchDeviceTypes()
          setCloudImportOpen(false)
        }}
      />

      {/* Auto-onboarding Configuration Dialog */}
      <UnifiedFormDialog
        open={showOnboardConfigDialog}
        onOpenChange={setShowOnboardConfigDialog}
        title={t('devices:pending.configTitle')}
        width="sm"
        onSubmit={saveOnboardConfig}
        isSubmitting={savingOnboardConfig}
        submitLabel={t('common:save')}
      >
        <div className="space-y-6">
          {/* Enable/Disable auto-onboarding */}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label htmlFor="onboard-enabled">{t('devices:pending.configSettings.enabled')}</Label>
              <p className="text-xs text-muted-foreground">
                {t('devices:pending.configSettings.enabledDesc')}
              </p>
            </div>
            <Switch
              id="onboard-enabled"
              checked={pendingOnboardConfig.enabled}
              onCheckedChange={(checked) =>
                setPendingOnboardConfig({ ...pendingOnboardConfig, enabled: checked })
              }
            />
          </div>

          {/* Max samples */}
          <div className="space-y-2">
            <Label htmlFor="maxSamples">{t('devices:pending.configSettings.maxSamples')}</Label>
            <Input
              id="maxSamples"
              type="number"
              min={1}
              max={100}
              value={pendingOnboardConfig.max_samples}
              onChange={(e) =>
                setPendingOnboardConfig({
                  ...pendingOnboardConfig,
                  max_samples: Math.max(1, parseInt(e.target.value) || 10),
                })
              }
              disabled={!pendingOnboardConfig.enabled}
            />
            <p className="text-xs text-muted-foreground">
              {t('devices:pending.configSettings.maxSamplesDesc')}
            </p>
          </div>

          {/* Draft retention time */}
          <div className="space-y-2">
            <Label htmlFor="retention">{t('devices:pending.configSettings.retention')}</Label>
            <div className="flex items-center gap-2">
              <Input
                id="retention"
                type="number"
                min={3600}
                max={604800}
                step={3600}
                value={pendingOnboardConfig.draft_retention_secs}
                onChange={(e) =>
                  setPendingOnboardConfig({
                    ...pendingOnboardConfig,
                    draft_retention_secs: Math.max(3600, parseInt(e.target.value) || 86400),
                  })
                }
                disabled={!pendingOnboardConfig.enabled}
              />
              <span className="text-sm text-muted-foreground whitespace-nowrap">
                {Math.round(pendingOnboardConfig.draft_retention_secs / 3600)} {t('devices:pending.hours')}
              </span>
            </div>
            <p className="text-xs text-muted-foreground">
              {t('devices:pending.configSettings.retentionDesc')}
            </p>
          </div>

        </div>
      </UnifiedFormDialog>
    </>
  )
}

import { useEffect, useState, useRef, useCallback } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { useToast } from "@/hooks/use-toast"
import { useEvents } from "@/hooks/useEvents"
import { confirm } from "@/hooks/use-confirm"
import { useNavigate, useLocation, useParams } from "react-router-dom"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { Upload, Download, Settings } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Switch } from "@/components/ui/switch"
import { Label } from "@/components/ui/label"
import { Input } from "@/components/ui/input"
import { api } from "@/lib/api"
import type { Device, DiscoveredDevice, DeviceType } from "@/types"
import {
  DeviceList,
  DeviceDetail,
  DiscoveryDialog,
  AddDeviceDialog,
  DeviceTypeList,
  AddDeviceTypeDialog,
  ViewDeviceTypeDialog,
  EditDeviceTypeDialog,
} from "./devices/index"
import { DeviceTypeGeneratorDialog } from "@/components/devices/DeviceTypeGeneratorDialog"
import { PendingDevicesList } from "./devices/PendingDevicesList"

type DeviceTabValue = "devices" | "types" | "drafts"

export function DevicesPage() {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const { deviceId: urlDeviceId } = useParams<{ deviceId?: string }>()
  const devices = useStore((state) => state.devices)
  const devicesLoading = useStore((state) => state.devicesLoading)
  const fetchDevices = useStore((state) => state.fetchDevices)
  const fetchDeviceDetails = useStore((state) => state.fetchDeviceDetails)
  const fetchDeviceTypeDetails = useStore((state) => state.fetchDeviceTypeDetails)
  const addDevice = useStore((state) => state.addDevice)
  const deleteDevice = useStore((state) => state.deleteDevice)
  const deviceTypes = useStore((state) => state.deviceTypes)
  const deviceTypesLoading = useStore((state) => state.deviceTypesLoading)
  const fetchDeviceTypes = useStore((state) => state.fetchDeviceTypes)
  const addDeviceType = useStore((state) => state.addDeviceType)
  const deleteDeviceType = useStore((state) => state.deleteDeviceType)
  const validateDeviceType = useStore((state) => state.validateDeviceType)
  const addDeviceDialogOpen = useStore((state) => state.addDeviceDialogOpen)
  const setAddDeviceDialogOpen = useStore((state) => state.setAddDeviceDialogOpen)
  const sendCommand = useStore((state) => state.sendCommand)
  const deviceTypeDetails = useStore((state) => state.deviceTypeDetails)
  const deviceDetails = useStore((state) => state.deviceDetails)
  const telemetryData = useStore((state) => state.telemetryData)
  const telemetrySummary = useStore((state) => state.telemetrySummary)
  const deviceCurrentState = useStore((state) => state.deviceCurrentState)
  const telemetryLoading = useStore((state) => state.telemetryLoading)
  const fetchTelemetryData = useStore((state) => state.fetchTelemetryData)
  const fetchTelemetrySummary = useStore((state) => state.fetchTelemetrySummary)
  const fetchDeviceCurrentState = useStore((state) => state.fetchDeviceCurrentState)
  const discoverDevices = useStore((state) => state.discoverDevices)
  const discovering = useStore((state) => state.discovering)
  const discoveredDevices = useStore((state) => state.discoveredDevices)

  // Pagination state
  const [devicePage, setDevicePage] = useState(1)
  const devicesPerPage = 10

  // Device type pagination state
  const [deviceTypePage, setDeviceTypePage] = useState(1)
  const deviceTypesPerPage = 10

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
    try {
      const config = await api.getOnboardConfig()
      setOnboardConfig(config)
      setPendingOnboardConfig(config)
    } catch (error) {
      console.error('Failed to fetch onboard config:', error)
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
  const paginatedDeviceTypes = deviceTypes.slice(
    (deviceTypePage - 1) * deviceTypesPerPage,
    deviceTypePage * deviceTypesPerPage
  )

  // Reset pagination when data changes
  useEffect(() => {
    setDevicePage(1)
  }, [devices.length])

  // Paginated data
  const paginatedDevices = devices.slice(
    (devicePage - 1) * devicesPerPage,
    devicePage * devicesPerPage
  )

  // Dialog states
  const [discoveryOpen, setDiscoveryOpen] = useState(false)

  // Device detail view state
  const [deviceDetailView, setDeviceDetailView] = useState<string | null>(null)
  const [selectedMetric, setSelectedMetric] = useState<string | null>(null)

  // Fetch devices when component mounts
  const hasFetchedDevices = useRef(false)
  useEffect(() => {
    if (!hasFetchedDevices.current) {
      hasFetchedDevices.current = true
      fetchDevices()
    }
  }, [fetchDevices])

  // Fetch device types when component mounts
  const hasFetchedTypes = useRef(false)
  useEffect(() => {
    if (!hasFetchedTypes.current) {
      hasFetchedTypes.current = true
      fetchDeviceTypes()
    }
  }, [fetchDeviceTypes])

  // Load device from URL parameter
  useEffect(() => {
    // Use functional state update to get latest value
    setDeviceDetailView((currentDetailView) => {
      // If URL has deviceId and it's different from current, load the device
      if (urlDeviceId && urlDeviceId !== currentDetailView) {
        // Find the device in the list
        let device = devices.find(d => d.id === urlDeviceId)

        // If not found in list, try to fetch directly from API
        const loadDevice = async () => {
          if (!device && !devicesLoading) {
            try {
              device = await api.getDevice(urlDeviceId)
            } catch (error) {
              console.error('Failed to load device from URL:', error)
              return urlDeviceId // Still set the view even if API fails (will show error)
            }
          }

          if (device) {
            setSelectedMetric(null)
            await fetchDeviceDetails(urlDeviceId)
            await fetchDeviceTypeDetails(device.device_type)
            // Use unified endpoint: device + metrics in one call
            await fetchDeviceCurrentState(urlDeviceId)
          }
        }

        loadDevice()
        return urlDeviceId
      }

      // If URL doesn't have deviceId but state does, clear it
      if (!urlDeviceId && currentDetailView) {
        setSelectedMetric(null)
        return null
      }

      return currentDetailView
    })
    // Only depend on urlDeviceId - other values accessed inside are stable or OK to be stale
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
      fetchDevices()
    }, 500) // 500ms debounce
  }, [fetchDevices])

  const debouncedFetchDeviceTypes = useCallback(() => {
    if (refreshDeviceTypesRef.current) {
      clearTimeout(refreshDeviceTypesRef.current)
    }
    refreshDeviceTypesRef.current = setTimeout(() => {
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
    switch (event.type) {
      case 'DeviceOnline':
      case 'DeviceOffline':
        // Status change - refresh devices immediately
        fetchDevices()
        break
      case 'DeviceRegistered':
      case 'DeviceUnregistered':
        // Device list changed - refresh devices
        debouncedFetchDevices()
        break
      case 'DeviceTypeRegistered':
      case 'DeviceTypeUnregistered':
        // Device type list changed - refresh device types
        debouncedFetchDeviceTypes()
        break
      case 'DeviceMetric':
        // Don't refresh on every metric - too frequent
        // Status is handled by DeviceOnline/Offline events
        break
      case 'DeviceCommandResult':
        // Command completed - refresh devices to see updated state
        debouncedFetchDevices()
        break
    }
  }, [fetchDevices, debouncedFetchDevices, debouncedFetchDeviceTypes])

  // Subscribe to device events for real-time updates
  const { isConnected: deviceEventsConnected } = useEvents({
    enabled: !deviceDetailView, // Only when not in detail view
    category: 'device',
    onEvent: handleDeviceEvent,
  })

  // Fallback polling when WebSocket is not connected (only when not in detail view)
  useEffect(() => {
    if (deviceDetailView || deviceEventsConnected) return

    const interval = setInterval(() => {
      fetchDevices()
    }, 30000)

    return () => clearInterval(interval)
  }, [deviceDetailView, deviceEventsConnected, fetchDevices])

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

  const handleOpenDeviceDetails = async (device: Device) => {
    // Navigate to device detail URL
    navigate(`/devices/${device.id}`)
    setDeviceDetailView(device.id)
    setSelectedMetric(null)
    await fetchDeviceDetails(device.id)
    await fetchDeviceTypeDetails(device.device_type)
    // Use unified endpoint: device + metrics in one call
    await fetchDeviceCurrentState(device.id)
  }

  const handleCloseDeviceDetail = () => {
    // Navigate back to devices list
    navigate('/devices')
    setDeviceDetailView(null)
    setSelectedMetric(null)
  }

  const handleRefreshDeviceDetail = async () => {
    if (deviceDetailView) {
      await fetchDeviceDetails(deviceDetailView)
      // Use unified endpoint for refresh
      await fetchDeviceCurrentState(deviceDetailView)
      if (selectedMetric) {
        await fetchTelemetryData(deviceDetailView, selectedMetric, undefined, undefined, 1000)
      }
    }
  }

  const handleMetricClick = async (metricName: string) => {
    if (!deviceDetailView) return
    setSelectedMetric(metricName)
    const end = Math.floor(Date.now() / 1000)
    const start = end - 86400 // 24 hours
    await fetchTelemetryData(deviceDetailView, metricName, start, end, 1000)
  }

  const handleSendCommand = async (commandName: string, paramsJson: string) => {
    if (!deviceDetailView) return

    try {
      let params: Record<string, unknown> = {}
      if (paramsJson.trim()) {
        try {
          params = JSON.parse(paramsJson)
        } catch {
          alert(t('devices:paramsError'))
          return
        }
      }
      const success = await sendCommand(deviceDetailView, commandName, params)
      if (success) {
        } else {
        alert(t('devices:sendCommandFailed'))
      }
    } catch {
      alert(t('devices:sendCommandFailed'))
    }
  }

  const handleAddDiscoveredDevice = async (device: DiscoveredDevice) => {
    if (!device.device_type) {
      toast({ title: t('common:failed'), description: t('devices:unknownType'), variant: "destructive" })
      return
    }
    // For discovered devices, use MQTT adapter with default topics
    const success = await addDevice({
      device_id: device.id,
      name: device.id,
      device_type: device.device_type,
      adapter_type: 'mqtt',
      connection_config: {
        telemetry_topic: `device/${device.device_type}/${device.id}/uplink`,
      }
    })
    if (success) {
      setDiscoveryOpen(false)
      toast({ title: t('common:success'), description: t('devices:add.successGeneric') })
    } else {
      toast({ title: t('common:failed'), description: t('devices:addDeviceFailed'), variant: "destructive" })
    }
  }

  const [addingDevice, setAddingDevice] = useState(false)

  // Device Type dialog states
  const [addDeviceTypeOpen, setAddDeviceTypeOpen] = useState(false)
  const [viewDeviceTypeOpen, setViewDeviceTypeOpen] = useState(false)
  const [editDeviceTypeOpen, setEditDeviceTypeOpen] = useState(false)
  const [generatorOpen, setGeneratorOpen] = useState(false)
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
        try {
          await addDeviceType(type)
          successCount++
        } catch (err) {
          errorCount++
          console.error(`Failed to import ${type.device_type}:`, err)
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

  return (
    <PageLayout
      title={deviceDetailView ? undefined : t('devices:title')}
      subtitle={deviceDetailView ? undefined : t('devices:subtitle')}
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
          <div className="flex items-center justify-center h-64">
            <div className="text-center">
              <div className="inline-block h-8 w-8 animate-spin rounded-full border-4 border-solid border-primary border-r-transparent" />
              <p className="mt-4 text-muted-foreground">Loading device details...</p>
            </div>
          </div>
        )
      ) : (
        // Tabbed View
        <PageTabs
          tabs={[
            { value: 'devices', label: t('devices:deviceList') },
            { value: 'types', label: t('devices:deviceTypes') },
            { value: 'drafts', label: t('devices:pending.tab') },
          ]}
          activeTab={activeTab}
          onTabChange={(v) => handleTabChange(v as DeviceTabValue)}
          actions={
            activeTab === 'devices'
              ? [
                  {
                    label: t('devices:addDevice'),
                    onClick: () => setAddDeviceDialogOpen(true),
                  },
                  {
                    label: t('devices:localNetworkScan'),
                    variant: 'outline',
                    onClick: () => setDiscoveryOpen(true),
                  },
                ]
              : activeTab === 'types'
              ? [
                  {
                    label: t('common:import'),
                    icon: <Upload className="h-4 w-4" />,
                    variant: 'outline',
                    onClick: handleDeviceTypeImportClick,
                    disabled: importingDeviceType,
                  },
                  {
                    label: t('common:export') + ' All',
                    icon: <Download className="h-4 w-4" />,
                    variant: 'outline',
                    onClick: handleDeviceTypeExportAll,
                    disabled: deviceTypes.length === 0,
                  },
                  {
                    label: t('devices:addDeviceType'),
                    onClick: () => setAddDeviceTypeOpen(true),
                  },
                ]
              : activeTab === 'drafts'
              ? [
                  {
                    label: t('devices:pending.config'),
                    icon: <Settings className="h-4 w-4" />,
                    variant: 'outline',
                    onClick: openOnboardConfigDialog,
                  },
                ]
              : []
          }
        >
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
              onDelete={handleDeleteDevice}
              onPageChange={setDevicePage}
              onAddDevice={() => setAddDeviceDialogOpen(true)}
              discoveryDialogOpen={discoveryOpen}
              onDiscoveryOpenChange={setDiscoveryOpen}
              discoveryDialog={
                <DiscoveryDialog
                  open={discoveryOpen}
                  onOpenChange={setDiscoveryOpen}
                  discovering={discovering}
                  discoveredDevices={discoveredDevices}
                  deviceTypes={deviceTypes}
                  onDiscover={discoverDevices}
                  onAddDiscovered={handleAddDiscoveredDevice}
                />
              }
              addDeviceDialog={
                <AddDeviceDialog
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
              onRefresh={() => {
                fetchDevices()
                fetchDeviceTypes()
              }}
            />
          </PageTabsContent>
        </PageTabs>
      )}

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

      {/* Auto-onboarding Configuration Dialog */}
      <Dialog open={showOnboardConfigDialog} onOpenChange={setShowOnboardConfigDialog}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t('devices:pending.configTitle')}</DialogTitle>
            <DialogDescription>
              {t('devices:pending.configDesc')}
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-6 py-4">
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

            {/* Info box */}
            <div className="rounded-md bg-muted p-3 text-sm">
              <p className="text-muted-foreground">
                💡 {t('devices:pending.configSettings.info')}
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setShowOnboardConfigDialog(false)}>
              {t('common:cancel')}
            </Button>
            <Button onClick={saveOnboardConfig} disabled={savingOnboardConfig}>
              {savingOnboardConfig ? t('common:saving') : t('common:save')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </PageLayout>
  )
}

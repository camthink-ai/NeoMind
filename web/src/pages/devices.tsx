import { useEffect, useState, useRef } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { useToast } from "@/hooks/use-toast"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
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
  HassDiscoveryDialog,
} from "./devices/index"

type DeviceTabValue = "devices" | "types"

export function DevicesPage() {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()
  const {
    devices,
    devicesLoading,
    fetchDevices,
    fetchDeviceDetails,
    fetchDeviceTypeDetails,
    addDevice,
    deleteDevice,
    deviceTypes,
    deviceTypesLoading,
    fetchDeviceTypes,
    addDeviceType,
    deleteDeviceType,
    validateDeviceType,
    generateMDL,
    addDeviceDialogOpen,
    setAddDeviceDialogOpen,
    sendCommand,
    deviceTypeDetails,
    deviceDetails,
    telemetryData,
    commandHistory,
    telemetryLoading,
    fetchTelemetryData,
    fetchCommandHistory,
    fetchMqttSettings,
    discoverDevices,
    discovering,
    discoveredDevices,
    // HASS Discovery
    hassDiscoveryStatus,
    hassDiscoveredDevices,
    hassDiscovering,
    fetchHassDiscoveryStatus,
    fetchHassDiscoveredDevices,
    startHassDiscovery,
    stopHassDiscovery,
    registerHassDevice,
    unregisterHassDevice,
    clearHassDiscoveredDevices,
  } = useStore()

  // Fetch MQTT settings on mount (once)
  const hasFetchedMqttSettings = useRef(false)
  useEffect(() => {
    if (!hasFetchedMqttSettings.current) {
      hasFetchedMqttSettings.current = true
      fetchMqttSettings()
    }
  }, [])

  // Pagination state
  const [devicePage, setDevicePage] = useState(1)
  const devicesPerPage = 10

  // Device type pagination state
  const [deviceTypePage, setDeviceTypePage] = useState(1)
  const deviceTypesPerPage = 10

  // Active tab state
  const [activeTab, setActiveTab] = useState<"devices" | "types">("devices")

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
  const [hassDiscoveryOpen, setHassDiscoveryOpen] = useState(false)

  // Fetch HASS discovery status when dialog opens
  useEffect(() => {
    if (hassDiscoveryOpen) {
      fetchHassDiscoveryStatus()
      fetchHassDiscoveredDevices()
    }
  }, [hassDiscoveryOpen])

  // Device detail view state
  const [deviceDetailView, setDeviceDetailView] = useState<string | null>(null)
  const [selectedMetric, setSelectedMetric] = useState<string | null>(null)

  // Fetch devices on mount (once)
  const hasFetchedDevices = useRef(false)
  useEffect(() => {
    if (!hasFetchedDevices.current) {
      hasFetchedDevices.current = true
      fetchDevices()
    }
  }, [])

  // Fetch device types on mount (once)
  const hasFetchedDeviceTypes = useRef(false)
  useEffect(() => {
    if (!hasFetchedDeviceTypes.current) {
      hasFetchedDeviceTypes.current = true
      fetchDeviceTypes()
    }
  }, [])

  // Auto-refresh device status every 10 seconds (only when not in detail view)
  useEffect(() => {
    if (deviceDetailView) return

    const interval = setInterval(() => {
      fetchDevices()
    }, 10000)

    return () => clearInterval(interval)
  }, [deviceDetailView])

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
    if (confirm(t('devices:deleteConfirm'))) {
      const success = await deleteDevice(id)
      if (success) {
        toast({ title: t('common:success'), description: t('devices:deviceDeleted') })
      } else {
        toast({ title: t('common:failed'), description: t('devices:deleteFailed'), variant: "destructive" })
      }
    }
  }

  const handleOpenDeviceDetails = async (device: Device) => {
    setDeviceDetailView(device.id)
    setSelectedMetric(null)
    await fetchDeviceDetails(device.id)
    await fetchDeviceTypeDetails(device.device_type)
    // Fetch all telemetry data (no specific metric = get all metrics)
    const end = Math.floor(Date.now() / 1000)
    const start = end - 86400 // 24 hours
    await fetchTelemetryData(device.id, undefined, start, end, 100)
    await fetchCommandHistory(device.id, 50)
  }

  const handleCloseDeviceDetail = () => {
    setDeviceDetailView(null)
    setSelectedMetric(null)
  }

  const handleRefreshDeviceDetail = async () => {
    if (deviceDetailView) {
      await fetchDeviceDetails(deviceDetailView)
      if (selectedMetric) {
        await fetchTelemetryData(deviceDetailView, selectedMetric, undefined, undefined, 1000)
      } else {
        // Fetch all metrics if no specific metric selected
        const end = Math.floor(Date.now() / 1000)
        const start = end - 86400
        await fetchTelemetryData(deviceDetailView, undefined, start, end, 100)
      }
      await fetchCommandHistory(deviceDetailView, 50)
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
        await fetchCommandHistory(deviceDetailView, 50)
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
  const [selectedDeviceType, setSelectedDeviceType] = useState<DeviceType | null>(null)
  const [editingDeviceType, setEditingDeviceType] = useState<DeviceType | null>(null)
  const [addingType, setAddingType] = useState(false)
  const [validatingType, setValidatingType] = useState(false)
  const [generatingMDL, setGeneratingMDL] = useState(false)

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
    if (confirm(t('devices:deleteTypeConfirm'))) {
      const success = await deleteDeviceType(id)
      if (success) {
        toast({ title: t('common:success'), description: t('devices:deviceTypeDeleted') })
      } else {
        toast({ title: t('common:failed'), description: t('devices:deviceTypeDeleteFailed'), variant: "destructive" })
      }
    }
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

  const handleGenerateMDL = async (deviceName: string, description: string, metricsExample: string, commandsExample: string) => {
    setGeneratingMDL(true)
    try {
      // Backend API still expects uplink_example/downlink_example for backward compatibility
      const result = await generateMDL({ 
        device_name: deviceName, 
        description, 
        uplink_example: metricsExample, 
        downlink_example: commandsExample 
      })
      // Add metric_count and command_count to the result
      const fullResult = {
        ...result,
        metric_count: result.metrics?.length || 0,
        command_count: result.commands?.length || 0,
      }
      return JSON.stringify(fullResult, null, 2)
    } finally {
      setGeneratingMDL(false)
    }
  }

  const handleEditDeviceTypeSubmit = async (data: DeviceType) => {
    return await handleAddDeviceType(data)
  }

  return (
    <PageLayout>
      {deviceDetailView && deviceDetails ? (
        // Device Detail View
        <DeviceDetail
          device={deviceDetails}
          deviceType={deviceTypeDetails}
          telemetryData={telemetryData}
          commandHistory={commandHistory}
          telemetryLoading={telemetryLoading}
          mqttSettings={useStore.getState().mqttSettings}
          selectedMetric={selectedMetric}
          onBack={handleCloseDeviceDetail}
          onRefresh={handleRefreshDeviceDetail}
          onMetricClick={handleMetricClick}
          onMetricBack={() => setSelectedMetric(null)}
          onSendCommand={handleSendCommand}
        />
      ) : (
        // Tabbed View
        <PageTabs
          tabs={[
            { value: 'devices', label: t('devices:deviceList') },
            { value: 'types', label: t('devices:deviceTypes') },
          ]}
          activeTab={activeTab}
          onTabChange={(v) => setActiveTab(v as DeviceTabValue)}
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
              hassDiscoveryDialogOpen={hassDiscoveryOpen}
              onHassDiscoveryOpenChange={setHassDiscoveryOpen}
              hassDiscoveryDialog={
                <HassDiscoveryDialog
                  open={hassDiscoveryOpen}
                  onClose={() => setHassDiscoveryOpen(false)}
                  hassDiscoveryStatus={hassDiscoveryStatus}
                  hassDiscoveredDevices={hassDiscoveredDevices}
                  hassDiscovering={hassDiscovering}
                  onStartDiscovery={async () => {
                    await startHassDiscovery({ auto_register: false })
                  }}
                  onStopDiscovery={async () => {
                    await stopHassDiscovery()
                  }}
                  onRefresh={async () => {
                    await fetchHassDiscoveryStatus()
                    await fetchHassDiscoveredDevices()
                  }}
                  onRegisterDevice={async (device) => {
                    return await registerHassDevice(device.device_id)
                  }}
                  onUnregisterDevice={async (deviceId) => {
                    return await unregisterHassDevice(deviceId)
                  }}
                  onClearDevices={() => {
                    clearHassDiscoveredDevices()
                  }}
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
              onAddType={() => setAddDeviceTypeOpen(true)}
              addTypeDialog={
                <AddDeviceTypeDialog
                  open={addDeviceTypeOpen}
                  onOpenChange={setAddDeviceTypeOpen}
                  onAdd={handleAddDeviceType}
                  onValidate={handleValidateDeviceType}
                  onGenerateMDL={handleGenerateMDL}
                  adding={addingType}
                  validating={validatingType}
                  generating={generatingMDL}
                />
              }
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
        onGenerateMDL={handleGenerateMDL}
      />
    </PageLayout>
  )
}

/**
 * Visual Dashboard Page
 *
 * Main dashboard page with grid layout, drag-and-drop, and component library.
 * Supports both generic IoT components and business components.
 */

import { useEffect, useState, useCallback, useRef, useMemo, memo } from 'react'
import '@/lib/debug-scroll' // Auto-inits if DEBUG_SCROLL=true in localStorage
import { createPortal } from 'react-dom'
import { getPortalRoot } from '@/lib/portal'
import { usePageSidebarSlot, PageSidebarColumn } from '@/components/layout/PageSidebarSlot'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { shallow } from 'zustand/shallow'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { useExtensionLifecycle } from '@/hooks/useExtensionLifecycle'
import { useCommunityComponentLifecycle } from '@/hooks/useCommunityComponentLifecycle'
import { useDashboardPrefetch } from '@/hooks/useDashboardPrefetch'
import { useDashboardRealtime } from '@/hooks/useDashboardRealtime'
import { useComponentConfigDialog } from '@/hooks/useComponentConfigDialog'
import { fetchCache } from '@/lib/utils/async'
import { cn } from '@/lib/utils'
import { chartColorsHex } from '@/design-system/tokens/color'
import { useIsMobile } from '@/hooks/useMobile'
import { MobilePageHeader } from '@/components/layout/MobilePageHeader'
import { PageHeader } from '@/components/layout/PageHeader'
import {
  LayoutDashboard,
  Plus,
  Minimize,
  Hash,
  Monitor,
  Grid,
  Settings2,
  Sparkles,
  ChevronRight,
} from 'lucide-react'
import { useParams, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
} from '@/components/automation/dialog'
import { toast } from '@/components/ui/use-toast'

// Config system
import {
  ComponentConfigDialog,
} from '@/components/dashboard/config'

// Dashboard components
import { DashboardGrid } from '@/components/dashboard/DashboardGrid'
import { LayerEditorDialog } from '@/components/dashboard/generic/LayerEditorDialog'
import { MapEditorDialog, type MapBinding } from '@/components/dashboard/generic/MapEditorDialog'
import { CenterPickerDialog } from '@/components/dashboard/generic/CenterPickerDialog'
import type { LayerBinding } from '@/components/dashboard/generic/CustomLayer'
import { DashboardListSidebar } from '@/components/dashboard/DashboardListSidebar'
import { ShareManagerDialog } from '@/components/dashboard/ShareManagerDialog'
import { MobileEditBar } from '@/components/dashboard/MobileEditBar'
import type { DashboardComponent, DataSource, GenericComponent } from '@/types/dashboard'
import type { Device } from '@/types'
import { COMPONENT_SIZE_CONSTRAINTS } from '@/types/dashboard'
import { dynamicRegistry } from '@/components/dashboard/registry/DynamicRegistry'
import { api } from '@/lib/api'
import { confirm } from '@/hooks/use-confirm'

// Renderers extracted to Renderers.tsx
import { renderDashboardComponent, ComponentWrapper } from './Renderers'

// Extracted sub-modules
import { getComponentLibrary } from './componentLibraryUtils'
import { DashboardToolbar } from './DashboardToolbar'

// ============================================================================
// Helper Functions
// ============================================================================

// All components show title in the display tab (unified standard)
function isTitleInDisplayComponent(_componentType?: string): boolean {
  // Unified: all components show title in Display tab
  return true
}

// ============================================================================
// Main Component
// ============================================================================

// Performance optimization: Memoize VisualDashboard to prevent unnecessary re-renders
// Only re-renders when dashboardId, editMode, or currentDashboard.id changes
const VisualDashboardMemo = memo(function VisualDashboard() {
  const { dashboardId } = useParams<{ dashboardId?: string }>()
  const navigate = useNavigate()
  const { t, i18n } = useTranslation('dashboardComponents')
  const { handleError } = useErrorHandler()

  // Dashboard state — split subscriptions to avoid cascade:
  // `devices` changes every 3s (batch polling) but rarely affects layout.
  // Keep it in a separate selector so the main block doesn't re-render on every batch.
  const {
    currentDashboard,
    currentDashboardId,
    dashboards,
    dashboardsLoading,
    editMode,
    componentLibraryOpen,
  } = useStore((s) => ({
    currentDashboard: s.currentDashboard,
    currentDashboardId: s.currentDashboardId,
    dashboards: s.dashboards,
    dashboardsLoading: s.dashboardsLoading,
    editMode: s.editMode,
    componentLibraryOpen: s.componentLibraryOpen,
  }), shallow)

  // Subscribe to devices LENGTH only — avoids re-rendering every 3s on batch polling
  // (component data comes from useDataSource, not this prop)
  const devicesLength = useStore((s) => s.devices.length)
  // Read devices directly from store when needed (not reactive)
  const devicesRef = useRef<Device[]>([])

  // Action selectors — single subscription for all actions (stable references)
  const {
    setEditMode, addComponent, updateComponent, batchUpdatePositions,
    removeComponent, duplicateComponent, createDashboard, updateDashboard,
    deleteDashboard, persistDashboard, setCurrentDashboard, setComponentLibraryOpen,
    fetchDashboards, fetchDevices, fetchDeviceTypes, fetchDevicesCurrentBatch,
    sendCommand, duplicateDashboard,
  } = useStore((s) => ({
    setEditMode: s.setEditMode, addComponent: s.addComponent, updateComponent: s.updateComponent,
    batchUpdatePositions: s.batchUpdatePositions, removeComponent: s.removeComponent,
    duplicateComponent: s.duplicateComponent, createDashboard: s.createDashboard,
    updateDashboard: s.updateDashboard, deleteDashboard: s.deleteDashboard,
    persistDashboard: s.persistDashboard, setCurrentDashboard: s.setCurrentDashboard,
    setComponentLibraryOpen: s.setComponentLibraryOpen,
    fetchDashboards: s.fetchDashboards, fetchDevices: s.fetchDevices,
    fetchDeviceTypes: s.fetchDeviceTypes, fetchDevicesCurrentBatch: s.fetchDevicesCurrentBatch,
    sendCommand: s.sendCommand, duplicateDashboard: s.duplicateDashboard,
  }), shallow)

  // Marketplace store selectors — single subscription
  const {
    marketComponents, marketLoading, installed: installedComponents,
    fetchMarket, fetchInstalled, installFromMarket, uninstall: uninstallComponent,
    refreshComponent: refreshComponentAction, updatesAvailable, checkUpdates,
  } = useStore((s) => ({
    marketComponents: s.marketComponents, marketLoading: s.marketLoading,
    installed: s.installed, fetchMarket: s.fetchMarket,
    fetchInstalled: s.fetchInstalled, installFromMarket: s.installFromMarket,
    uninstall: s.uninstall, refreshComponent: s.refreshComponent,
    updatesAvailable: s.updatesAvailable, checkUpdates: s.checkUpdates,
  }), shallow)

  // Extension lifecycle management for hot updates
  const { refreshVersion } = useExtensionLifecycle({
    autoSyncOnRegister: true,
    autoRemoveOnUnregister: true,
  })

  // Community component lifecycle
  useCommunityComponentLifecycle()

  // Pre-batch data loading: 1 batch request + telemetry cache warm-up
  useDashboardPrefetch(currentDashboard?.components ?? [])

  // Sort dashboards by `sortOrder` (manual sidebar order), falling back to
  // creation time for legacy dashboards without an explicit order. Stable sort
  // keeps tie-breakers predictable across fetches.
  const sortedDashboards = useMemo(
    () => [...dashboards].sort((a, b) => {
      const sa = a.sortOrder ?? Number.MAX_SAFE_INTEGER
      const sb = b.sortOrder ?? Number.MAX_SAFE_INTEGER
      if (sa !== sb) return sa - sb
      return (a.createdAt ?? 0) - (b.createdAt ?? 0)
    }),
    [dashboards]
  )

  // Memoize component library with refreshVersion dependency to trigger re-renders
  const componentLibrary = useMemo(() => getComponentLibrary(t), [t, refreshVersion, installedComponents.length])

  // Component library search
  const [librarySearch, setLibrarySearch] = useState('')
  const [libraryTab, setLibraryTab] = useState<'components' | 'marketplace'>('components')
  const [importDialogOpen, setImportDialogOpen] = useState(false)
  const [installingId, setInstallingId] = useState<string | null>(null)

  // Fetch marketplace data when marketplace tab is active
  useEffect(() => {
    if (componentLibraryOpen && libraryTab === 'marketplace') {
      fetchMarket()
    }
  }, [componentLibraryOpen, libraryTab, fetchMarket])

  // Check for community component updates when the library opens (30s throttle).
  const lastUpdateCheckRef = useRef<number>(0)
  useEffect(() => {
    if (!componentLibraryOpen) return
    const now = Date.now()
    if (now - lastUpdateCheckRef.current < 30_000) return
    lastUpdateCheckRef.current = now
    checkUpdates()
  }, [componentLibraryOpen, checkUpdates])

  // Fetch installed components early for community registry sync
  // This MUST complete before community widgets can receive fetchData prop
  useEffect(() => {
    fetchInstalled()
  }, [fetchInstalled])

  const filteredLibrary = useMemo(() => {
    if (!librarySearch.trim()) return componentLibrary
    const q = librarySearch.toLowerCase()
    return componentLibrary
      .map(cat => ({
        ...cat,
        items: cat.items.filter(item =>
          item.name.toLowerCase().includes(q) ||
          item.description.toLowerCase().includes(q)
        ),
      }))
      .filter(cat => cat.items.length > 0)
  }, [componentLibrary, librarySearch])

  // Extension fullscreen dialog state
  const [extFullscreenContent, setExtFullscreenContent] = useState<React.ReactNode | null>(null)

  const openExtFullscreen = useCallback((content: React.ReactNode) => {
    setExtFullscreenContent(content)
  }, [])
  const closeExtFullscreen = useCallback(() => {
    setExtFullscreenContent(null)
  }, [])

  // Mobile editing state
  const isMobile = useIsMobile()
  const isDesktop = !isMobile
  const pageSidebarSlot = usePageSidebarSlot()
  const [mobileSelectedId, setMobileSelectedId] = useState<string | null>(null)
  const [mobileEditBarOpen, setMobileEditBarOpen] = useState(false)

  // Map editor dialog state
  const [mapEditorOpen, setMapEditorOpen] = useState(false)
  const [mapEditorBindings, setMapEditorBindings] = useState<MapBinding[]>([])

  // Center picker dialog state
  const [centerPickerOpen, setCenterPickerOpen] = useState(false)

  // Layer editor dialog state
  const [layerEditorOpen, setLayerEditorOpen] = useState(false)
  const [layerEditorBindings, setLayerEditorBindings] = useState<LayerBinding[]>([])

  // Agents for agent-monitor-widget config (summaries only)
  const [agents, setAgents] = useState<{ id: string; name: string; status: string }[]>([])
  const [agentsLoading, setAgentsLoading] = useState(false)

  // Vision models for ai-analyst config
  const [visionModels, setVisionModels] = useState<{ id: string; name: string; backendId: string; backendName: string; isMultimodal?: boolean }[]>([])
  const [visionModelsLoading, setVisionModelsLoading] = useState(false)

  // Fullscreen state
  const [isFullscreen, setIsFullscreen] = useState(false)

  // Share dialog state
  const [shareDialogOpen, setShareDialogOpen] = useState(false)

  // Mobile sidebar drawer state (desktop sidebar is always open)
  const [sidebarOpen, setSidebarOpen] = useState(false)

  // Layout mode: 'sidebar' or 'tabs' (horizontal tab bar in toolbar, default)
  const [layoutMode, setLayoutMode] = useState<'sidebar' | 'tabs'>(() => {
    const saved = localStorage.getItem('neomind_dashboard_layout_mode')
    if (saved === 'tabs' || saved === 'sidebar') return saved
    return 'tabs'
  })

  // Switch to tab bar layout
  const handleSwitchToTabs = useCallback(() => {
    setLayoutMode('tabs')
    localStorage.setItem('neomind_dashboard_layout_mode', 'tabs')
  }, [])

  // Switch back to sidebar layout
  const handleSwitchToSidebar = useCallback(() => {
    setLayoutMode('sidebar')
    localStorage.setItem('neomind_dashboard_layout_mode', 'sidebar')
  }, [])

  // Fullscreen toggle function - CSS-only fullscreen for dashboard content
  const toggleFullscreen = useCallback(() => {
    setIsFullscreen(prev => !prev)
  }, [])

  // Dashboard list handlers
  const handleDashboardSwitch = useCallback((id: string) => {
    setCurrentDashboard(id)
    // Reset mobile editing state when switching dashboards
    setMobileSelectedId(null)
    setMobileEditBarOpen(false)
    // URL will be updated automatically by the Store → URL sync effect
  }, [setCurrentDashboard])

  // Dashboard interaction handlers
  const handleDeviceClick = useCallback(async (deviceId: string) => {
    const device = devicesRef.current.find(d => d.id === deviceId || d.device_id === deviceId)
    if (device) {
      // Navigate to device detail page
      navigate(`/devices/${device.id}`)
    } else {
      toast({
        title: t('visualDashboard.deviceNotFound'),
        description: t('visualDashboard.deviceNotFoundDesc'),
        variant: 'destructive',
      })
    }
  }, [navigate, t])

  const handleMetricClick = useCallback(async (metricId: string, deviceId?: string) => {
    // Show metric info in toast
    toast({
      title: t('visualDashboard.metricInfo'),
      description: `${t('visualDashboard.metric')}: ${metricId}${deviceId ? `\n${t('visualDashboard.device')}: ${deviceId.slice(0, 8)}...` : ''}`,
    })
  }, [t])

  const handleCommandClick = useCallback(async (deviceId: string, command: string) => {
    try {
      const success = await sendCommand(deviceId, command)
      if (success) {
        toast({
          title: t('visualDashboard.commandSent'),
          description: `${t('visualDashboard.command')}: ${command}\n${t('visualDashboard.device')}: ${deviceId.slice(0, 8)}...`,
        })
      } else {
        toast({
          title: t('visualDashboard.commandFailed'),
          description: `${t('visualDashboard.command')}: ${command}`,
          variant: 'destructive',
        })
      }
    } catch (error) {
      handleError(error, t('visualDashboard.commandError'))
    }
  }, [sendCommand, t, handleError])

  const handleDashboardCreate = useCallback(async (name: string) => {
    const newId = await createDashboard({
      name,
      layout: {
        columns: 12,
        rows: 'auto' as const,
        breakpoints: { lg: 1200, md: 996, sm: 768, xs: 480 },
      },
      components: [],
    })
    if (newId) {
      // Flush backend sync immediately to resolve any id remapping before navigating,
      // so the URL gets the final stable dashboard id. Otherwise the URL would hold
      // the local id while the store later updates to the backend-assigned id, causing
      // the URL ↔ Store sync to bounce the user away from the newly created dashboard.
      await useStore.getState().flushSync()
      const finalId = useStore.getState().currentDashboardId ?? newId
      setCurrentDashboard(finalId)
      navigate(`/visual-dashboard/${finalId}`, { replace: true })
    }
  }, [createDashboard, navigate, setCurrentDashboard])

  const handleDashboardRename = useCallback((id: string, name: string) => {
    updateDashboard(id, { name })
  }, [updateDashboard])

  const handleDashboardDuplicate = useCallback(async (id: string) => {
    const newId = await duplicateDashboard(id)
    if (!newId) {
      toast({ title: t('visualDashboard.duplicateFailed'), variant: 'destructive' })
      return
    }
    toast({ title: t('visualDashboard.duplicated') })
    await setCurrentDashboard(newId)
    navigate(`/visual-dashboard/${newId}`)
  }, [duplicateDashboard, setCurrentDashboard, navigate, t])

  const handleDashboardDelete = useCallback(async (id: string) => {
    const confirmed = await confirm({
      title: 'Delete Dashboard',
      description: 'Delete this dashboard?',
      confirmText: 'Delete',
      cancelText: 'Cancel',
      variant: 'destructive'
    })
    if (confirmed) {
      deleteDashboard(id)
    }
  }, [deleteDashboard])

  // Track if we've initialized to avoid duplicate calls
  const hasInitialized = useRef(false)

  // Track last synced dashboard ID to prevent URL↔Store ping-pong
  const lastSyncedIdRef = useRef<string | null>(null)

  // Track previous components to detect actual changes (not just reference changes)
  // Create a stable key for components to detect actual changes
  // This key only changes when component data actually changes, not on every render
  const componentsStableKey = useMemo(() => {
    const components = currentDashboard?.components ?? []
    // Lightweight content hash — must include dataSource/config changes
    // so the grid rebuilds when data binding or settings change.
    let hash = components.length.toString(36)
    for (const c of components) {
      const gc = c as GenericComponent
      // Hash dataSource identity (use _saveTs stamp if present for forced refresh)
      const ds = gc.dataSource
      const dsKey = ds ? (Array.isArray(ds) ? ds.map((d: any) => `${d.type}:${d.sourceId ?? d.extensionId ?? ''}:${d.metricId ?? ''}:${d._saveTs ?? ''}`).join(',') : `${(ds as any).type}:${(ds as any).sourceId ?? (ds as any).extensionId ?? ''}:${(ds as any).metricId ?? ''}:${(ds as any)._saveTs ?? ''}`) : ''
      // Hash config — use full JSON content to detect ALL value changes,
      // not just structural changes (length/keys).
      const configHash = gc.config ? JSON.stringify(gc.config) : ''
      hash += `|${c.id}:${c.type}:${c.title}:${c.position?.x ?? 0},${c.position?.y ?? 0},${c.position?.w ?? 0},${c.position?.h ?? 0}:${dsKey}:${configHash}`
    }
    return hash
  }, [currentDashboard])

  // Initialize dashboards on mount
  useEffect(() => {
    if (hasInitialized.current) return
    hasInitialized.current = true

    // Fetch dashboards first so the shell and saved layout can paint quickly.
    fetchDashboards()

    // Fetch devices and types immediately (not delayed) so that dashboard
    // components with data bindings can read current_values synchronously
    // on first mount instead of waiting for async fetch chains.
    fetchDevices()
    fetchDeviceTypes()
  }, [fetchDashboards, fetchDevices, fetchDeviceTypes])

  // Real-time dashboard sync, device fetching, and polling handled by hook
  useDashboardRealtime({
    currentDashboard,
    currentDashboardId,
    devicesLength,
    dashboardsLoading,
    dashboardsCount: dashboards.length,
    devicesRef,
    fetchDashboards,
    fetchDevices,
    fetchDevicesCurrentBatch,
  })

  // ==========================================================================
  // URL ↔ Store Sync
  // ==========================================================================

  // URL → Store: When URL dashboardId changes, load that dashboard into store.
  // Uses lastSyncedIdRef to skip when this direction already handled the ID.
  useEffect(() => {
    if (dashboards.length === 0) return

    if (dashboardId) {
      const exists = dashboards.some(d => d.id === dashboardId)
      if (exists && dashboardId !== currentDashboardId && dashboardId !== lastSyncedIdRef.current) {
        lastSyncedIdRef.current = dashboardId
        setCurrentDashboard(dashboardId)
      } else if (!exists && currentDashboardId) {
        navigate(`/visual-dashboard/${currentDashboardId}`, { replace: true })
      } else if (!exists && dashboards.length > 0) {
        const defaultDashboard = dashboards.find(d => d.isDefault) || dashboards[0]
        navigate(`/visual-dashboard/${defaultDashboard.id}`, { replace: true })
      }
    } else if (currentDashboardId) {
      // No dashboardId in URL but store has one — sync to URL (initial load)
      lastSyncedIdRef.current = currentDashboardId
      navigate(`/visual-dashboard/${currentDashboardId}`, { replace: true })
    }
  }, [dashboardId, dashboards])

  // Store → URL: When store currentDashboardId changes (e.g. sidebar click),
  // update URL. Skips when the ID was already synced from the URL direction.
  useEffect(() => {
    if (dashboards.length === 0 || !currentDashboardId) return
    if (currentDashboardId === dashboardId) return
    if (currentDashboardId === lastSyncedIdRef.current) return
    lastSyncedIdRef.current = currentDashboardId
    navigate(`/visual-dashboard/${currentDashboardId}`, { replace: true })
  }, [currentDashboardId])

  // Handle edge case: no dashboards exist yet
  useEffect(() => {
    if (dashboards.length === 0 && !dashboardsLoading && !dashboardId) {
      // Stay at /visual-dashboard without an ID - let user create first dashboard
      navigate('/visual-dashboard', { replace: true })
    }
  }, [dashboards.length, dashboardsLoading, dashboardId, navigate])

  // Config dialog state and handlers (extracted hook)
  const {
    configOpen,
    selectedComponent,
    componentConfig,
    configSchema,
    configTitle,
    handleOpenConfig,
    handleCancelConfig,
    handleSaveConfig,
    handleMapEditorSave,
    handleLayerEditorSave,
    handleCenterPickerSave,
    handleTitleChange,
  } = useComponentConfigDialog({
    currentDashboard,
    updateComponent,
    persistDashboard,
    agents,
    agentsLoading,
    visionModels,
    visionModelsLoading,
    setCenterPickerOpen,
    setMapEditorBindings,
    setMapEditorOpen,
    setLayerEditorBindings,
    setLayerEditorOpen,
  })

  // Load agents — preload on mount, refresh when config opens for agent-monitor-widget
  useEffect(() => {
    // When the config dialog opens for an agent-monitor-widget, force a fresh
    // fetch so newly created agents (e.g. from the Agents page) appear in the
    // dropdown immediately, bypassing the 10s fetchCache TTL.
    if (configOpen && selectedComponent?.type === 'agent-monitor-widget') {
      fetchCache.invalidate('agents-list')
    }
    const loadAgents = async () => {
      if (!fetchCache.shouldFetch('agents-list')) return
      fetchCache.markFetching('agents-list')
      setAgentsLoading(true)
      try {
        const data = await api.listAgentSummaries()
        setAgents(data.agents || [])
        fetchCache.markFetched('agents-list')
      } catch (error) {
        handleError(error, { operation: 'Load agents for dashboard', showToast: false })
        setAgents([])
        fetchCache.invalidate('agents-list')
      } finally {
        setAgentsLoading(false)
      }
    }
    // Preload on mount, or reload when config opens for agent-monitor-widget
    if (!configOpen || selectedComponent?.type === 'agent-monitor-widget') {
      loadAgents()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [configOpen, selectedComponent])

  // NOTE: agents are accessed directly via the `agents` state variable in
  // generateConfigSchema closure — no need to inject them into componentConfig.

  // Load LLM backends available as analysis models for ai-analyst.
  // All backends are listed (multimodal is NOT required — AI Analyst can run on
  // text/numeric data). Multimodal capability is surfaced as an Eye badge in the
  // picker. This matches the runtime AnalystConfigPanel behavior (useAnalystModels).
  useEffect(() => {
    const loadVisionModels = async () => {
      if (configOpen && selectedComponent?.type === 'ai-analyst') {
        setVisionModelsLoading(true)
        try {
          const resp = await api.listLlmBackends()
          const backends = resp?.backends || []
          const models: { id: string; name: string; backendId: string; backendName: string; isMultimodal?: boolean }[] = []
          for (const backend of backends) {
            models.push({
              id: backend.id,
              name: backend.name || backend.model,
              backendId: backend.id,
              backendName: backend.name || backend.id,
              isMultimodal: backend.capabilities?.supports_multimodal ?? false,
            })
          }
          setVisionModels(models)
        } catch (error) {
          handleError(error, { operation: 'Load analysis models for dashboard', showToast: false })
          setVisionModels([])
        } finally {
          setVisionModelsLoading(false)
        }
      }
    }
    loadVisionModels()
  }, [configOpen, selectedComponent?.type, selectedComponent?.id])

  // NOTE: visionModels are accessed directly via the `visionModels` state variable in
  // generateConfigSchema closure — no need to inject them into componentConfig.

  // Note: Removed auto-create dashboard logic
  // Users should explicitly create dashboards via the UI
  // This prevents creating duplicate dashboards on refresh

  // Handle adding a component
  const handleAddComponent = (componentType: string) => {
    const item = componentLibrary
      .flatMap(cat => cat.items)
      .find(i => i.id === componentType)

    // Get size constraints for this component type
    let constraints = COMPONENT_SIZE_CONSTRAINTS[componentType as keyof typeof COMPONENT_SIZE_CONSTRAINTS]

    // For extension components, get constraints from DTO
    if (!constraints) {
      const extensionDto = dynamicRegistry.getMeta(componentType)
      if (extensionDto?.size_constraints) {
        constraints = extensionDto.size_constraints as any
      }
    }

    // Build appropriate default config based on component type
    let defaultConfig: any = {}

    // For extension components, use their default config from DTO
    const extensionDto = dynamicRegistry.getMeta(componentType)
    if (extensionDto?.default_config) {
      defaultConfig = { ...extensionDto.default_config }
    }

    // For extension components, set up dataSource with extensionId
    let dataSource: any = undefined
    if (extensionDto?.extension_id) {
      dataSource = {
        type: 'extension' as const,
        extensionId: extensionDto.extension_id,
      }
      // Also add extensionMetric if available from data_binding
      if (extensionDto.data_binding?.extension_metric) {
        dataSource.extensionMetric = extensionDto.data_binding.extension_metric
      }
    }

    switch (componentType) {
      // Charts
      case 'line-chart':
      case 'area-chart':
        defaultConfig = {
          series: [{ name: 'Value', data: [10, 25, 15, 30, 28, 35, 20], color: chartColorsHex[0] }],
          labels: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
        }
        break
      case 'bar-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      case 'pie-chart':
        defaultConfig = {
          data: [{ name: 'A', value: 30 }, { name: 'B', value: 50 }, { name: 'C', value: 20 }]
        }
        break
      // Indicators
      case 'sparkline':
        defaultConfig = {
          data: [12, 19, 15, 25, 22, 30, 28]
        }
        break
      case 'progress-bar':
        defaultConfig = {
          value: 65,
          min: 0,
          max: 100
        }
        break
      case 'led-indicator':
        defaultConfig = {
          rules: []
        }
        break
      // Controls
      case 'toggle-switch':
        defaultConfig = {
          size: 'md'
        }
        break
      // Display & Content
      case 'image-display':
        defaultConfig = {
          src: '',
          alt: 'Sample Image',
          fit: 'contain',
          rounded: true,
          zoomable: true,
        }
        break
      case 'image-history':
        defaultConfig = {
          dataSource: undefined,
          fit: 'fill',
          rounded: true,
          limit: 50,
          timeRange: 24,
        }
        break
      case 'web-display':
        defaultConfig = {
          src: 'https://example.com',
          title: 'Website',
          sandbox: true,
          showHeader: true,
        }
        break
      case 'markdown-display':
        defaultConfig = {
          content: '# Title\n\nThis is **markdown** content.\n\n- Item 1\n- Item 2\n\n`code example`',
          variant: 'default',
        }
        break
      case 'video-display':
        defaultConfig = {
          src: '',
          type: 'file',
          autoplay: false,
          muted: true,
          controls: true,
          loop: false,
          fit: 'contain',
          rounded: true,
          showFullscreen: true,
        }
        break
      case 'map-display':
        defaultConfig = {
          center: { lat: 39.9042, lng: 116.4074 }, // Beijing
          zoom: 10,
          minZoom: 2,
          maxZoom: 18,
          showControls: true,
          showLayers: true,
          showFullscreen: true,
          interactive: true,
          tileLayer: 'osm',
          markers: [
            { id: '1', latitude: 39.9042, longitude: 116.4074, label: 'Beijing', status: 'online' },
            { id: '2', latitude: 31.2304, longitude: 121.4737, label: 'Shanghai', status: 'online' },
            { id: '3', latitude: 23.1291, longitude: 113.2644, label: 'Guangzhou', status: 'warning' },
          ],
        }
        break
      case 'custom-layer':
        defaultConfig = {
          backgroundType: 'grid',
          gridSize: 20,
          showControls: true,
          showFullscreen: true,
          interactive: true,
          editable: false,
        }
        break
      // Business Components
      case 'agent-monitor-widget':
      case 'ai-analyst':
        defaultConfig = {}
        break
      default:
        // For extension components, keep the default_config from DTO (already set above)
        if (Object.keys(defaultConfig).length === 0) {
          defaultConfig = {}
        }
    }

    const w = constraints?.defaultW ?? 4
    const h = constraints?.defaultH ?? 3

    // Calculate y position based on existing components.
    // We use the bottom edge of the lowest existing component to place
    // the new component below everything currently visible.
    const existingComponents = currentDashboard?.components ?? []
    let maxY = 0
    for (const c of existingComponents) {
      const bottom = (c.position?.y ?? 0) + (c.position?.h ?? 1)
      if (bottom > maxY) maxY = bottom
    }

    const newComponent: Omit<DashboardComponent, 'id'> = {
      type: componentType as any,
      position: {
        x: 0,
        y: maxY,
        w,
        h,
        minW: constraints?.minW,
        minH: constraints?.minH,
        maxW: constraints?.maxW,
        maxH: constraints?.maxH,
      },
      title: item?.name || componentType,
      config: defaultConfig,
      ...(dataSource && { dataSource }),
    }

    addComponent(newComponent)
    setComponentLibraryOpen(false)
  }

  // Handle layout change - batch update all changed positions in a single store update
  // to prevent infinite re-render loops (store update → grid recalc → onLayoutChange → ...)
  const handleLayoutChange = useCallback((layout: readonly any[]) => {
    const components = currentDashboard?.components ?? []
    const changed: Array<{ id: string; position: { x: number; y: number; w: number; h: number } }> = []

    for (const item of layout) {
      const existing = components.find(c => c.id === item.i)
      if (!existing) continue
      if (
        existing.position.x !== item.x ||
        existing.position.y !== item.y ||
        existing.position.w !== item.w ||
        existing.position.h !== item.h
      ) {
        changed.push({
          id: item.i,
          position: { x: item.x, y: item.y, w: item.w, h: item.h },
        })
      }
    }

    if (changed.length > 0) {
      batchUpdatePositions(changed)
    }
  }, [currentDashboard?.components, batchUpdatePositions])

  // Memoize grid components to prevent infinite re-renders
  // Only recalculate when actual component data changes (detected via stableKey)
  // Note: handleOpenConfig, removeComponent, duplicateComponent are NOT dependencies
  // because they don't affect the rendered output structure, only event handlers
  // devices.length is deliberately excluded from deps — it changes every 3s due to
  // batch polling and is NOT used in this callback (component data comes via useDataSource).
  // IMPORTANT: Use currentDashboard from props (reactive) to ensure updates are reflected
  const gridComponents = useMemo(() => {
    return (currentDashboard?.components ?? []).map((component) => {
      // Get dataSource from component (it should be a separate property, not in config)
      const componentDataSource = (component as any).dataSource

      // Create callbacks for this component to persist configuration changes
      const handleDataSourceChange = (newDataSource: any) => {
        updateComponent(component.id, { dataSource: newDataSource as DataSource }, false)
      }

      const handleConfigChange = (newConfig: Record<string, any>) => {
        updateComponent(component.id, { config: newConfig }, false)
      }

      return {
        id: component.id,
        position: component.position,
        children: (
          <ComponentWrapper
            key={component.id}
            component={component}
            editMode={editMode}
            onOpenConfig={handleOpenConfig}
            onRemove={removeComponent}
            onDuplicate={duplicateComponent}
            onSelect={isMobile ? (comp) => {
              if (comp) {
                setMobileSelectedId(comp.id)
                setMobileEditBarOpen(true)
              }
            } : undefined}
            selectedComponentId={mobileSelectedId}
            isMobile={isMobile}
          >
            {renderDashboardComponent(component, editMode, handleDataSourceChange, handleConfigChange, openExtFullscreen, closeExtFullscreen)}
          </ComponentWrapper>
        ),
      }
    }) ?? []
  }, [componentsStableKey, editMode, isMobile, installedComponents.length])

  if (!currentDashboard) {
    // Show loading skeleton while dashboards are loading OR
    // while we have dashboards but haven't resolved currentDashboard yet (prevents flash)
    if (dashboardsLoading || (dashboards.length > 0 && !currentDashboardId)) {
      const isTabMode = layoutMode === 'tabs'
      return (
        <div className="flex h-screen">
          {/* Skeleton sidebar (only in sidebar mode) */}
          {!isTabMode && (
            <div className="hidden lg:flex w-64 shrink-0 flex-col border-r p-4 space-y-3">
              <Skeleton className="h-8 w-full rounded-lg" />
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full rounded-lg" />
              ))}
            </div>
          )}
          {/* Skeleton main content */}
          <div className="flex-1 flex flex-col">
            <div className="flex items-center justify-between px-4 h-11 ">
              {isTabMode ? (
                <div className="flex items-center gap-2 flex-1 min-w-0">
                  {/* Left: toggle + add skeletons */}
                  <Skeleton className="h-7 w-7 rounded-md shrink-0" />
                  <Skeleton className="h-7 w-7 rounded-md shrink-0" />
                  <div className="h-5 w-px bg-border shrink-0" />
                  {/* Middle: tab skeletons */}
                  <div className="flex items-center gap-0.5 flex-1 min-w-0">
                    {Array.from({ length: 4 }).map((_, i) => (
                      <Skeleton key={i} className="h-7 w-24 rounded-md shrink-0" />
                    ))}
                  </div>
                </div>
              ) : (
                <Skeleton className="h-7 w-40" />
              )}
              <div className="flex gap-2 shrink-0">
                <Skeleton className="h-9 w-9 rounded-lg" />
                <Skeleton className="h-9 w-9 rounded-lg" />
              </div>
            </div>
            <div className="flex-1 p-6 space-y-4">
              <div className="grid grid-cols-[repeat(auto-fill,minmax(max(160px,(100%/6-10px)),1fr))] gap-4">
                {Array.from({ length: 6 }).map((_, i) => (
                  <div key={i} className="border rounded-lg p-4 space-y-3">
                    <Skeleton className="h-4 w-2/3" />
                    <Skeleton className="h-20 w-full" />
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
      )
    }

    // No dashboard found - guided empty state: manual create is primary,
    // "let AI build it" routes to chat (which carries its own model setup
    // guidance if no LLM backend exists yet).
    return (
      <div className="flex items-center justify-center h-screen bg-background">
        <div className="text-center max-w-md px-6">
          <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-xl bg-primary-light text-primary">
            <LayoutDashboard className="size-7" />
          </div>
          <h2 className="mb-2 text-lg font-semibold tracking-tight">
            {t('visualDashboard.noDashboardFound')}
          </h2>
          <p className="text-sm text-muted-foreground mb-6 leading-relaxed">
            {t('visualDashboard.emptyGuideDesc')}
          </p>
          <div className="flex items-center justify-center gap-3">
            <Button
              className="h-11 sm:h-10"
              onClick={() => {
                handleDashboardCreate('Overview').catch((err) => {
                  console.error('[VisualDashboard] Failed to create dashboard:', err)
                })
              }}
            >
              <Plus className="h-4 w-4 mr-1" />
              {t('visualDashboard.createDashboard')}
            </Button>
            <Button
              variant="secondary"
              className="h-11 sm:h-10 gap-2"
              onClick={() => navigate('/chat')}
            >
              <Sparkles className="h-4 w-4" />
              {t('visualDashboard.emptyAskAi')}
            </Button>
          </div>
          {/* Three-step path so a first-time user knows what "building a
              dashboard" involves before clicking. */}
          <div className="mt-6 flex items-center justify-center gap-x-2 text-xs text-muted-foreground">
            {[
              t('visualDashboard.emptyStep1'),
              t('visualDashboard.emptyStep2'),
              t('visualDashboard.emptyStep3'),
            ].map((label, i) => (
              <span key={label} className="flex items-center gap-1.5 whitespace-nowrap">
                {i > 0 && <ChevronRight className="size-3 opacity-50 mr-1" />}
                <span className="flex size-4 items-center justify-center rounded-full bg-muted text-[10px] font-medium">
                  {i + 1}
                </span>
                {label}
              </span>
            ))}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full overflow-hidden bg-background">
      {/* Fullscreen portal */}
      {isFullscreen && createPortal(
        <div className="fixed inset-0 z-[100] bg-background flex flex-col">
          <div className="flex-1 overflow-auto p-4 relative">
            <Button
              variant="outline"
              size="icon"
              onClick={toggleFullscreen}
              className="absolute top-4 right-4 z-50 shadow-lg bg-bg-90 backdrop-blur"
              title={t('visualDashboard.exitFullscreen')}
            >
              <Minimize className="h-4 w-4" />
            </Button>
            <DashboardGrid
              components={gridComponents}
              editMode={false}
              onLayoutChange={() => {}}
            />
          </div>
        </div>,
        getPortalRoot()
      )}

      {/* Sidebar - separate column (only in sidebar layout mode). On desktop
          it's hoisted to the shell's full-height slot (left of the TopBar,
          level with the AppSidebar); mobile keeps the drawer. */}
      {!isFullscreen && layoutMode === 'sidebar' && (
        isDesktop && pageSidebarSlot ? createPortal(
          <PageSidebarColumn>
            <DashboardListSidebar
              dashboards={sortedDashboards}
              currentDashboardId={currentDashboardId}
              onSwitch={handleDashboardSwitch}
              onCreate={handleDashboardCreate}
              onRename={handleDashboardRename}
              onDuplicate={handleDashboardDuplicate}
              onDelete={handleDashboardDelete}
              onReorder={(newOrder) => useStore.getState().reorderDashboards(newOrder)}
              open={sidebarOpen}
              onOpenChange={setSidebarOpen}
              isDesktop={true}
              onSwitchToTabs={handleSwitchToTabs}
            />
          </PageSidebarColumn>,
          pageSidebarSlot
        ) : (
          <DashboardListSidebar
            dashboards={sortedDashboards}
            currentDashboardId={currentDashboardId}
            onSwitch={handleDashboardSwitch}
            onCreate={handleDashboardCreate}
            onRename={handleDashboardRename}
            onDuplicate={handleDashboardDuplicate}
            onDelete={handleDashboardDelete}
            onReorder={(newOrder) => useStore.getState().reorderDashboards(newOrder)}
            open={sidebarOpen}
            onOpenChange={setSidebarOpen}
            isDesktop={isDesktop}
            onSwitchToTabs={handleSwitchToTabs}
          />
        )
      )}

      {/* Main content area */}
      <div className={cn("flex-1 flex flex-col overflow-hidden", isFullscreen && "hidden")}>
        {/* Mobile per-page header: hamburger (opens nav drawer) + generic
            page title. The specific dashboard name + switcher lives in
            DashboardToolbar below to avoid duplication. */}
        {isMobile && (
          <MobilePageHeader title={t('common:nav.visual-dashboard')} />
        )}
        {/* Desktop title row — identical to PageLayout's so the floating
            global controls (top-right) align with every other page. */}
        {!isMobile && (
          <div className="shrink-0 bg-background">
            <div className="w-full px-4 pt-4 pb-2 sm:px-6 sm:pt-5 sm:pb-3 md:px-8 md:pt-6 md:pb-3">
              <PageHeader title={t('common:nav.visual-dashboard')} />
            </div>
          </div>
        )}
        <DashboardToolbar
          sortedDashboards={sortedDashboards}
          currentDashboardId={currentDashboardId}
          currentDashboard={currentDashboard}
          layoutMode={layoutMode}
          onDashboardSwitch={handleDashboardSwitch}
          onDashboardCreate={handleDashboardCreate}
          onDashboardRename={handleDashboardRename}
          onDashboardDuplicate={handleDashboardDuplicate}
          onDashboardDelete={handleDashboardDelete}
          onDashboardReorder={(newOrder) => useStore.getState().reorderDashboards(newOrder)}
          onSwitchToSidebar={handleSwitchToSidebar}
          editMode={editMode}
          setEditMode={setEditMode}
          isMobile={isMobile}
          setMobileSelectedId={setMobileSelectedId}
          setMobileEditBarOpen={setMobileEditBarOpen}
          onOpenShare={() => setShareDialogOpen(true)}
          onToggleFullscreen={toggleFullscreen}
          componentLibraryOpen={componentLibraryOpen}
          setComponentLibraryOpen={setComponentLibraryOpen}
          libraryTab={libraryTab}
          onLibraryTabChange={setLibraryTab}
          librarySearch={librarySearch}
          onLibrarySearchChange={setLibrarySearch}
          filteredLibrary={filteredLibrary}
          onAddComponent={handleAddComponent}
          marketComponents={marketComponents}
          marketLoading={marketLoading}
          installedComponents={installedComponents}
          installingId={installingId}
          onInstall={installFromMarket}
          onUninstall={uninstallComponent}
          onRefreshComponent={refreshComponentAction}
          onSetInstalling={setInstallingId}
          updatesAvailable={updatesAvailable}
          importDialogOpen={importDialogOpen}
          onImportDialogOpenChange={setImportDialogOpen}
        />

        {/* Dashboard Grid */}
        <div className="flex-1 overflow-auto p-4 relative">
          {(currentDashboard.components?.length ?? 0) === 0 ? (
            <div className="h-full flex flex-col items-center justify-center text-muted-foreground">
              <LayoutDashboard className="h-16 w-16 mb-4 opacity-50" />
              <p className="text-lg font-medium">{t('visualDashboard.emptyDashboard')}</p>
              <p className="text-sm mt-2">
                {editMode ? t('visualDashboard.addComponentsHint') : t('visualDashboard.enterEditModeHint')}
              </p>
              {editMode ? (
                <Button
                  variant="outline"
                  size="sm"
                  className="mt-4"
                  onClick={() => setComponentLibraryOpen(true)}
                >
                  <Plus className="h-4 w-4 mr-1" />
                  {t('visualDashboard.addComponent')}
                </Button>
              ) : (
                <Button
                  variant="default"
                  size="sm"
                  className="mt-4"
                  onClick={() => setEditMode(true)}
                >
                  <Settings2 className="h-4 w-4 mr-1" />
                  {t('common:editDashboard')}
                </Button>
              )}
            </div>
          ) : (
            <DashboardGrid
              components={gridComponents}
              editMode={editMode}
              onLayoutChange={handleLayoutChange}
            />
          )}
        </div>
      </div>

      {/* Config Dialog */}
      <ComponentConfigDialog
        open={configOpen}
        onClose={handleCancelConfig}
        onSave={handleSaveConfig}
        title={configTitle}
        onTitleChange={handleTitleChange}
        configSchema={configSchema}
        componentType={selectedComponent?.type || ''}
        previewDataSource={componentConfig.dataSource}
        previewConfig={componentConfig}
        showTitleInDisplay={isTitleInDisplayComponent(selectedComponent?.type)}
        position={selectedComponent?.position}
      />

      {/* Map Editor Dialog */}
      <MapEditorDialog
        open={mapEditorOpen}
        onOpenChange={setMapEditorOpen}
        bindings={mapEditorBindings}
        center={(componentConfig.center as { lat: number; lng: number }) || { lat: 39.9042, lng: 116.4074 }}
        zoom={componentConfig.zoom as number || 10}
        tileLayer={componentConfig.tileLayer as string || 'osm'}
        onSave={handleMapEditorSave}
      />

      {/* Center Picker Dialog */}
      <CenterPickerDialog
        open={centerPickerOpen}
        onOpenChange={setCenterPickerOpen}
        center={(componentConfig.center as { lat: number; lng: number }) || { lat: 39.9042, lng: 116.4074 }}
        zoom={componentConfig.zoom as number || 10}
        tileLayer={componentConfig.tileLayer as string || 'osm'}
        onSave={handleCenterPickerSave}
      />

      {/* Layer Editor Dialog */}
      <LayerEditorDialog
        open={layerEditorOpen}
        onOpenChange={setLayerEditorOpen}
        bindings={layerEditorBindings}
        backgroundType={componentConfig.backgroundType as 'color' | 'image' | 'transparent' | 'grid' || 'grid'}
        backgroundColor={componentConfig.backgroundColor as string}
        backgroundImage={componentConfig.backgroundImage as string}
        onSave={handleLayerEditorSave}
      />

      {/* Mobile Edit Bar */}
      {isMobile && mobileEditBarOpen && mobileSelectedId && (
        <MobileEditBar
          isOpen={mobileEditBarOpen}
          onClose={() => {
            setMobileEditBarOpen(false)
            setMobileSelectedId(null)
          }}
          onSettings={() => {
            const comp = currentDashboard?.components.find(c => c.id === mobileSelectedId)
            if (comp) {
              handleOpenConfig(comp.id)
              setMobileEditBarOpen(false)
            }
          }}
          onCopy={() => {
            if (mobileSelectedId) {
              duplicateComponent(mobileSelectedId)
              setMobileEditBarOpen(false)
              setMobileSelectedId(null)
            }
          }}
          onDelete={() => {
            if (mobileSelectedId) {
              removeComponent(mobileSelectedId)
              setMobileEditBarOpen(false)
              setMobileSelectedId(null)
            }
          }}
          componentName={currentDashboard?.components.find(c => c.id === mobileSelectedId)?.title}
        />
      )}

      {/* Share Management Dialog */}
      <ShareManagerDialog
        open={shareDialogOpen}
        onOpenChange={setShareDialogOpen}
        dashboardId={currentDashboardId}
        dashboardName={currentDashboard?.name}
      />

      {/* Extension Fullscreen Dialog */}
      <FullScreenDialog
        open={!!extFullscreenContent}
        onOpenChange={(open) => { if (!open) closeExtFullscreen() }}
      >
        <FullScreenDialogHeader
          icon={<Monitor className="w-5 h-5" />}
          title="Edit Content"
          onClose={closeExtFullscreen}
        />
        <FullScreenDialogContent>
          {extFullscreenContent}
        </FullScreenDialogContent>
      </FullScreenDialog>
    </div>
  )
})

// Export the memoized component
export { VisualDashboardMemo as VisualDashboard }

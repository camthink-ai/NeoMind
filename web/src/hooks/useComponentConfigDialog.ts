/**
 * useComponentConfigDialog
 *
 * Manages the component configuration dialog state and all related handlers:
 * - Opening/closing the config dialog
 * - Live preview (updating store in real-time as config changes)
 * - Saving / canceling config
 * - Map editor, layer editor, center picker save handlers
 * - Title change
 * - Schema generation
 *
 * The hook owns all config dialog state. External dependencies (agents,
 * vision models, dialog setters) are passed in as parameters.
 */

import { useState, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useStore } from '@/store'
import { clearTelemetryCache } from '@/hooks/useDataSource/fetch'
import { getSourceId } from '@/types/dashboard'
import type { Dashboard, DashboardComponent } from '@/types/dashboard'
import type { ComponentConfigSchema } from '@/components/dashboard/config/ComponentConfigBuilder'
import type { MapBinding } from '@/components/dashboard/generic/MapEditorDialog'
import type { LayerBinding } from '@/components/dashboard/generic/CustomLayer'
import { generateConfigSchema as _generateConfigSchema } from '@/pages/dashboard-components/configSchemas'

export interface UseComponentConfigDialogParams {
  currentDashboard: Dashboard | null

  // Store actions
  updateComponent: (id: string, updates: Partial<DashboardComponent>, immediate?: boolean) => void
  persistDashboard: () => Promise<void>

  // External state for schema generation
  agents: { id: string; name: string; status: string }[]
  agentsLoading: boolean
  visionModels: { id: string; name: string; backendId: string; backendName: string; isMultimodal?: boolean }[]
  visionModelsLoading: boolean

  // Dialog setters for map/layer/center editors (state owned by main component)
  setCenterPickerOpen: (open: boolean) => void
  setMapEditorBindings: (bindings: MapBinding[]) => void
  setMapEditorOpen: (open: boolean) => void
  setLayerEditorBindings: (bindings: LayerBinding[]) => void
  setLayerEditorOpen: (open: boolean) => void
}

export interface UseComponentConfigDialogReturn {
  configOpen: boolean
  selectedComponent: DashboardComponent | null
  componentConfig: Record<string, any>
  configSchema: ComponentConfigSchema | null
  configTitle: string
  handleOpenConfig: (componentId: string) => void
  handleCancelConfig: () => void
  handleSaveConfig: () => Promise<void>
  handleMapEditorSave: (bindings: MapBinding[]) => Promise<void>
  handleLayerEditorSave: (bindings: LayerBinding[]) => Promise<void>
  handleCenterPickerSave: (center: { lat: number; lng: number }) => Promise<void>
  handleTitleChange: (title: string) => void
}

export function useComponentConfigDialog(params: UseComponentConfigDialogParams): UseComponentConfigDialogReturn {
  const {
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
  } = params

  const { t } = useTranslation('dashboardComponents')

  // Config dialog state
  const [configOpen, setConfigOpen] = useState(false)
  const [selectedComponent, setSelectedComponent] = useState<DashboardComponent | null>(null)
  const [configTitle, setConfigTitle] = useState('')
  const [componentConfig, setComponentConfig] = useState<Record<string, any>>({})
  const [configSchema, setConfigSchema] = useState<ComponentConfigSchema | null>(null)

  // Store original config for revert on cancel
  const [originalComponentConfig, setOriginalComponentConfig] = useState<Record<string, any>>({})
  const [originalTitle, setOriginalTitle] = useState('')

  // Track initial config load to avoid unnecessary updates
  const initialConfigRef = useRef<any>(null)
  const isInitialLoad = useRef(false)
  const lastSyncedConfigRef = useRef<string>('')

  // Generate config schema based on component type
  const generateConfigSchema = (componentType: string, currentConfig: any): ComponentConfigSchema | null => {
    return _generateConfigSchema(componentType, currentConfig, {
      setConfigTitle,
      selectedComponent,
      updateComponent,
      setComponentConfig,
      t,
      agents,
      currentDashboard,
      setCenterPickerOpen,
      setMapEditorBindings,
      setMapEditorOpen,
      setLayerEditorBindings,
      setLayerEditorOpen,
      agentsLoading,
      visionModels,
      visionModelsLoading,
    })
  }

  // Handle opening config dialog
  const handleOpenConfig = useCallback((componentId: string) => {
    const component = currentDashboard?.components.find(c => c.id === componentId)
    if (!component) return

    setSelectedComponent(component)
    // Extract both config and dataSource (they are separate properties on GenericComponent)
    const config = { ...((component as any).config || {}) }
    const dataSource = (component as any).dataSource
    // Include title in config so style sections can access it
    const configWithTitle = { ...config, title: component.title }
    // Merge dataSource into config for unified state management
    const mergedConfig = dataSource ? { ...configWithTitle, dataSource } : configWithTitle

    // Store original config for revert on cancel
    setOriginalComponentConfig(mergedConfig)
    setOriginalTitle(component.title || '')

    setConfigTitle(component.title || '')
    setComponentConfig(mergedConfig)
    setConfigOpen(true)
  }, [currentDashboard?.components])

  // Live preview: update component in real-time as config changes
  // Applies changes to the store immediately so the grid preview updates.
  // Schema is regenerated so the dialog's own inputs stay responsive.
  useEffect(() => {
    if (configOpen && selectedComponent) {
      // Skip initial load - don't update store with same config
      if (!isInitialLoad.current) {
        initialConfigRef.current = componentConfig
        isInitialLoad.current = true
        lastSyncedConfigRef.current = JSON.stringify(componentConfig)
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
        return
      }

      // Check if config actually changed since last sync
      const currentJSON = JSON.stringify(componentConfig)
      if (currentJSON !== lastSyncedConfigRef.current) {
        // Regenerate schema immediately so input values update
        setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))

        // Update last synced config
        lastSyncedConfigRef.current = currentJSON

        // Apply to store immediately for live preview in the grid
        const { dataSource, ...configOnly } = componentConfig
        const currentDS = (selectedComponent as any).dataSource
        const updateData: any = { config: configOnly }
        if (dataSource !== undefined || currentDS !== undefined) {
          updateData.dataSource = dataSource
        }
        updateComponent(selectedComponent.id, updateData, false)
      }
    } else {
      // Reset when dialog closes
      isInitialLoad.current = false
      initialConfigRef.current = null
      lastSyncedConfigRef.current = ''
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [componentConfig, configOpen, selectedComponent?.id, selectedComponent?.type, updateComponent])

  // Regenerate schema when async-loaded context (agents / visionModels) arrives.
  // Without this, the schema is built once with empty arrays at dialog-open time
  // and never refreshes when the fetch resolves — leaving dropdowns empty.
  useEffect(() => {
    if (configOpen && selectedComponent && isInitialLoad.current) {
      setConfigSchema(generateConfigSchema(selectedComponent.type, componentConfig))
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [visionModels, visionModelsLoading, agents, agentsLoading])

  // Handle canceling component config - revert to original
  const handleCancelConfig = useCallback(() => {
    if (selectedComponent && originalComponentConfig) {
      // Read the LIVE component from the store. `selectedComponent` is the
      // snapshot taken at dialog-open time — its dataSource predates any
      // binding the live preview added, so a newly-bound dataSource would
      // slip through the check below and survive the cancel.
      const liveDashboard = useStore.getState().currentDashboard
      const liveComponent = liveDashboard?.components.find(c => c.id === selectedComponent.id)
      const currentDS = (liveComponent as any)?.dataSource

      // Revert to original config (no need to persist - reverting to saved state)
      const { dataSource, ...configOnly } = originalComponentConfig
      const updateData: any = { config: configOnly }
      // Include dataSource if:
      // 1. Original config had dataSource, OR
      // 2. The live component has one (added via live preview — must be cleared)
      if (dataSource !== undefined || currentDS !== undefined) {
        updateData.dataSource = dataSource
      }
      updateComponent(selectedComponent.id, updateData, false)

      // Revert title
      if (originalTitle !== (liveComponent?.title ?? selectedComponent.title)) {
        updateComponent(selectedComponent.id, { title: originalTitle }, false)
      }
    }
    setConfigOpen(false)
  }, [selectedComponent, originalComponentConfig, originalTitle, updateComponent])

  // Handle saving component config - persist the dashboard to localStorage
  const handleSaveConfig = async () => {
    if (selectedComponent) {
      // Get the latest component from the store to merge with local changes
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      // Extract dataSource — only from authoritative locations:
      // 1. componentConfig.dataSource (newly selected/changed in config dialog)
      // 2. latestComponent.dataSource (existing on component as separate property)
      // Do NOT read from nested config.dataSource — the migration moved it to top-level,
      // and reading the nested one can restore a dataSource the user intentionally cleared.
      const configDataSource = componentConfig.dataSource
      const latestComponentDataSource = (latestComponent as any)?.dataSource

      // Use explicit null check: if user cleared dataSource (set to null/undefined), respect that.
      // Only fall back to the latest component dataSource if config didn't touch it at all.
      const finalDataSource = configDataSource !== undefined
        ? configDataSource
        : latestComponentDataSource

      // Merge local config changes with the latest component config
      const mergedConfig = {
        ...(latestComponent as any)?.config || {},
        ...componentConfig,
      }

      // IMPORTANT: Remove dataSource from mergedConfig to avoid confusion
      // dataSource should be stored as a separate property, not inside config
      delete (mergedConfig as any).dataSource

      // Remove runtime-only fields that should never be persisted
      delete (mergedConfig as any).editMode

      // Update the component in the store
      // CRITICAL: dataSource must be saved as a separate property, not inside config
      const updateData: any = {
        config: mergedConfig,
        title: configTitle,
      }
      if (finalDataSource !== undefined) {
        updateData.dataSource = finalDataSource
      }

      // 1. Save clean data (without _saveTs) to the store for persistence
      updateComponent(selectedComponent.id, updateData, false)

      // 2. Persist to storage — clean dataSource is saved
      await persistDashboard()

      // 3. Force telemetry cache refresh so dashboard components re-fetch with new settings
      clearTelemetryCache()

      // 4. Stamp dataSource with a unique timestamp to force re-render.
      //    This is done AFTER persist so _saveTs is not stored to backend.
      //    The stamp triggers: componentsStableKey change → gridComponents rebuild → useDataSource re-fetch.
      if (finalDataSource !== undefined) {
        const saveTs = Date.now()
        const stampedDataSource = Array.isArray(finalDataSource)
          ? finalDataSource.map((ds: any) => ({ ...ds, _saveTs: saveTs }))
          : { ...(finalDataSource as any), _saveTs: saveTs }
        updateComponent(selectedComponent.id, { dataSource: stampedDataSource }, false)
      }
    }
    setConfigOpen(false)
  }

  // Handle saving map editor bindings
  const handleMapEditorSave = async (bindings: MapBinding[]) => {
    // Fix any duplicate IDs in bindings before saving
    const idCount = new Map<string, number>() as Map<string, number>
    const fixedBindings = bindings.map((binding, index) => {
      const ds = binding.dataSource as any
      const currentId = binding.id
      idCount.set(currentId, (idCount.get(currentId) || 0) + 1)

      // If ID is duplicated, regenerate it
      if (idCount.get(currentId)! > 1) {
        let newId: string
        if (binding.type === 'metric' || ds?.type === 'telemetry') {
          newId = `metric-${getSourceId(ds)}-${ds?.metricId || ds?.property || index}`
        } else if (binding.type === 'command') {
          newId = `command-${getSourceId(ds)}-${ds?.command}`
        } else {
          newId = `device-${getSourceId(ds)}-${index}`
        }
        return { ...binding, id: newId }
      }
      return binding
    })

    if (selectedComponent) {
      // CRITICAL FIX: Get the latest component config from the store to avoid stale state
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      const latestConfig = (latestComponent as any)?.config || {}
      const latestDataSource = (latestComponent as any)?.dataSource

      // Merge the latest config with the new bindings, preserving dataSource
      const newConfig = { ...latestConfig, bindings: fixedBindings }
      const updateData: any = { config: newConfig }

      // CRITICAL: Preserve dataSource when updating
      if (latestDataSource) {
        updateData.dataSource = latestDataSource
      }

      // Update the store with both config and dataSource
      updateComponent(selectedComponent.id, updateData, false)

      // Update local config state
      setComponentConfig(prev => ({ ...prev, bindings: fixedBindings }))
    }

    // Persist to localStorage
    await persistDashboard()

    setMapEditorOpen(false)
  }

  // Handle saving layer editor bindings
  const handleLayerEditorSave = async (bindings: LayerBinding[]) => {
    if (selectedComponent) {
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      const latestConfig = (latestComponent as any)?.config || {}
      const latestDataSource = (latestComponent as any)?.dataSource

      // Merge the latest config with the new bindings, preserving dataSource
      const newConfig = { ...latestConfig, bindings }
      const updateData: any = { config: newConfig }

      // Preserve dataSource when updating
      if (latestDataSource) {
        updateData.dataSource = latestDataSource
      }

      // Update the store
      updateComponent(selectedComponent.id, updateData, false)

      // Force re-render

      // Update local config state
      setComponentConfig(prev => ({ ...prev, bindings }))
    }

    // Persist to localStorage
    await persistDashboard()

    setLayerEditorOpen(false)
  }

  // Handle saving center picker
  const handleCenterPickerSave = async (newCenter: { lat: number; lng: number }) => {
    if (selectedComponent) {
      const latestDashboard = useStore.getState().currentDashboard
      const latestComponent = latestDashboard?.components.find(c => c.id === selectedComponent.id)

      const latestConfig = (latestComponent as any)?.config || {}
      const latestDataSource = (latestComponent as any)?.dataSource

      // Merge the latest config with the new center, preserving dataSource
      const newConfig = { ...latestConfig, center: newCenter }
      const updateData: any = { config: newConfig }

      // Preserve dataSource when updating
      if (latestDataSource) {
        updateData.dataSource = latestDataSource
      }

      // Update the store
      updateComponent(selectedComponent.id, updateData, false)

      // Force re-render

      // Update local config state
      setComponentConfig(prev => ({ ...prev, center: newCenter }))
    }

    // Persist to localStorage
    await persistDashboard()

    setCenterPickerOpen(false)
  }

  // Handle title change (local state only — store updated via debounced live preview)
  const handleTitleChange = (newTitle: string) => {
    setConfigTitle(newTitle)
  }

  return {
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
  }
}

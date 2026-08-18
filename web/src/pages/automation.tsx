/**
 * NeoMind Automation Page
 *
 * Automation interface with rules and data transforms.
 * AI Agents are now managed separately in /agents page.
 * Uses PageLayout + PageTabs structure consistent with other pages.
 */

import { useState, useCallback, useEffect, useRef, useMemo } from "react"
import { useDataVersion } from "@/hooks/useDataVersion"
import { useTranslation } from "react-i18next"
import { useNavigate, useLocation } from "react-router-dom"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabsBar, PageTabsContent, PageTabsBottomNav, Pagination } from "@/components/shared"
import { Sparkles, GitBranch, Download, Upload, MoreVertical } from "lucide-react"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { showErrorToast } from "@/lib/error-messages"
import { useIsMobile } from "@/hooks/useMobile"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
} from "@/components/ui/dropdown-menu"
import type { TransformAutomation, Rule, Extension, ExtensionDataSourceInfo, TransformDataSourceInfo } from "@/types"

// Import split-pane builder components
import { SimpleRuleBuilderSplit } from "@/components/automation/SimpleRuleBuilderSplit"
import { TransformBuilder as TransformBuilderSplit } from "@/components/automation/TransformBuilderSplit"
import { RuleTemplatePicker } from "@/components/automation/dialog/RuleTemplatePicker"
import { DeviceOfflineTemplateDialog } from "@/components/automation/dialog/DeviceOfflineTemplateDialog"

// Import list components
import { RulesList, ITEMS_PER_PAGE as RULES_ITEMS_PER_PAGE } from "./automation-components/RulesList"
import { RuleDetailDialog } from "./automation-components/RuleDetailDialog"
import { TransformsList, ITEMS_PER_PAGE as TRANSFORMS_ITEMS_PER_PAGE } from "./automation-components/TransformsList"

type AutomationTab = 'rules' | 'transforms'

export function AutomationPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAuto } = useTranslation('automation')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()
  const isMobile = useIsMobile()

  // Router integration
  const navigate = useNavigate()
  const location = useLocation()

  // Get tab from URL path
  const getTabFromPath = (): AutomationTab => {
    const pathSegments = location.pathname.split('/')
    const lastSegment = pathSegments[pathSegments.length - 1]
    if (lastSegment === 'transforms') {
      return 'transforms'
    }
    return 'rules'
  }

  // Active tab state - sync with URL
  const [activeTab, setActiveTab] = useState<AutomationTab>(getTabFromPath)

  // Update tab when URL changes
  useEffect(() => {
    const tabFromPath = getTabFromPath()
    setActiveTab(tabFromPath)
  }, [location.pathname])

  // Update URL when tab changes
  const handleTabChange = (tab: AutomationTab) => {
    setActiveTab(tab)
    if (tab === 'rules') {
      navigate('/automation')
    } else {
      navigate(`/automation/${tab}`)
    }
  }

  // Builder states
  const [showRuleDialog, setShowRuleDialog] = useState(false)
  const [showTransformDialog, setShowTransformDialog] = useState(false)
  const [showTemplatePicker, setShowTemplatePicker] = useState(false)
  const [showOfflineDialog, setShowOfflineDialog] = useState(false)

  // Editing states
  const [editingRule, setEditingRule] = useState<Rule | undefined>(undefined)
  const [editingTransform, setEditingTransform] = useState<TransformAutomation | undefined>(undefined)

  // Data state
  const [rules, setRules] = useState<Rule[]>([])
  const [transforms, setTransforms] = useState<TransformAutomation[]>([])
  const [loading, setLoading] = useState(false)

  // Pagination state
  const [rulesPage, setRulesPage] = useState(1)
  const [transformsPage, setTransformsPage] = useState(1)

  // Resources for dialogs (lazy-loaded)
  const [devices, setDevices] = useState<any[]>([])
  const [deviceTypes, setDeviceTypes] = useState<any[]>([])
  const [ruleDevices, setRuleDevices] = useState<any[]>([])  // Devices with metrics for rules
  const [extensions, setExtensions] = useState<Extension[]>([])
  const [extensionDataSources, setExtensionDataSources] = useState<ExtensionDataSourceInfo[]>([])
  const [transformDataSources, setTransformDataSources] = useState<TransformDataSourceInfo[]>([])
  const [messageChannels, setMessageChannels] = useState<Array<{ name: string; type: string; enabled: boolean }>>([])
  const [agents, setAgents] = useState<Array<{ id: string; name: string }>>([])
  const [resourcesLoaded, setResourcesLoaded] = useState(false)
  const [resourcesLoading, setResourcesLoading] = useState(false)

  // Load tab-specific list only (fast, minimal API calls)
  const loadTabData = useCallback(async () => {
    setLoading(true)
    try {
      if (activeTab === 'rules') {
        const data = await api.listRules()
        if (data && 'rules' in data) {
          setRules((data.rules || []).sort((a: Rule, b: Rule) => {
            const aTime = typeof a.created_at === 'string' ? new Date(a.created_at).getTime() : a.created_at
            const bTime = typeof b.created_at === 'string' ? new Date(b.created_at).getTime() : b.created_at
            return bTime - aTime
          }))
        }
      } else if (activeTab === 'transforms') {
        const data = await api.listTransforms()
        if (data && 'transforms' in data) {
          setTransforms((data.transforms || []).sort((a: TransformAutomation, b: TransformAutomation) => b.created_at - a.created_at))
        }
      }
    } catch (error) {
      handleError(error, { operation: `Load ${activeTab}`, showToast: false })
    } finally {
      setLoading(false)
    }
  }, [activeTab, handleError])

  // Lazy-load dialog resources only when needed (first dialog open)
  const loadResources = useCallback(async () => {
    if (resourcesLoaded || resourcesLoading) return
    setResourcesLoading(true)
    try {
      const [devicesData, typesResult, resourcesResult, extResult, channelsResult, agentsResult] = await Promise.all([
        api.getDevices().catch((): any => ({ devices: [] })),
        api.getDeviceTypes().catch((): any => ({ device_types: [] })),
        api.getRuleResources().catch((): any => ({ devices: [] })),
        Promise.all([
          api.listExtensions().catch((): Extension[] => []),
          api.listAllDataSources().catch((): (ExtensionDataSourceInfo | TransformDataSourceInfo)[] => []),
        ]),
        api.listMessageChannels().catch((): any => ({ channels: [] })),
        api.listAgentSummaries().catch((): any => ({ agents: [] })),
      ])

      setDevices(devicesData.devices || [])
      setDeviceTypes(typesResult.device_types || [])
      setRuleDevices(resourcesResult.devices || [])

      const [extData, dsData] = extResult
      setExtensions(extData)
      setExtensionDataSources(dsData.filter((source): source is ExtensionDataSourceInfo => 'extension_id' in source))
      setTransformDataSources(dsData.filter((source): source is TransformDataSourceInfo => 'transform_id' in source))

      setAgents((agentsResult.agents || []).map((a: any) => ({ id: a.id, name: a.name })))
      setMessageChannels((channelsResult.channels || []).map((ch: any) => ({
        name: ch.name,
        type: ch.channel_type,
        enabled: ch.enabled
      })))
      setResourcesLoaded(true)
    } catch (error) {
      handleError(error, { operation: 'Load resources', showToast: false })
    } finally {
      setResourcesLoading(false)
    }
  }, [resourcesLoaded, resourcesLoading, handleError])

  // Load tab data when tab changes (or when a DataChanged event bumps the
  // domain version — e.g. the AI panel created a rule via the CLI)
  const dataVersion = useDataVersion('automations', 'rules')
  useEffect(() => {
    loadTabData()
  }, [loadTabData, dataVersion])

  // Legacy alias for refresh button and import/export handlers
  const loadItems = loadTabData

  // Reset pagination when data changes
  useEffect(() => {
    setRulesPage(1)
  }, [rules.length])

  useEffect(() => {
    setTransformsPage(1)
  }, [transforms.length])

  // Compute paginated data
  // On mobile: show cumulative data (all pages up to current)
  // On desktop: show only current page
  const paginatedRules = useMemo(() => {
    if (isMobile) {
      return rules.slice(0, rulesPage * RULES_ITEMS_PER_PAGE)
    } else {
      return rules.slice(
        (rulesPage - 1) * RULES_ITEMS_PER_PAGE,
        rulesPage * RULES_ITEMS_PER_PAGE
      )
    }
  }, [rules, rulesPage, RULES_ITEMS_PER_PAGE, isMobile])

  const paginatedTransforms = useMemo(() => {
    if (isMobile) {
      return transforms.slice(0, transformsPage * TRANSFORMS_ITEMS_PER_PAGE)
    } else {
      return transforms.slice(
        (transformsPage - 1) * TRANSFORMS_ITEMS_PER_PAGE,
        transformsPage * TRANSFORMS_ITEMS_PER_PAGE
      )
    }
  }, [transforms, transformsPage, TRANSFORMS_ITEMS_PER_PAGE, isMobile])

  // Handlers
  const handleCreate = async () => {
    if (activeTab === 'rules') {
      setEditingRule(undefined)
      setShowRuleDialog(true)
      loadResources()
    } else if (activeTab === 'transforms') {
      setEditingTransform(undefined)
      setShowTransformDialog(true)
      loadResources()
    }
  }

  // Rule detail dialog state
  const [detailRule, setDetailRule] = useState<Rule | null>(null)
  const [detailRuleOpen, setDetailRuleOpen] = useState(false)

  // Rule handlers
  const handleViewRule = (rule: Rule) => {
    setDetailRule(rule)
    setDetailRuleOpen(true)
  }

  const handleEditRule = async (rule: Rule) => {
    try {
      const detail = await api.getRule(rule.id)
      setEditingRule(detail.rule)
      setShowRuleDialog(true)
      loadResources()
    } catch (error) {
      handleError(error, { operation: 'Load rule details', showToast: false })
      showErrorToast(toast, error, tCommon('failed'))
    }
  }

  const handleDeleteRule = async (rule: Rule) => {
    const confirmed = await confirm({
      title: tCommon('delete'),
      description: tAuto('deleteConfirm'),
      confirmText: tCommon('delete'),
      cancelText: tCommon('cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    try {
      await api.deleteRule(rule.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      handleError(error, { operation: 'Delete rule' })
    }
  }

  const handleToggleRule = async (rule: Rule) => {
    try {
      if (rule.enabled) {
        await api.disableRule(rule.id)
      } else {
        await api.enableRule(rule.id)
      }
      await loadItems()
    } catch (error) {
      handleError(error, { operation: 'Toggle rule' })
    }
  }

  const handleExecuteRule = async (rule: Rule) => {
    try {
      const result = await api.testRule(rule.id, true) // execute=true to actually run actions
      if ((result as any).executed) {
        toast({
          title: tCommon('success'),
          description: tAuto('executeSuccess') + ' - ' + ((result as any).execution_result?.actions_executed?.length || 0) + ' actions executed',
        })
      } else {
        toast({
          title: tCommon('success'),
          description: tAuto('executeSuccess'),
        })
      }
    } catch (error) {
      handleError(error, { operation: 'Execute rule' })
    }
  }

  // Transform handlers
  const handleEditTransform = async (transform: TransformAutomation) => {
    setEditingTransform(transform)
    setShowTransformDialog(true)
    loadResources()
  }

  const handleDeleteTransform = async (transform: TransformAutomation) => {
    const confirmed = await confirm({
      title: tCommon('delete'),
      description: tAuto('deleteConfirm'),
      confirmText: tCommon('delete'),
      cancelText: tCommon('cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    try {
      await api.deleteAutomation(transform.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      handleError(error, { operation: 'Delete transform' })
    }
  }

  const handleToggleTransform = async (transform: TransformAutomation) => {
    try {
      await api.setAutomationStatus(transform.id, !transform.enabled)
      await loadItems()
    } catch (error) {
      handleError(error, { operation: 'Toggle transform' })
    }
  }

  const handleExportSingleTransform = async (transform: TransformAutomation) => {
    try {
      const exportData = {
        automations: [{
          id: transform.id,
          name: transform.name,
          description: transform.description,
          type: 'transform',
          enabled: transform.enabled,
          created_at: transform.created_at,
          updated_at: transform.updated_at,
          definition: {
            scope: transform.scope,
            js_code: transform.js_code,
            output_prefix: transform.output_prefix,
            complexity: transform.complexity,
          },
        }],
        count: 1,
        exported_at: new Date().toISOString(),
      }
      const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      // Sanitize filename
      const safeName = transform.name.replace(/[^a-zA-Z0-9_-]/g, '_')
      a.download = `transform-${safeName}-${new Date().toISOString().split('T')[0]}.json`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
      toast({
        title: tCommon('success'),
        description: `Exported "${transform.name}"`,
      })
    } catch (error) {
      handleError(error, { operation: 'Export transform', showToast: true })
    }
  }

  // Save handlers
  const handleSaveRule = async (rule: any) => {
    try {
      if (rule.id) {
        await api.updateRule(rule.id, rule)
      } else {
        await api.createRule(rule)
      }
      setShowRuleDialog(false)
      setEditingRule(undefined)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('ruleSaved'),
      })
    } catch (error) {
      handleError(error, { operation: 'Save rule' })
      throw error
    }
  }

  const handleSaveTransform = async (data: Partial<TransformAutomation>) => {
    try {
      // Build the transform definition with only transform-specific fields
      const definition = {
        scope: data.scope || 'global',
        js_code: data.js_code || '',
        output_prefix: data.output_prefix || '',
        complexity: data.complexity || 2,
      }

      if (editingTransform?.id) {
        // Update existing transform - send name, description, enabled and definition
        await api.updateAutomation(editingTransform.id, {
          name: data.name,
          description: data.description,
          enabled: data.enabled,
          definition,
        })
      } else {
        // Create new transform - include type
        await api.createAutomation({
          name: data.name || '',
          description: data.description,
          type: 'transform',
          enabled: data.enabled ?? true,
          definition,
        })
      }
      setShowTransformDialog(false)
      setEditingTransform(undefined)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('transformSaved'),
      })
    } catch (error) {
      handleError(error, { operation: 'Save transform' })
      throw error
    }
  }

  // Import/Export handlers
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleExportRules = async () => {
    try {
      const data = await api.exportRules('json')
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `neomind-rules-${new Date().toISOString().split('T')[0]}.json`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
      toast({
        title: tCommon('success'),
        description: `Exported ${data.total_count} rules`,
      })
    } catch (error) {
      handleError(error, { operation: 'Export rules', showToast: true })
    }
  }

  const handleImportRules = () => {
    fileInputRef.current?.click()
  }

  const handleExportTransforms = async () => {
    try {
      const data = await api.exportAutomations()
      // Filter only transform-type automations
      const transformData = {
        automations: (data.automations || []).filter((a: any) => a.type === 'transform'),
        count: ((data.automations || []).filter((a: any) => a.type === 'transform')).length,
        exported_at: data.exported_at,
      }
      const blob = new Blob([JSON.stringify(transformData, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `neomind-transforms-${new Date().toISOString().split('T')[0]}.json`
      document.body.appendChild(a)
      a.click()
      document.body.removeChild(a)
      URL.revokeObjectURL(url)
      toast({
        title: tCommon('success'),
        description: `Exported ${transformData.count} transforms`,
      })
    } catch (error) {
      handleError(error, { operation: 'Export transforms', showToast: true })
    }
  }

  const handleImportTransforms = () => {
    fileInputRef.current?.click()
  }

  const handleFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return

    try {
      const content = await file.text()
      const data = JSON.parse(content)

      let result: { imported?: number; skipped?: number; errors?: unknown[]; failed?: number; message?: string }

      if (activeTab === 'rules') {
        const rulesToImport = data.rules || data
        result = await api.importRules(rulesToImport)

        let description = `${tAuto('imported')} ${result.imported}`
        if (result.skipped && result.skipped > 0) {
          description += `, ${tAuto('skipped')} ${result.skipped}`
        }
        if (result.errors && result.errors.length > 0) {
          description += `, ${result.errors.length} ${tAuto('importFailed')}`
        }

        toast({
          title: tCommon('success'),
          description,
        })
      } else {
        // Transforms tab
        const automationsToImport = data.automations || data
        result = await api.importAutomations(automationsToImport)

        let description = `${tAuto('imported')} ${result.imported}`
        if (result.failed && result.failed > 0) {
          description += `, ${result.failed} ${tAuto('importFailed')}`
        }

        toast({
          title: tCommon('success'),
          description: result.message || description,
        })
      }

      await loadItems()
    } catch (error) {
      handleError(error, { operation: `Import ${activeTab}`, showToast: true })
    } finally {
      e.target.value = ''
    }
  }

  const tabs = [
    { value: 'rules' as AutomationTab, label: tAuto('tabs.rules'), icon: <Sparkles className="h-4 w-4" /> },
    { value: 'transforms' as AutomationTab, label: tAuto('tabs.transforms'), icon: <GitBranch className="h-4 w-4" /> },
  ]

  const actions = [
    {
      label: tCommon('create'),
      onClick: handleCreate,
    },
  ]

  // Secondary actions: bulk Import/Export demoted from a top-level dropdown
  // (was actionsExtra) into the page-level "More" overflow. Per-item export
  // already lives in each row's ⋯ menu in RulesList / TransformsList.
  const secondaryActions =
    activeTab === 'rules'
      ? [
          {
            label: tAuto('emptyState.createFromTemplate'),
            icon: <Sparkles className="h-4 w-4" />,
            onClick: () => setShowTemplatePicker(true),
          },
          {
            label: tAuto('export'),
            icon: <Download className="h-4 w-4" />,
            onClick: handleExportRules,
          },
          {
            label: tAuto('import'),
            icon: <Upload className="h-4 w-4" />,
            onClick: handleImportRules,
          },
        ]
      : activeTab === 'transforms'
      ? [
          {
            label: tAuto('export'),
            icon: <Download className="h-4 w-4" />,
            onClick: handleExportTransforms,
          },
          {
            label: tAuto('import'),
            icon: <Upload className="h-4 w-4" />,
            onClick: handleImportTransforms,
          },
        ]
      : []

  return (
    <>
      <PageLayout
        title={tAuto('title')}
        subtitle={tAuto('pageDescription')}
        hideFooterOnMobile
        hasBottomNav
        headerContent={
          <PageTabsBar
            tabs={tabs}
            activeTab={activeTab}
            onTabChange={(v) => handleTabChange(v as AutomationTab)}
            actions={actions}
            secondaryActions={secondaryActions}
          />
        }
        footer={
          activeTab === 'rules' && rules.length > RULES_ITEMS_PER_PAGE ? (
            <Pagination
              total={rules.length}
              pageSize={RULES_ITEMS_PER_PAGE}
              currentPage={rulesPage}
              onPageChange={setRulesPage}
            />
          ) : activeTab === 'transforms' && transforms.length > TRANSFORMS_ITEMS_PER_PAGE ? (
            <Pagination
              total={transforms.length}
              pageSize={TRANSFORMS_ITEMS_PER_PAGE}
              currentPage={transformsPage}
              onPageChange={setTransformsPage}
            />
          ) : undefined
        }
      >
        {/* Rules Tab */}
        <PageTabsContent value="rules" activeTab={activeTab}>
          <RulesList
            rules={rules}
            loading={loading}
            paginatedRules={paginatedRules}
            page={rulesPage}
            onPageChange={setRulesPage}
            onView={handleViewRule}
            onEdit={handleEditRule}
            onDelete={handleDeleteRule}
            onToggleStatus={handleToggleRule}
            onExecute={handleExecuteRule}
          />

          {/* Rule detail dialog (click-to-view) */}
          <RuleDetailDialog
            rule={detailRule}
            open={detailRuleOpen}
            onOpenChange={setDetailRuleOpen}
            onEdit={handleEditRule}
          />
        </PageTabsContent>
        <PageTabsContent value="transforms" activeTab={activeTab}>
          <TransformsList
            transforms={transforms}
            loading={loading}
            paginatedTransforms={paginatedTransforms}
            page={transformsPage}
            onPageChange={setTransformsPage}
            onEdit={handleEditTransform}
            onDelete={handleDeleteTransform}
            onToggleStatus={handleToggleTransform}
            onExport={handleExportSingleTransform}
          />
        </PageTabsContent>
      </PageLayout>

      {/* Mobile Bottom Navigation */}
      <PageTabsBottomNav
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => handleTabChange(v as AutomationTab)}
      />

      {/* Rule Builder Dialog */}
      <SimpleRuleBuilderSplit
        open={showRuleDialog}
        onOpenChange={setShowRuleDialog}
        rule={editingRule}
        onSave={handleSaveRule}
        resources={{ devices: ruleDevices, deviceTypes, extensions, extensionDataSources, transformDataSources, messageChannels, agents }}
      />

      {/* Template picker + device-offline dialog */}
      <RuleTemplatePicker
        open={showTemplatePicker}
        onOpenChange={setShowTemplatePicker}
        onSelectTemplate={(id) => {
          setShowTemplatePicker(false)
          if (id === 'device_offline') {
            setShowOfflineDialog(true)
            loadResources()
          }
        }}
      />
      <DeviceOfflineTemplateDialog
        open={showOfflineDialog}
        onOpenChange={setShowOfflineDialog}
        onCreated={loadItems}
      />

      {/* Transform Builder Dialog */}
      <TransformBuilderSplit
        open={showTransformDialog}
        onOpenChange={setShowTransformDialog}
        transform={editingTransform}
        devices={devices}
        deviceTypes={deviceTypes}
        onSave={handleSaveTransform}
      />

      {/* Hidden file input for import */}
      <input
        ref={fileInputRef}
        type="file"
        accept=".json"
        onChange={handleFileChange}
        className="hidden"
      />
    </>
  )
}

// Export as default for the route
export default AutomationPage

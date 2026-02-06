/**
 * NeoMind Automation Page
 *
 * Automation interface with rules and data transforms.
 * AI Agents are now managed separately in /agents page.
 * Uses PageLayout + PageTabs structure consistent with other pages.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate, useLocation } from "react-router-dom"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent, Pagination } from "@/components/shared"
import { Sparkles, GitBranch } from "lucide-react"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import type { TransformAutomation, Rule } from "@/types"

// Import split-pane builder components
import { SimpleRuleBuilderSplit } from "@/components/automation/SimpleRuleBuilderSplit"
import { TransformBuilder as TransformBuilderSplit } from "@/components/automation/TransformBuilderSplit"

// Import list components
import { RulesList, ITEMS_PER_PAGE as RULES_ITEMS_PER_PAGE } from "./automation-components/RulesList"
import { TransformsList, ITEMS_PER_PAGE as TRANSFORMS_ITEMS_PER_PAGE } from "./automation-components/TransformsList"

type AutomationTab = 'rules' | 'transforms'

export function AutomationPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAuto } = useTranslation('automation')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()

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

  // Resources for dialogs
  const [devices, setDevices] = useState<any[]>([])
  const [deviceTypes, setDeviceTypes] = useState<any[]>([])
  const [ruleDevices, setRuleDevices] = useState<any[]>([])  // Devices with metrics for rules

  // Fetch data
  const loadItems = useCallback(async () => {
    setLoading(true)
    try {
      // Load devices for all tabs (for transforms)
      const devicesData = await api.getDevices()
      setDevices(devicesData.devices || [])

      // Load device types
      try {
        const typesData = await api.getDeviceTypes()
        setDeviceTypes(typesData.device_types || [])
      } catch {
        setDeviceTypes([])
      }

      // Load rule resources (for rules - includes metrics)
      try {
        const resourcesData = await api.getRuleResources()
        setRuleDevices(resourcesData.devices || [])
      } catch (err) {
        handleError(err, { operation: 'Load rule resources', showToast: false })
        setRuleDevices([])
      }

      // Load tab-specific data
      if (activeTab === 'rules') {
        const data = await api.listRules()
        // Sort by created_at descending (newest first)
        setRules((data.rules || []).sort((a, b) => {
          const aTime = typeof a.created_at === 'string' ? new Date(a.created_at).getTime() : a.created_at
          const bTime = typeof b.created_at === 'string' ? new Date(b.created_at).getTime() : b.created_at
          return bTime - aTime
        }))
      } else if (activeTab === 'transforms') {
        const data = await api.listTransforms()
        // Sort by created_at descending (newest first)
        setTransforms((data.transforms || []).sort((a, b) => b.created_at - a.created_at))
      }
    } catch (error) {
      handleError(error, { operation: `Load ${activeTab}`, showToast: false })
    } finally {
      setLoading(false)
    }
  }, [activeTab])

  // Load items when tab changes
  useEffect(() => {
    loadItems()
  }, [loadItems])

  // Reset pagination when data changes
  useEffect(() => {
    setRulesPage(1)
  }, [rules.length])

  useEffect(() => {
    setTransformsPage(1)
  }, [transforms.length])

  // Compute paginated data
  const paginatedRules = rules.slice(
    (rulesPage - 1) * RULES_ITEMS_PER_PAGE,
    rulesPage * RULES_ITEMS_PER_PAGE
  )

  const paginatedTransforms = transforms.slice(
    (transformsPage - 1) * TRANSFORMS_ITEMS_PER_PAGE,
    transformsPage * TRANSFORMS_ITEMS_PER_PAGE
  )

  // Handlers
  const handleCreate = () => {
    if (activeTab === 'rules') {
      setEditingRule(undefined)
      setShowRuleDialog(true)
    } else if (activeTab === 'transforms') {
      setEditingTransform(undefined)
      setShowTransformDialog(true)
    }
  }

  // Rule handlers
  const handleEditRule = async (rule: Rule) => {
    try {
      const detail = await api.getRule(rule.id)
      setEditingRule(detail.rule)
      setShowRuleDialog(true)
    } catch (error) {
      handleError(error, { operation: 'Load rule details', showToast: false })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
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
      handleError(error, { operation: 'Delete rule', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
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
      handleError(error, { operation: 'Toggle rule', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
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
      handleError(error, { operation: 'Execute rule', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  // Transform handlers
  const handleEditTransform = (transform: TransformAutomation) => {
    setEditingTransform(transform)
    setShowTransformDialog(true)
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
      handleError(error, { operation: 'Delete transform', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleToggleTransform = async (transform: TransformAutomation) => {
    try {
      await api.setAutomationStatus(transform.id, !transform.enabled)
      await loadItems()
    } catch (error) {
      handleError(error, { operation: 'Toggle transform', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
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
      handleError(error, { operation: 'Save rule', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
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
      handleError(error, { operation: 'Save transform', showToast: true })
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
      throw error
    }
  }

  return (
    <PageLayout
      title={tAuto('title')}
      subtitle={tAuto('pageDescription')}
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
      {/* Tabs with Actions */}
      <PageTabs
        tabs={[
          { value: 'rules', label: tAuto('tabs.rules'), icon: <Sparkles className="h-4 w-4" /> },
          { value: 'transforms', label: tAuto('tabs.transforms'), icon: <GitBranch className="h-4 w-4" /> },
        ]}
        activeTab={activeTab}
        onTabChange={(v) => handleTabChange(v as AutomationTab)}
        actions={[
          {
            label: tCommon('create'),
            onClick: handleCreate,
          },
          {
            label: tCommon('refresh'),
            variant: 'outline',
            onClick: loadItems,
            disabled: loading,
          },
        ]}
      >
        {/* Rules Tab */}
        <PageTabsContent value="rules" activeTab={activeTab}>
          <RulesList
            rules={rules}
            loading={loading}
            paginatedRules={paginatedRules}
            page={rulesPage}
            onPageChange={setRulesPage}
            onEdit={handleEditRule}
            onDelete={handleDeleteRule}
            onToggleStatus={handleToggleRule}
            onExecute={handleExecuteRule}
          />
        </PageTabsContent>

        {/* Transforms Tab */}
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
          />
        </PageTabsContent>
      </PageTabs>

      {/* Rule Builder Dialog */}
      <SimpleRuleBuilderSplit
        open={showRuleDialog}
        onOpenChange={setShowRuleDialog}
        rule={editingRule}
        onSave={handleSaveRule}
        resources={{ devices: ruleDevices, deviceTypes }}
      />

      {/* Transform Builder Dialog */}
      <TransformBuilderSplit
        open={showTransformDialog}
        onOpenChange={setShowTransformDialog}
        transform={editingTransform}
        devices={devices}
        onSave={handleSaveTransform}
      />
    </PageLayout>
  )
}

// Export as default for the route
export default AutomationPage

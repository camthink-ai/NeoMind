/**
 * NeoTalk Automation Page
 *
 * Unified automation interface with rules, workflows, and data transforms.
 * Uses PageLayout + PageTabs structure consistent with other pages.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import type { TransformAutomation, Rule, Workflow as WorkflowType } from "@/types"

// Import split-pane builder components
import { SimpleRuleBuilderSplit } from "@/components/automation/SimpleRuleBuilderSplit"
import { TransformBuilder as TransformBuilderSplit } from "@/components/automation/TransformBuilderSplit"
import { WorkflowBuilderSplit } from "@/components/automation/WorkflowBuilderSplit"

// Import list components
import { RulesList } from "./automation-components/RulesList"
import { WorkflowsList } from "./automation-components/WorkflowsList"
import { TransformsList } from "./automation-components/TransformsList"

type AutomationTab = 'rules' | 'workflows' | 'transforms'

export function AutomationPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAuto } = useTranslation('automation')
  const { toast } = useToast()
  const [activeTab, setActiveTab] = useState<AutomationTab>('rules')

  // Builder states
  const [showRuleDialog, setShowRuleDialog] = useState(false)
  const [showTransformDialog, setShowTransformDialog] = useState(false)
  const [showWorkflowDialog, setShowWorkflowDialog] = useState(false)

  // Editing states
  const [editingRule, setEditingRule] = useState<Rule | undefined>(undefined)
  const [editingTransform, setEditingTransform] = useState<TransformAutomation | undefined>(undefined)
  const [editingWorkflow, setEditingWorkflow] = useState<WorkflowType | undefined>(undefined)

  // Data state
  const [rules, setRules] = useState<Rule[]>([])
  const [workflows, setWorkflows] = useState<WorkflowType[]>([])
  const [transforms, setTransforms] = useState<TransformAutomation[]>([])
  const [loading, setLoading] = useState(false)

  // Resources for dialogs
  const [devices, setDevices] = useState<Array<{ id: string; name: string; device_type?: string }>>([])
  const [deviceTypes, setDeviceTypes] = useState<any[]>([])

  // Fetch data
  const loadItems = useCallback(async () => {
    setLoading(true)
    try {
      // Load devices for all tabs
      const devicesData = await api.getDevices()
      setDevices(devicesData.devices?.map((d: any) => ({
        id: d.id,
        name: d.name,
        device_type: d.device_type
      })) || [])

      // Load device types
      try {
        const typesData = await api.getDeviceTypes()
        setDeviceTypes(typesData.device_types || [])
      } catch {
        setDeviceTypes([])
      }

      // Load tab-specific data
      if (activeTab === 'rules') {
        const data = await api.listRules()
        setRules(data.rules || [])
      } else if (activeTab === 'workflows') {
        const data = await api.listWorkflows()
        setWorkflows(data.workflows || [])
      } else if (activeTab === 'transforms') {
        const data = await api.listTransforms()
        setTransforms(data.transforms || [])
      }
    } catch (error) {
      console.error(`Failed to load ${activeTab}:`, error)
    } finally {
      setLoading(false)
    }
  }, [activeTab])

  // Load items when tab changes
  useEffect(() => {
    loadItems()
  }, [loadItems])

  // Handlers
  const handleCreate = () => {
    if (activeTab === 'rules') {
      setEditingRule(undefined)
      setShowRuleDialog(true)
    } else if (activeTab === 'workflows') {
      setEditingWorkflow(undefined)
      setShowWorkflowDialog(true)
    } else if (activeTab === 'transforms') {
      setEditingTransform(undefined)
      setShowTransformDialog(true)
    }
  }

  // Rule handlers
  const handleEditRule = (rule: Rule) => {
    setEditingRule(rule)
    setShowRuleDialog(true)
  }

  const handleDeleteRule = async (rule: Rule) => {
    if (!confirm(tAuto('deleteConfirm'))) return
    try {
      await api.deleteRule(rule.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      console.error('Failed to delete rule:', error)
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
      console.error('Failed to toggle rule:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleExecuteRule = async (rule: Rule) => {
    try {
      await api.testRule(rule.id)
      toast({
        title: tCommon('success'),
        description: tAuto('executeSuccess'),
      })
    } catch (error) {
      console.error('Failed to execute rule:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  // Workflow handlers
  const handleEditWorkflow = (workflow: WorkflowType) => {
    setEditingWorkflow(workflow)
    setShowWorkflowDialog(true)
  }

  const handleDeleteWorkflow = async (workflow: WorkflowType) => {
    if (!confirm(tAuto('deleteConfirm'))) return
    try {
      await api.deleteWorkflow(workflow.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      console.error('Failed to delete workflow:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleToggleWorkflow = async (workflow: WorkflowType) => {
    try {
      await api.updateWorkflow(workflow.id, { enabled: !workflow.enabled })
      await loadItems()
    } catch (error) {
      console.error('Failed to toggle workflow:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleExecuteWorkflow = async (workflow: WorkflowType) => {
    try {
      await api.executeWorkflow(workflow.id)
      toast({
        title: tCommon('success'),
        description: tAuto('executeSuccess'),
      })
    } catch (error) {
      console.error('Failed to execute workflow:', error)
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
    if (!confirm(tAuto('deleteConfirm'))) return
    try {
      await api.deleteAutomation(transform.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      console.error('Failed to delete transform:', error)
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
      console.error('Failed to toggle transform:', error)
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
      console.error('Failed to save rule:', error)
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
      if (editingTransform?.id) {
        await api.updateAutomation(editingTransform.id, {
          name: data.name,
          description: data.description,
          enabled: data.enabled,
          definition: {
            scope: data.scope,
            operations: data.operations,
            js_code: data.js_code,
            output_prefix: data.output_prefix,
          },
        })
      } else {
        await api.createAutomation({
          name: data.name || '',
          description: data.description,
          type: 'transform',
          enabled: data.enabled ?? true,
          definition: {
            scope: data.scope || { type: 'global' },
            operations: data.operations || [],
            js_code: data.js_code,
            output_prefix: data.output_prefix,
          },
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
      console.error('Failed to save transform:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
      throw error
    }
  }

  const handleSaveWorkflow = async (workflow: any) => {
    try {
      if (workflow.id) {
        await api.updateWorkflow(workflow.id, workflow)
      } else {
        await api.createWorkflow(workflow)
      }
      setShowWorkflowDialog(false)
      setEditingWorkflow(undefined)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('workflowSaved'),
      })
    } catch (error) {
      console.error('Failed to save workflow:', error)
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
      subtitle={tAuto('description')}
    >
      {/* Tabs with Actions */}
      <PageTabs
        tabs={[
          { value: 'rules', label: tAuto('tabs.rules') },
          { value: 'workflows', label: tAuto('tabs.workflows') },
          { value: 'transforms', label: tAuto('tabs.transforms') },
        ]}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as AutomationTab)}
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
            onEdit={handleEditRule}
            onDelete={handleDeleteRule}
            onToggleStatus={handleToggleRule}
            onExecute={handleExecuteRule}
          />
        </PageTabsContent>

        {/* Workflows Tab */}
        <PageTabsContent value="workflows" activeTab={activeTab}>
          <WorkflowsList
            workflows={workflows}
            loading={loading}
            onEdit={handleEditWorkflow}
            onDelete={handleDeleteWorkflow}
            onToggleStatus={handleToggleWorkflow}
            onExecute={handleExecuteWorkflow}
          />
        </PageTabsContent>

        {/* Transforms Tab */}
        <PageTabsContent value="transforms" activeTab={activeTab}>
          <TransformsList
            transforms={transforms}
            loading={loading}
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
        resources={{ devices, deviceTypes }}
      />

      {/* Transform Builder Dialog */}
      <TransformBuilderSplit
        open={showTransformDialog}
        onOpenChange={setShowTransformDialog}
        transform={editingTransform}
        devices={devices}
        onSave={handleSaveTransform}
      />

      {/* Workflow Builder */}
      <WorkflowBuilderSplit
        open={showWorkflowDialog}
        onClose={() => setShowWorkflowDialog(false)}
        workflow={editingWorkflow}
        onSave={handleSaveWorkflow}
        resources={{ devices, metrics: ['temperature', 'humidity'], alertChannels: [] }}
      />
    </PageLayout>
  )
}

// Export as default for the route
export default AutomationPage

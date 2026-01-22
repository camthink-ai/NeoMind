/**
 * NeoTalk Automation Page
 *
 * Unified automation interface with AI agents, rules and data transforms.
 * Uses PageLayout + PageTabs structure consistent with other pages.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import type { TransformAutomation, Rule, AiAgent, AiAgentDetail } from "@/types"

// Import split-pane builder components
import { SimpleRuleBuilderSplit } from "@/components/automation/SimpleRuleBuilderSplit"
import { TransformBuilder as TransformBuilderSplit } from "@/components/automation/TransformBuilderSplit"

// Import list components
import { RulesList } from "./automation-components/RulesList"
import { TransformsList } from "./automation-components/TransformsList"

// Import agents components
import { AgentsList } from "./agents-components/AgentsList"
import { AgentCreatorDialog } from "./agents-components/AgentCreatorDialog"
import { AgentMemoryDialog } from "./agents-components/AgentMemoryDialog"

type AutomationTab = 'agents' | 'rules' | 'transforms'

export function AutomationPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAuto } = useTranslation('automation')
  const { toast } = useToast()
  const [activeTab, setActiveTab] = useState<AutomationTab>('agents')

  // Builder states
  const [showRuleDialog, setShowRuleDialog] = useState(false)
  const [showTransformDialog, setShowTransformDialog] = useState(false)

  // Agents dialog states
  const [showAgentDialog, setShowAgentDialog] = useState(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState(false)

  // Editing states
  const [editingRule, setEditingRule] = useState<Rule | undefined>(undefined)
  const [editingTransform, setEditingTransform] = useState<TransformAutomation | undefined>(undefined)
  const [editingAgent, setEditingAgent] = useState<AiAgentDetail | undefined>(undefined)

  // Agents dialog data states
  const [memoryAgentId, setMemoryAgentId] = useState('')
  const [memoryAgentName, setMemoryAgentName] = useState('')

  // Data state
  const [rules, setRules] = useState<Rule[]>([])
  const [transforms, setTransforms] = useState<TransformAutomation[]>([])
  const [agents, setAgents] = useState<AiAgent[]>([])
  const [loading, setLoading] = useState(false)

  // Resources for dialogs
  const [devices, setDevices] = useState<any[]>([])
  const [deviceTypes, setDeviceTypes] = useState<any[]>([])

  // Fetch data
  const loadItems = useCallback(async () => {
    setLoading(true)
    try {
      // Load devices for all tabs
      const devicesData = await api.getDevices()
      setDevices(devicesData.devices || [])

      // Load device types
      try {
        const typesData = await api.getDeviceTypes()
        setDeviceTypes(typesData.device_types || [])
      } catch {
        setDeviceTypes([])
      }

      // Load tab-specific data
      if (activeTab === 'agents') {
        const data = await api.listAgents()
        setAgents(data.agents || [])
      } else if (activeTab === 'rules') {
        const data = await api.listRules()
        setRules(data.rules || [])
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
    if (activeTab === 'agents') {
      setEditingAgent(undefined)
      setShowAgentDialog(true)
    } else if (activeTab === 'rules') {
      setEditingRule(undefined)
      setShowRuleDialog(true)
    } else if (activeTab === 'transforms') {
      setEditingTransform(undefined)
      setShowTransformDialog(true)
    }
  }

  // Agent handlers
  const handleEditAgent = async (agent: AiAgent) => {
    try {
      const detail = await api.getAgent(agent.id)
      setEditingAgent(detail)
      setShowAgentDialog(true)
    } catch (error) {
      console.error('Failed to load agent details:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleDeleteAgent = async (agent: AiAgent) => {
    if (!confirm(tAuto('deleteConfirm'))) return
    try {
      await api.deleteAgent(agent.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAuto('itemDeleted'),
      })
    } catch (error) {
      console.error('Failed to delete agent:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleToggleAgent = async (agent: AiAgent) => {
    try {
      const newStatus = agent.status === 'Active' ? 'paused' : 'active'
      await api.setAgentStatus(agent.id, newStatus)
      await loadItems()
    } catch (error) {
      console.error('Failed to toggle agent status:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleExecuteAgent = async (agent: AiAgent) => {
    try {
      await api.executeAgent(agent.id)
      toast({
        title: tCommon('success'),
        description: tAuto('executeSuccess'),
      })
      setTimeout(() => loadItems(), 500)
    } catch (error) {
      console.error('Failed to execute agent:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleSaveAgent = async (data: any) => {
    try {
      if (editingAgent) {
        await api.updateAgent(editingAgent.id, data)
      } else {
        await api.createAgent(data)
      }
      setShowAgentDialog(false)
      setEditingAgent(undefined)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: editingAgent ? tAuto('itemUpdated') : tAuto('itemCreated'),
      })
    } catch (error) {
      console.error('Failed to save agent:', error)
      throw error
    }
  }

  const handleViewMemory = (agentId: string, agentName: string) => {
    setMemoryAgentId(agentId)
    setMemoryAgentName(agentName)
    setMemoryDialogOpen(true)
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
      // Build the transform definition matching backend TransformAutomation structure
      // Backend requires: id, name, description, enabled, scope, js_code, output_prefix, complexity, execution_count, created_at, updated_at, last_executed
      const now = Math.floor(Date.now() / 1000)
      const buildDefinition = () => {
        const baseFields = {
          id: editingTransform?.id || crypto.randomUUID(),
          name: data.name || '',
          description: data.description || '',
          enabled: data.enabled ?? true,
          scope: data.scope || 'global',
          js_code: data.js_code || '',
          output_prefix: data.output_prefix || '',
          complexity: data.complexity || 2,
          execution_count: 0,
          created_at: now,
          updated_at: now,
          last_executed: null as number | null,
        }
        return baseFields
      }

      if (editingTransform?.id) {
        await api.updateAutomation(editingTransform.id, {
          name: data.name,
          description: data.description,
          enabled: data.enabled,
          definition: buildDefinition(),
        })
      } else {
        await api.createAutomation({
          name: data.name || '',
          description: data.description,
          type: 'transform',
          enabled: data.enabled ?? true,
          definition: buildDefinition(),
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

  return (
    <PageLayout
      title={tAuto('title')}
      subtitle={tAuto('pageDescription')}
    >
      {/* Tabs with Actions */}
      <PageTabs
        tabs={[
          { value: 'agents', label: tAuto('tabs.agents') },
          { value: 'rules', label: tAuto('tabs.rules') },
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
        {/* Agents Tab */}
        <PageTabsContent value="agents" activeTab={activeTab}>
          <AgentsList
            agents={agents}
            loading={loading}
            onEdit={handleEditAgent}
            onDelete={handleDeleteAgent}
            onToggleStatus={handleToggleAgent}
            onExecute={handleExecuteAgent}
            onViewMemory={handleViewMemory}
          />
        </PageTabsContent>

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

      {/* Agent Creator/Editor Dialog */}
      <AgentCreatorDialog
        open={showAgentDialog}
        onOpenChange={setShowAgentDialog}
        agent={editingAgent}
        devices={devices}
        deviceTypes={deviceTypes}
        onSave={handleSaveAgent}
      />

      {/* Agent Memory Dialog */}
      <AgentMemoryDialog
        open={memoryDialogOpen}
        onOpenChange={setMemoryDialogOpen}
        agentId={memoryAgentId}
        agentName={memoryAgentName}
      />
    </PageLayout>
  )
}

// Export as default for the route
export default AutomationPage

/**
 * NeoTalk AI Agents Page
 *
 * User-defined AI Agents for autonomous IoT automation.
 * Uses PageLayout + PageTabs structure consistent with other pages.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import { PageTabs, PageTabsContent } from "@/components/shared"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import type { AiAgent, AiAgentDetail, AgentExecution } from "@/types"

// Import components
import { AgentsList } from "./agents-components/AgentsList"
import { AgentExecutionsList } from "./agents-components/AgentExecutionsList"
import { AgentCreatorDialog } from "./agents-components/AgentCreatorDialog"
import { AgentMemoryDialog } from "./agents-components/AgentMemoryDialog"
import { ExecutionDetailDialog } from "./agents-components/ExecutionDetailDialog"

type AgentTab = 'agents' | 'executions'

export function AgentsPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { toast } = useToast()
  const [activeTab, setActiveTab] = useState<AgentTab>('agents')

  // Dialog states
  const [showAgentDialog, setShowAgentDialog] = useState(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState(false)
  const [executionDetailOpen, setExecutionDetailOpen] = useState(false)

  // Dialog data states
  const [memoryAgentId, setMemoryAgentId] = useState('')
  const [memoryAgentName, setMemoryAgentName] = useState('')
  const [detailAgentId, setDetailAgentId] = useState('')
  const [detailExecutionId, setDetailExecutionId] = useState('')

  // Editing states
  const [editingAgent, setEditingAgent] = useState<AiAgentDetail | undefined>(undefined)

  // Data state
  const [agents, setAgents] = useState<AiAgent[]>([])
  const [selectedAgent] = useState<AiAgentDetail | undefined>(undefined)
  const [executions, setExecutions] = useState<AgentExecution[]>([])
  const [loading, setLoading] = useState(false)

  // Resources for dialogs
  const [devices, setDevices] = useState<any[]>([])
  const [deviceTypes, setDeviceTypes] = useState<any[]>([])

  // Fetch data
  const loadItems = useCallback(async () => {
    setLoading(true)
    try {
      // Load devices for dialogs
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
        // Sort by created_at descending (newest first)
        setAgents((data.agents || []).sort((a, b) =>
          new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
        ))
      } else if (activeTab === 'executions' && selectedAgent) {
        const data = await api.getAgentExecutions(selectedAgent.id)
        setExecutions(data.executions || [])
      }
    } catch (error) {
      console.error(`Failed to load ${activeTab}:`, error)
    } finally {
      setLoading(false)
    }
  }, [activeTab, selectedAgent])

  // Load items when tab changes
  useEffect(() => {
    loadItems()
  }, [loadItems])

  // Handlers
  const handleCreate = () => {
    setEditingAgent(undefined)
    setShowAgentDialog(true)
  }

  const handleEdit = async (agent: AiAgent) => {
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

  const handleDelete = async (agent: AiAgent) => {
    if (!confirm(tAgent('deleteConfirm'))) return
    try {
      await api.deleteAgent(agent.id)
      await loadItems()
      toast({
        title: tCommon('success'),
        description: tAgent('agentDeleted'),
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

  const handleToggleStatus = async (agent: AiAgent) => {
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

  const handleExecute = async (agent: AiAgent) => {
    try {
      const result = await api.executeAgent(agent.id)
      toast({
        title: tCommon('success'),
        description: tAgent('executionStarted', { id: result.execution_id }),
      })
      // Reload after a short delay to show updated status
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

  const handleSave = async (data: any) => {
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
        description: editingAgent ? tAgent('agentUpdated') : tAgent('agentCreated'),
      })
    } catch (error) {
      console.error('Failed to save agent:', error)
      throw error
    }
  }

  // Open memory dialog for an agent
  const handleViewMemory = (agentId: string, agentName: string) => {
    setMemoryAgentId(agentId)
    setMemoryAgentName(agentName)
    setMemoryDialogOpen(true)
  }

  // Open execution detail dialog
  const handleViewExecutionDetail = (agentId: string, executionId: string) => {
    setDetailAgentId(agentId)
    setDetailExecutionId(executionId)
    setExecutionDetailOpen(true)
  }

  return (
    <PageLayout
      title={tAgent('title')}
      subtitle={tAgent('description')}
    >
      {/* Tabs with Actions */}
      <PageTabs
        tabs={[
          { value: 'agents', label: tAgent('tabs.agents') },
          ...(selectedAgent ? [{ value: 'executions', label: tAgent('tabs.executions') }] : []),
        ]}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as AgentTab)}
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
            onEdit={handleEdit}
            onDelete={handleDelete}
            onToggleStatus={handleToggleStatus}
            onExecute={handleExecute}
            onViewMemory={handleViewMemory}
          />
        </PageTabsContent>

        {/* Executions Tab */}
        <PageTabsContent value="executions" activeTab={activeTab}>
          {selectedAgent && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <div>
                  <h3 className="text-lg font-semibold">{selectedAgent.name}</h3>
                  <p className="text-sm text-muted-foreground">{selectedAgent.user_prompt}</p>
                </div>
              </div>
              <AgentExecutionsList
                executions={executions}
                loading={loading}
                agentId={selectedAgent.id}
                onViewDetail={handleViewExecutionDetail}
              />
            </div>
          )}
        </PageTabsContent>
      </PageTabs>

      {/* Agent Creator/Editor Dialog */}
      <AgentCreatorDialog
        open={showAgentDialog}
        onOpenChange={setShowAgentDialog}
        agent={editingAgent}
        devices={devices}
        deviceTypes={deviceTypes}
        onSave={handleSave}
      />

      {/* Agent Memory Dialog */}
      <AgentMemoryDialog
        open={memoryDialogOpen}
        onOpenChange={setMemoryDialogOpen}
        agentId={memoryAgentId}
        agentName={memoryAgentName}
      />

      {/* Execution Detail Dialog */}
      <ExecutionDetailDialog
        open={executionDetailOpen}
        onOpenChange={setExecutionDetailOpen}
        agentId={detailAgentId}
        executionId={detailExecutionId}
      />
    </PageLayout>
  )
}

// Export as default for the route
export default AgentsPage

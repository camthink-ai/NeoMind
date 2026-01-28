/**
 * NeoTalk AI Agents Page
 *
 * User-defined AI Agents for autonomous IoT automation.
 * Card grid layout with detail dialog for viewing individual agent details.
 */

import { useState, useCallback, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { Loader2 } from "lucide-react"
import type { AiAgent, AiAgentDetail } from "@/types"

// Import components
import { AgentCard, CreateCard } from "./agents-components/AgentCard"
import { AgentCreatorDialog } from "./agents-components/AgentCreatorDialog"
import { ExecutionDetailDialog } from "./agents-components/ExecutionDetailDialog"

// Import dialogs
import { AgentMemoryDialog } from "./agents-components/AgentMemoryDialog"

// Import detail panel content for reuse in dialog
import { AgentDetailPanel } from "./agents-components/AgentDetailPanel"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export function AgentsPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { toast } = useToast()

  // Dialog states
  const [showAgentDialog, setShowAgentDialog] = useState(false)
  const [memoryDialogOpen, setMemoryDialogOpen] = useState(false)
  const [executionDetailOpen, setExecutionDetailOpen] = useState(false)
  const [detailDialogOpen, setDetailDialogOpen] = useState(false)

  // Dialog data states
  const [memoryAgentId, setMemoryAgentId] = useState('')
  const [memoryAgentName, setMemoryAgentName] = useState('')
  const [detailAgentId, setDetailAgentId] = useState('')
  const [detailExecutionId, setDetailExecutionId] = useState('')

  // Editing states
  const [editingAgent, setEditingAgent] = useState<AiAgentDetail | undefined>(undefined)
  const [selectedAgent, setSelectedAgent] = useState<AiAgentDetail | null>(null)

  // Data state
  const [agents, setAgents] = useState<AiAgent[]>([])
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

      // Load agents
      const data = await api.listAgents()
      // Sort by created_at descending (newest first)
      setAgents((data.agents || []).sort((a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      ))
    } catch (error) {
      console.error('Failed to load agents:', error)
    } finally {
      setLoading(false)
    }
  }, [])

  // Load items on mount
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
    const confirmed = await confirm({
      title: tCommon('delete'),
      description: tAgent('deleteConfirm'),
      confirmText: tCommon('delete'),
      cancelText: tCommon('cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    try {
      await api.deleteAgent(agent.id)
      await loadItems()
      if (selectedAgent?.id === agent.id) {
        setSelectedAgent(null)
      }
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

  // Open detail dialog for an agent
  const handleViewDetail = async (agent: AiAgent) => {
    try {
      const detail = await api.getAgent(agent.id)
      setSelectedAgent(detail)
      setDetailDialogOpen(true)
    } catch (error) {
      console.error('Failed to load agent details:', error)
      toast({
        title: tCommon('failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  // Refresh detail when dialog is open
  useEffect(() => {
    if (detailDialogOpen && selectedAgent) {
      api.getAgent(selectedAgent.id).then(setSelectedAgent).catch(console.error)
    }
  }, [agents, detailDialogOpen, selectedAgent?.id])

  return (
    <PageLayout
      title={tAgent('title')}
      subtitle={tAgent('description')}
    >
      {loading ? (
        <div className="flex items-center justify-center py-20">
          <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
        </div>
      ) : agents.length === 0 ? (
        <div className="flex items-center justify-center py-20">
          <CreateCard onClick={handleCreate} />
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
          <CreateCard onClick={handleCreate} />
          {agents.map((agent) => (
            <AgentCard
              key={agent.id}
              agent={agent}
              onToggleStatus={handleToggleStatus}
              onExecute={handleExecute}
              onEdit={handleEdit}
              onDelete={handleDelete}
              onViewMemory={handleViewMemory}
              onClick={() => handleViewDetail(agent)}
            />
          ))}
        </div>
      )}

      {/* Agent Creator/Editor Dialog */}
      <AgentCreatorDialog
        open={showAgentDialog}
        onOpenChange={setShowAgentDialog}
        agent={editingAgent}
        devices={devices}
        deviceTypes={deviceTypes}
        onSave={handleSave}
      />

      {/* Agent Detail Dialog */}
      <Dialog open={detailDialogOpen} onOpenChange={setDetailDialogOpen}>
        <DialogContent className="max-w-4xl max-h-[90vh] overflow-hidden p-0">
          <DialogHeader className="px-6 pt-6 pb-4 border-b">
            <DialogTitle className="text-lg">智能体详情</DialogTitle>
          </DialogHeader>
          <div className="overflow-y-auto max-h-[calc(90vh-100px)]">
            {selectedAgent && (
              <AgentDetailPanel
                agent={selectedAgent}
                onEdit={handleEdit}
                onExecute={handleExecute}
                onViewExecutionDetail={handleViewExecutionDetail}
                onRefresh={loadItems}
                inlineMode
              />
            )}
          </div>
        </DialogContent>
      </Dialog>

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

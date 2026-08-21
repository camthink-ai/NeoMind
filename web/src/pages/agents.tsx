/**
 * NeoMind AI Agents Page
 *
 * User-defined AI Agents for autonomous IoT automation.
 * Card grid layout with detail dialog for viewing individual agent details.
 * Uses WebSocket events for real-time agent status updates.
 * Also includes System Memory tab for viewing aggregated memory.
 */

import { useState, useCallback, useEffect, useRef, useMemo } from "react"
import { useStore } from "@/store"
import { useDataVersion } from "@/hooks/useDataVersion"
import { useLocation, useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import { BuiltinModelWizard } from "@/components/llm/BuiltinModelWizard"
import { PageTabsBar, PageTabsContent, PageTabsBottomNav, Pagination } from "@/components/shared"
import { LoadingState } from "@/components/shared/LoadingState"
import { api } from "@/lib/api"
import { useToast } from "@/hooks/use-toast"
import { confirm } from "@/hooks/use-confirm"
import { useEvents } from "@/hooks/useEvents"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { showErrorToast } from "@/lib/error-messages"
import { useIsMobile } from "@/hooks/useMobile"
import { Bot, Plus, Brain, Cpu, Settings, BookOpen, Edit, Play, FileText, Wrench, Search, Download, Server } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { debounce } from "@/lib/utils/async"
import type { ToolSourceFilter } from "./agents-components/ToolsPanel"
import { EmptyState } from "@/components/shared/EmptyState"
import type { AiAgent, AiAgentDetail, Device, DeviceType, Extension, UnifiedDataSourceInfo } from "@/types"
import type { AgentExecutionStartedEvent, AgentExecutionCompletedEvent, AgentThinkingEvent } from "@/lib/events"

// Import components
import { AgentCard } from "./agents-components/AgentCard"
import { AgentEditorFullScreen } from "./agents-components/AgentEditorFullScreen"
import { ExecutionDetailDialog } from "./agents-components/ExecutionDetailDialog"
import { AgentDetailPanel } from "./agents-components/AgentDetailPanel"
import { MemoryPanel, MemoryPanelRef } from "./agents-components/MemoryPanel"
import { SkillsPanel, type SkillsPanelHandle } from "./agents-components/SkillsPanel"
import { ToolsPanel } from "./agents-components/ToolsPanel"
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from "@/components/automation/dialog/FullScreenDialog"

export function AgentsPage() {
  const { t: tCommon } = useTranslation('common')
  const { t: tAgent } = useTranslation('agents')
  const { toast } = useToast()
  const { handleError } = useErrorHandler()
  const location = useLocation()
  const navigate = useNavigate()

  // LLM backend presence — drives the no-backend setup banner (agents are
  // inert without a model).
  const llmBackends = useStore((s) => s.llmBackends)
  const llmBackendLoading = useStore((s) => s.llmBackendLoading)
  const loadBackends = useStore((s) => s.loadBackends)
  const openSettings = useStore((s) => s.openSettings)
  useEffect(() => { loadBackends() }, [loadBackends])
  const [agentWizardOpen, setAgentWizardOpen] = useState(false)

  // Determine active tab from URL path
  const getTabFromPath = () => {
    if (location.pathname.includes('/agents/memory')) {
      return 'memory'
    }
    if (location.pathname.includes('/agents/skills')) {
      return 'skills'
    }
    if (location.pathname.includes('/agents/tools')) {
      return 'tools'
    }
    return 'agents'
  }
  const activeTab = getTabFromPath()

  // Handle tab change by navigating to the appropriate URL
  const handleTabChange = (tab: string) => {
    if (tab === 'memory') {
      navigate('/agents/memory')
    } else if (tab === 'skills') {
      navigate('/agents/skills')
    } else if (tab === 'tools') {
      navigate('/agents/tools')
    } else {
      navigate('/agents')
    }
  }

  // Dialog states
  const [showAgentDialog, setShowAgentDialog] = useState(false)
  const [executionDetailOpen, setExecutionDetailOpen] = useState(false)
  const [detailDialogOpen, setDetailDialogOpen] = useState(false)

  // Dialog data states
  const [detailAgentId, setDetailAgentId] = useState('')
  const [detailExecutionId, setDetailExecutionId] = useState('')

  // Editing states
  const [editingAgent, setEditingAgent] = useState<AiAgentDetail | undefined>(undefined)
  const [selectedAgent, setSelectedAgent] = useState<AiAgentDetail | null>(null)

  // Data state
  const [agents, setAgents] = useState<AiAgent[]>([])
  const [loading, setLoading] = useState(false)
  const [memoryRefreshKey, setMemoryRefreshKey] = useState(0)
  const memoryPanelRef = useRef<MemoryPanelRef>(null)
  const skillsPanelRef = useRef<SkillsPanelHandle>(null)

  // Skills pagination state (lifted from SkillsPanel for PageLayout footer)
  const [skillsPagination, setSkillsPagination] = useState<{
    total: number
    pageSize: number
    currentPage: number
    onPageChange: (page: number) => void
    loading: boolean
  }>({ total: 0, pageSize: 10, currentPage: 1, onPageChange: () => {}, loading: false })
  const handleSkillsPaginationChange = useCallback((info: typeof skillsPagination) => {
    setSkillsPagination(info)
  }, [])

  // Tools tab: lifted pagination + search state. Search input lives in the
  // PageTabsBar actionsExtra (top-right) like Data Explorer; the actual filter
  // runs client-side inside ToolsPanel.
  const [toolsPagination, setToolsPagination] = useState<{
    total: number
    pageSize: number
    currentPage: number
    onPageChange: (page: number) => void
    loading: boolean
  }>({ total: 0, pageSize: 10, currentPage: 1, onPageChange: () => {}, loading: false })
  const handleToolsPaginationChange = useCallback((info: typeof toolsPagination) => {
    setToolsPagination(info)
  }, [])
  const [toolsSearch, setToolsSearch] = useState("")
  const [toolsDebouncedSearch, setToolsDebouncedSearch] = useState("")
  const [toolsSourceFilter, setToolsSourceFilter] = useState<ToolSourceFilter>('all')
  const updateToolsSearch = useMemo(() => debounce(setToolsDebouncedSearch, 300), [])
  useEffect(() => {
    updateToolsSearch(toolsSearch)
  }, [toolsSearch, updateToolsSearch])
  // Clear search + filter when leaving the tools tab so they don't leak across tabs.
  useEffect(() => {
    if (activeTab !== 'tools') {
      setToolsSearch("")
      setToolsDebouncedSearch("")
      setToolsSourceFilter('all')
    }
  }, [activeTab])

  // Track executing agents for real-time updates with timestamps for timeout
  const [executingAgents, setExecutingAgents] = useState<Map<string, number>>(new Map())
  // Track current thinking state per agent
  const [agentThinking, setAgentThinking] = useState<Record<string, string>>({})
  // Throttle thinking updates to reduce re-renders (200ms interval)
  const lastThinkingUpdateRef = useRef(0)
  const pendingThinkingRef = useRef<Record<string, string>>({})

  // Resources for dialogs
  const [devices, setDevices] = useState<Device[]>([])
  const [deviceTypes, setDeviceTypes] = useState<DeviceType[]>([])
  const [extensions, setExtensions] = useState<Extension[]>([])
  const [unifiedDataSources, setUnifiedDataSources] = useState<UnifiedDataSourceInfo[]>([])

  // Fetch agents list only (fast)
  const loadItems = useCallback(async () => {
    setLoading(true)
    try {
      const agentsData = await api.listAgents()

      // Sort by created_at descending (newest first)
      setAgents((agentsData.agents || []).sort((a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      ))
    } catch (error) {
      handleError(error, { operation: 'Load agents', showToast: false })
    } finally {
      setLoading(false)
    }
  }, [])

  // Lazy-load editor resources (devices, extensions, data sources) when opening editor
  const [editorResourcesLoaded, setEditorResourcesLoaded] = useState(false)
  const editorLoadingRef = useRef(false)
  const loadEditorResources = useCallback(async () => {
    if (editorResourcesLoaded || editorLoadingRef.current) return
    editorLoadingRef.current = true
    try {
      const [devicesData, typesResult, extData] = await Promise.all([
        api.getDevices().catch((): { devices: Device[] } => ({ devices: [] })),
        api.getDeviceTypes().catch((): { device_types: DeviceType[] } => ({ device_types: [] })),
        api.listExtensions().catch((): Extension[] => []),
      ])

      setDevices(devicesData.devices || [])
      setDeviceTypes(typesResult.device_types || [])
      setExtensions(extData)
      setEditorResourcesLoaded(true)
    } catch (error) {
      editorLoadingRef.current = false
      handleError(error, { operation: 'Load editor resources', showToast: false })
    }
  }, [editorResourcesLoaded, handleError])

  // Load items on mount
  const dataVersion = useDataVersion('agents', 'skills')
  useEffect(() => {
    loadItems()
  }, [loadItems, dataVersion])

  // Listen to WebSocket events for real-time agent status updates
  useEvents({
    enabled: true,
    eventTypes: ['AgentExecutionStarted', 'AgentExecutionCompleted', 'AgentThinking'],
    onConnected: (connected) => {
      if (!connected) {
        // Connection lost - clear all executing states and reset agent statuses
        setExecutingAgents(new Map())
        pendingThinkingRef.current = {}
        setAgentThinking({})
        // Reset executing status in agents list - will be refreshed when reconnecting
        setAgents(prev => prev.map(agent =>
          agent.status === 'Executing'
            ? { ...agent, status: 'Active' as const, currentThinking: null }
            : { ...agent, currentThinking: null }
        ))
        // Also reset selected agent if present
        setSelectedAgent(prev => prev?.status === 'Executing'
          ? { ...prev, status: 'Active', currentThinking: null }
          : prev
        )
      } else {
        // Connection restored - refresh agent status from server
        loadItems()
      }
    },
    onEvent: (event) => {
      const eventData = event.data as { agent_id?: string }

      switch (event.type) {
        case 'AgentExecutionStarted': {
          const startedData = (event as AgentExecutionStartedEvent).data
          // Add to executing map with timestamp
          setExecutingAgents(prev => new Map(prev).set(startedData.agent_id, Date.now()))

          // Update the specific agent's status in the list
          setAgents(prev => prev.map(agent =>
            agent.id === startedData.agent_id
              ? { ...agent, status: 'Executing' as const }
              : agent
          ))

          // Update selected agent if it's the same one
          if (selectedAgent?.id === startedData.agent_id) {
            setSelectedAgent(prev => prev ? { ...prev, status: 'Executing' } : null)
          }
          break
        }

        case 'AgentExecutionCompleted': {
          const completedData = (event as AgentExecutionCompletedEvent).data

          // Remove from executing map immediately
          setExecutingAgents(prev => {
            const next = new Map(prev)
            next.delete(completedData.agent_id)
            return next
          })

          // Clear thinking state immediately
          setAgentThinking(prev => {
            const next = { ...prev }
            delete next[completedData.agent_id]
            return next
          })

          // Immediately update the agent's status in the list to Active or Error
          const newStatus: AiAgent['status'] = completedData.success ? 'Active' : 'Error'
          setAgents(prev => prev.map(agent =>
            agent.id === completedData.agent_id
              ? { ...agent, status: newStatus }
              : agent
          ))

          // Update selected agent if it's the same one
          if (selectedAgent?.id === completedData.agent_id) {
            setSelectedAgent(prev => prev ? {
              ...prev,
              status: newStatus
            } : null)
          }
          break
        }

        case 'AgentThinking': {
          const thinkingData = (event as AgentThinkingEvent).data
          // Buffer thinking update and flush at most every 200ms
          pendingThinkingRef.current[thinkingData.agent_id] = thinkingData.description
          const now = Date.now()
          if (now - lastThinkingUpdateRef.current >= 200) {
            lastThinkingUpdateRef.current = now
            const pending = { ...pendingThinkingRef.current }
            pendingThinkingRef.current = {}
            setAgentThinking(prev => ({ ...prev, ...pending }))
          }
          break
        }
      }
    },
  })

  // Auto-cleanup: Remove agents from executing state after timeout (5 minutes)
  useEffect(() => {
    const EXECUTION_TIMEOUT_MS = 5 * 60 * 1000 // 5 minutes

    const interval = setInterval(() => {
      const now = Date.now()
      const expiredIds: string[] = []

      setExecutingAgents(prev => {
        const next = new Map<string, number>()
        let hasChanges = false

        prev.forEach((timestamp, agentId) => {
          if (now - timestamp > EXECUTION_TIMEOUT_MS) {
            hasChanges = true
            expiredIds.push(agentId)
          } else {
            next.set(agentId, timestamp)
          }
        })

        return hasChanges ? next : prev
      })

      // Clean up thinking state and reload outside of setState callback
      if (expiredIds.length > 0) {
        setAgentThinking(prev => {
          const next = { ...prev }
          for (const id of expiredIds) {
            delete next[id]
          }
          return next
        })
        loadItems()
      }
    }, 30000) // Check every 30 seconds

    return () => clearInterval(interval)
  }, [loadItems])

  // Handlers
  const handleCreate = () => {
    setEditingAgent(undefined)
    setShowAgentDialog(true)
    loadEditorResources()
  }

  const handleEdit = async (agent: AiAgent) => {
    try {
      setShowAgentDialog(true)
      const [detail] = await Promise.all([
        api.getAgent(agent.id),
        loadEditorResources(),
      ])
      setEditingAgent(detail)
    } catch (error) {
      setShowAgentDialog(false)
      handleError(error, { operation: 'Load agent details', showToast: false })
      showErrorToast(toast, error, tCommon('failed'))
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
      handleError(error, { operation: 'Delete agent', showToast: true })
      showErrorToast(toast, error, tCommon('failed'))
    }
  }

  const handleToggleStatus = async (agent: AiAgent) => {
    try {
      const newStatus = agent.status === 'Active' ? 'paused' : 'active'
      await api.setAgentStatus(agent.id, newStatus)
      await loadItems()
    } catch (error) {
      handleError(error, { operation: 'Toggle agent status', showToast: true })
      showErrorToast(toast, error, tCommon('failed'))
    }
  }

  const handleExecute = async (agent: AiAgent) => {
    // Immediately mark as executing for responsive UI
    setAgents(prev => prev.map(a =>
      a.id === agent.id ? { ...a, status: 'Executing' } : a
    ))
    setExecutingAgents(prev => new Map(prev).set(agent.id, Date.now()))

    try {
      // API returns immediately — execution runs in background
      await api.executeAgent(agent.id)
      toast({
        title: tCommon('success'),
        description: tAgent('executionStarted', { id: agent.name }),
      })
    } catch (error) {
      // Revert status on error
      setAgents(prev => prev.map(a =>
        a.id === agent.id ? { ...a, status: 'Active' } : a
      ))
      setExecutingAgents(prev => {
        const next = new Map(prev)
        next.delete(agent.id)
        return next
      })
      handleError(error, { operation: 'Execute agent', showToast: true })
      showErrorToast(toast, error, tCommon('failed'))
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
      // Toast is shown by AgentEditorFullScreen — don't duplicate here
    } catch (error) {
      handleError(error, { operation: 'Save agent', showToast: false })
      throw error
    }
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
      handleError(error, { operation: 'Load agent details for panel', showToast: false })
      showErrorToast(toast, error, tCommon('failed'))
    }
  }

  // Edit from detail: close detail first to avoid scroll-through bug
  const handleEditFromDetail = (agent: AiAgentDetail) => {
    setDetailDialogOpen(false)
    setEditingAgent(agent)
    setShowAgentDialog(true)
    loadEditorResources()
  }

  // Note: detail data is loaded solely by handleViewDetail. The dialog's only
  // open path is through that handler (which fetches fresh data each click),
  // so no separate refresh effect is needed.

  // Merge executing state from WebSocket with agent data (memoized to avoid re-rendering all cards)
  // Only show Executing if agent is currently executing AND not paused/error in database
  const agentsWithExecutingStatus = useMemo(() => agents.map(agent => {
    // If agent is paused or error in database, don't override with executing state
    if (agent.status === 'Paused' || agent.status === 'Error') {
      return {
        ...agent,
        currentThinking: null
      };
    }
    // Show Executing only if in executing set and currently Active/Executing in database
    return {
      ...agent,
      status: executingAgents.has(agent.id) ? 'Executing' : agent.status,
      currentThinking: executingAgents.has(agent.id) ? (agentThinking[agent.id] || null) : null
    };
  }), [agents, executingAgents, agentThinking])

  const isMobile = useIsMobile()

  // Pagination state
  const AGENTS_PER_PAGE = 10
  const [agentsPage, setAgentsPage] = useState(1)

  // Reset pagination when agents change
  useEffect(() => {
    setAgentsPage(1)
  }, [agents.length])

  // Paginated agents
  // On mobile: show cumulative data (all pages up to current)
  // On desktop: show only current page
  const paginatedAgents = useMemo(() => {
    if (isMobile) {
      return agentsWithExecutingStatus.slice(0, agentsPage * AGENTS_PER_PAGE)
    } else {
      const startIndex = (agentsPage - 1) * AGENTS_PER_PAGE
      return agentsWithExecutingStatus.slice(startIndex, startIndex + AGENTS_PER_PAGE)
    }
  }, [agentsWithExecutingStatus, agentsPage, isMobile])

  const tabs = [
    { value: 'agents', label: tAgent('tabs.agents'), icon: <Cpu className="h-4 w-4" /> },
    { value: 'memory', label: tAgent('tabs.memory'), icon: <Brain className="h-4 w-4" /> },
    { value: 'skills', label: tAgent('tabs.skills'), icon: <BookOpen className="h-4 w-4" /> },
    { value: 'tools', label: tAgent('tabs.tools'), icon: <Wrench className="h-4 w-4" /> },
  ]

  const tabActions = activeTab === 'agents' && agents.length > 0
    ? [{ label: tAgent('createAgent'), icon: <Plus className="h-4 w-4" />, onClick: handleCreate }]
    : activeTab === 'memory'
    ? [
        { label: tAgent('systemMemory.custom.create', 'Add File'), icon: <FileText className="h-4 w-4" />, onClick: () => memoryPanelRef.current?.openCreateFile() },
        { label: tAgent('systemMemory.config.title', 'Config'), icon: <Settings className="h-4 w-4" />, onClick: () => memoryPanelRef.current?.openConfig() },
      ]
    : activeTab === 'skills'
    ? [
        { label: tAgent('skills.createSkill', 'Add Skill'), icon: <Plus className="h-4 w-4" />, onClick: () => skillsPanelRef.current?.openCreate() },
      ]
    : []

  const skillsActionsExtra = activeTab === 'tools' ? (
    <div className="flex items-center gap-2">
      <Select value={toolsSourceFilter} onValueChange={(v) => setToolsSourceFilter(v as ToolSourceFilter)}>
        <SelectTrigger className="h-9 w-[140px]">
          <SelectValue placeholder={tAgent('detail.toolsFilterAllSources', 'All sources')} />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{tAgent('detail.toolsFilterAllSources', 'All sources')}</SelectItem>
          <SelectItem value="built-in">{tAgent('detail.toolsSourceBuiltIn')}</SelectItem>
          <SelectItem value="extension">{tAgent('detail.toolsSourceExtension')}</SelectItem>
        </SelectContent>
      </Select>
      <div className="relative">
        <span className="absolute left-2.5 top-0 bottom-0 flex items-center">
          <Search className="h-4 w-4 text-muted-foreground pointer-events-none" />
        </span>
        <Input
          placeholder={tAgent('detail.toolsSearchPlaceholder', 'Search tools…')}
          value={toolsSearch}
          onChange={(e) => setToolsSearch(e.target.value)}
          className="pl-9 w-[180px] md:w-[240px] h-9"
        />
      </div>
    </div>
  ) : undefined

  return (
    <PageLayout
      title={tAgent('title')}
      subtitle={tAgent('description')}
      headerContent={
        <PageTabsBar
          tabs={tabs}
          activeTab={activeTab}
          onTabChange={handleTabChange}
          actions={tabActions}
          actionsExtra={skillsActionsExtra}
        />
      }
      hideFooterOnMobile
      hasBottomNav
      footer={
        activeTab === 'agents' && agents.length > AGENTS_PER_PAGE ? (
          <Pagination
            total={agents.length}
            pageSize={AGENTS_PER_PAGE}
            currentPage={agentsPage}
            onPageChange={setAgentsPage}
          />
        ) : activeTab === 'skills' && skillsPagination.total > skillsPagination.pageSize ? (
          <Pagination
            total={skillsPagination.total}
            pageSize={skillsPagination.pageSize}
            currentPage={skillsPagination.currentPage}
            onPageChange={skillsPagination.onPageChange}
            isLoading={skillsPagination.loading}
          />
        ) : activeTab === 'tools' && toolsPagination.total > toolsPagination.pageSize ? (
          <Pagination
            total={toolsPagination.total}
            pageSize={toolsPagination.pageSize}
            currentPage={toolsPagination.currentPage}
            onPageChange={toolsPagination.onPageChange}
            isLoading={toolsPagination.loading}
          />
        ) : undefined
      }
    >
      <PageTabsContent value="agents" activeTab={activeTab}>
        {agents.length === 0 && loading ? (
          <LoadingState variant="page" />
        ) : agents.length === 0 ? (
          <div className="flex min-h-[500px] items-center justify-center">
            {llmBackends.length === 0 && !llmBackendLoading ? (
              /* No backend — same empty-state visual, but the guidance swaps:
                 agents are inert without a model, so the CTAs lead with model
                 setup; creating an agent first stays reachable via text link. */
              <div className="text-center max-w-md px-6">
                <div className="mx-auto mb-5 flex size-14 items-center justify-center rounded-xl bg-primary-light text-primary">
                  <Bot className="size-7" />
                </div>
                <h2 className="mb-2 text-lg font-semibold tracking-tight">{tAgent('noAgents')}</h2>
                <p className="text-sm text-muted-foreground mb-6 leading-relaxed">
                  {tAgent('noAgentsNoBackendDesc')}
                </p>
                <div className="flex flex-col items-center gap-3">
                  <Button className="w-full h-11 sm:h-10 gap-2" onClick={() => setAgentWizardOpen(true)}>
                    <Download className="h-4 w-4" />
                    {tCommon('llmGuide.builtinShort')}
                  </Button>
                  <Button variant="secondary" className="w-full h-11 sm:h-10 gap-2" onClick={() => openSettings('llm')}>
                    <Server className="h-4 w-4" />
                    {tCommon('llmGuide.ownShort')}
                  </Button>
                </div>
                <button
                  className="mt-4 text-xs text-muted-foreground hover:text-foreground transition-colors"
                  onClick={handleCreate}
                >
                  {tAgent('createAgentAnyway')}
                </button>
                <BuiltinModelWizard
                  open={agentWizardOpen}
                  onOpenChange={setAgentWizardOpen}
                  onActivated={() => setAgentWizardOpen(false)}
                />
              </div>
            ) : (
              <EmptyState
                icon={<Bot className="h-12 w-12" />}
                title={tAgent('noAgents')}
                description={tAgent('noAgentsDesc')}
                action={{
                  label: tAgent('createAgent'),
                  onClick: handleCreate,
                }}
              />
            )}
          </div>
        ) : (
          <div className="grid grid-cols-1 @md:grid-cols-2 @lg:grid-cols-3 @xl:grid-cols-4 gap-4">
            {paginatedAgents.map((agent, index) => (
              <div
                key={agent.id}
                className="animate-fade-in-up"
                style={{ animationDelay: `${index * 50}ms`, animationFillMode: 'both' }}
              >
                <AgentCard
                  agent={agent}
                  onToggleStatus={handleToggleStatus}
                  onExecute={handleExecute}
                  onEdit={handleEdit}
                  onDelete={handleDelete}
                  onClick={() => handleViewDetail(agent)}
                />
              </div>
            ))}
          </div>
        )}
      </PageTabsContent>

      <PageTabsContent value="memory" activeTab={activeTab}>
        <MemoryPanel ref={memoryPanelRef} refreshKey={memoryRefreshKey} />
      </PageTabsContent>

      <PageTabsContent value="skills" activeTab={activeTab}>
        <SkillsPanel ref={skillsPanelRef} onPaginationChange={handleSkillsPaginationChange} />
      </PageTabsContent>

      <PageTabsContent value="tools" activeTab={activeTab}>
        <ToolsPanel
          onPaginationChange={handleToolsPaginationChange}
          searchQuery={toolsDebouncedSearch}
          sourceFilter={toolsSourceFilter}
        />
      </PageTabsContent>

      {/* Mobile: Bottom navigation bar */}
      <PageTabsBottomNav
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={handleTabChange}
      />

      {/* Agent Editor Full Screen */}
      <AgentEditorFullScreen
        open={showAgentDialog}
        onOpenChange={setShowAgentDialog}
        agent={editingAgent}
        devices={devices}
        deviceTypes={deviceTypes}
        extensions={extensions}
        unifiedDataSources={unifiedDataSources}
        onSave={handleSave}
      />

      {/* Agent Detail Dialog */}
      <FullScreenDialog open={detailDialogOpen} onOpenChange={setDetailDialogOpen}>
        <FullScreenDialogHeader
          icon={<Bot className="h-5 w-5" />}
          iconBg="bg-accent-purple-light"
          iconColor="text-accent-purple"
          title={selectedAgent?.name || tAgent('detailTitle')}
          subtitle={selectedAgent?.description}
          onClose={() => setDetailDialogOpen(false)}
          actions={
            <>
              <Button variant="outline" size="sm" onClick={() => selectedAgent && handleEditFromDetail(selectedAgent)}>
                <Edit className="h-4 w-4 mr-1.5" />
                {tAgent('detail.edit')}
              </Button>
              <Button size="sm" onClick={() => selectedAgent && handleExecute(selectedAgent)} disabled={selectedAgent?.status === 'Executing'}>
                <Play className="h-4 w-4 mr-1.5" />
                {tAgent('detail.execute')}
              </Button>
            </>
          }
        />
        <FullScreenDialogContent>
          <FullScreenDialogMain className="overflow-auto">
            {selectedAgent && (
              <AgentDetailPanel
                agent={selectedAgent}
                onEdit={handleEditFromDetail}
                onExecute={handleExecute}
                onViewExecutionDetail={handleViewExecutionDetail}
                onRefresh={loadItems}
                inlineMode
              />
            )}
          </FullScreenDialogMain>
        </FullScreenDialogContent>
      </FullScreenDialog>

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

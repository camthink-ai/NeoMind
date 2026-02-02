// Messages Page
// Unified notification/message system for NeoMind

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useLocation } from 'react-router-dom'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabs, PageTabsContent, EmptyStateInline, Pagination } from '@/components/shared'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { confirm } from '@/hooks/use-confirm'
import type { NotificationMessage, MessageSeverity, MessageStatus, MessageCategory } from '@/types'
import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuCheckboxItem,
} from '@/components/ui/dropdown-menu'
import {
  AlertCircle,
  Info,
  AlertTriangle,
  ShieldAlert,
  Trash2,
  Bell,
  Megaphone,
  MoreVertical,
  Eye,
  RefreshCw,
  Filter,
  X,
  CheckCircle2,
  TestTube,
} from 'lucide-react'
import { CreateMessageDialog } from '@/components/messages/CreateMessageDialog'
import { formatTimestamp } from '@/lib/utils/format'
import { cn } from '@/lib/utils'

type TabValue = 'messages' | 'channels'

// Get tab from URL path
const getTabFromPath = (pathname: string): TabValue => {
  const pathSegments = pathname.split('/')
  const lastSegment = pathSegments[pathSegments.length - 1]
  if (lastSegment === 'channels') {
    return 'channels'
  }
  return 'messages'
}

// Severity config
const SEVERITY_CONFIG: Record<string, { icon: typeof Info; color: string; bgColor: string }> = {
  info: { icon: Info, color: 'text-blue-500', bgColor: 'bg-blue-500/10 border-blue-500/20' },
  warning: { icon: AlertTriangle, color: 'text-yellow-500', bgColor: 'bg-yellow-500/10 border-yellow-500/20' },
  critical: { icon: AlertCircle, color: 'text-orange-500', bgColor: 'bg-orange-500/10 border-orange-500/20' },
  emergency: { icon: ShieldAlert, color: 'text-red-500', bgColor: 'bg-red-500/10 border-red-500/20' },
}

// Status config
const STATUS_CONFIG: Record<string, { variant: 'default' | 'secondary' | 'outline'; label: string }> = {
  active: { variant: 'default', label: 'messages.status.active' },
  acknowledged: { variant: 'secondary', label: 'messages.status.acknowledged' },
  resolved: { variant: 'outline', label: 'messages.status.resolved' },
  archived: { variant: 'outline', label: 'messages.status.archived' },
  false_positive: { variant: 'outline', label: 'messages.status.false_positive' },
}

// Category config
const CATEGORY_CONFIG: Record<string, { label: string; icon: typeof Bell }> = {
  alert: { label: 'messages.category.alert', icon: AlertCircle },
  system: { label: 'messages.category.system', icon: Bell },
  business: { label: 'messages.category.business', icon: Megaphone },
}

export default function MessagesPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const location = useLocation()

  // Helper to get API base URL for Tauri environment
  const getApiUrl = (path: string) => {
    const apiBase = (window as any).__TAURI__ ? 'http://localhost:3000/api' : '/api'
    return `${apiBase}${path}`
  }

  // Tab state - sync with URL
  const [activeTab, setActiveTab] = useState<TabValue>(() => getTabFromPath(location.pathname))

  // Messages state
  const [messages, setMessages] = useState<NotificationMessage[]>([])
  const [loading, setLoading] = useState(false)

  // Pagination
  const [messagePage, setMessagePage] = useState(1)
  const messagesPerPage = 10

  // Filters - support multiple selections
  const [selectedSeverities, setSelectedSeverities] = useState<Set<MessageSeverity>>(new Set())
  const [selectedStatuses, setSelectedStatuses] = useState<Set<MessageStatus>>(new Set())
  const [selectedCategories, setSelectedCategories] = useState<Set<MessageCategory>>(new Set())

  // Check if any filters are active
  const hasActiveFilters = selectedSeverities.size > 0 || selectedStatuses.size > 0 || selectedCategories.size > 0

  // Clear all filters
  const clearAllFilters = () => {
    setSelectedSeverities(new Set())
    setSelectedStatuses(new Set())
    setSelectedCategories(new Set())
  }

  // Toggle filter
  const toggleSeverity = (severity: MessageSeverity) => {
    const newSet = new Set(selectedSeverities)
    if (newSet.has(severity)) {
      newSet.delete(severity)
    } else {
      newSet.add(severity)
    }
    setSelectedSeverities(newSet)
  }

  const toggleStatus = (status: MessageStatus) => {
    const newSet = new Set(selectedStatuses)
    if (newSet.has(status)) {
      newSet.delete(status)
    } else {
      newSet.add(status)
    }
    setSelectedStatuses(newSet)
  }

  const toggleCategory = (category: MessageCategory) => {
    const newSet = new Set(selectedCategories)
    if (newSet.has(category)) {
      newSet.delete(category)
    } else {
      newSet.add(category)
    }
    setSelectedCategories(newSet)
  }

  // Get active filter count
  const getActiveFilterCount = () => selectedSeverities.size + selectedStatuses.size + selectedCategories.size

  // Test channel state
  const [testingChannel, setTestingChannel] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // Test a channel
  const handleTestChannel = async (channelName: string) => {
    setTestingChannel(channelName)
    try {
      const response = await fetch(getApiUrl(`/messages/channels/${encodeURIComponent(channelName)}/test`), {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          message: 'Test message from NeoMind',
          title: 'Channel Test',
        }),
      })
      const result = await response.json()
      if (result.success) {
        setTestResults(prev => ({ ...prev, [channelName]: { success: true, message: result.message || 'Test sent successfully' } }))
        toast({ title: t('common.success'), description: `Channel "${channelName}" test successful` })
      } else {
        throw new Error(result.message || 'Test failed')
      }
    } catch (error: any) {
      const errorMsg = error?.message || String(error)
      setTestResults(prev => ({ ...prev, [channelName]: { success: false, message: errorMsg } }))
      toast({ title: t('common.failed'), description: errorMsg, variant: 'destructive' })
    } finally {
      setTestingChannel(null)
    }
  }

  // Calculate paginated messages
  const paginatedMessages = messages.slice(
    (messagePage - 1) * messagesPerPage,
    messagePage * messagesPerPage
  )

  // Channels state
  const [channels, setChannels] = useState<any[]>([])

  // Dialogs
  const [createDialogOpen, setCreateDialogOpen] = useState(false)

  const { toast } = useToast()

  // Update tab when URL changes
  useEffect(() => {
    setActiveTab(getTabFromPath(location.pathname))
  }, [location.pathname])

  // Update URL when tab changes
  const handleTabChange = (tab: TabValue) => {
    setActiveTab(tab)
    if (tab === 'channels') {
      navigate('/messages/channels')
    } else {
      navigate('/messages')
    }
  }

  // Fetch messages using the unified messages API
  const fetchMessages = useCallback(async () => {
    setLoading(true)
    try {
      const response = await fetch(getApiUrl('/messages'), {
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
        },
      })
      const rawData = await response.json() as any
      console.log('Messages API response:', rawData)

      // Handle different response formats from messages endpoint
      let messagesArray: any[] = []
      if (Array.isArray(rawData)) {
        messagesArray = rawData
      } else if (rawData?.messages && Array.isArray(rawData.messages)) {
        messagesArray = rawData.messages
      } else if (rawData?.data?.messages && Array.isArray(rawData.data.messages)) {
        messagesArray = rawData.data.messages
      } else if (rawData?.data && Array.isArray(rawData.data)) {
        messagesArray = rawData.data
      }

      // Convert to NotificationMessage format
      let messages: NotificationMessage[] = messagesArray.map((msg: any) => ({
        id: msg.id || '',
        category: msg.category || 'alert',
        severity: msg.severity || 'info',
        title: msg.title || '',
        message: msg.message || '',
        source: msg.source || 'system',
        source_type: msg.source_type || '',
        timestamp: msg.timestamp || msg.created_at || new Date().toISOString(),
        status: msg.status || 'active',
        tags: msg.tags || [],
        metadata: msg.metadata,
      }))

      // Apply filters using Sets
      if (selectedSeverities.size > 0) {
        messages = messages.filter((m: NotificationMessage) => selectedSeverities.has(m.severity as MessageSeverity))
      }
      if (selectedStatuses.size > 0) {
        messages = messages.filter((m: NotificationMessage) => selectedStatuses.has(m.status as MessageStatus))
      }
      if (selectedCategories.size > 0) {
        messages = messages.filter((m: NotificationMessage) => selectedCategories.has(m.category as MessageCategory))
      }

      // Sort by timestamp descending (handle invalid timestamps)
      messages.sort((a: NotificationMessage, b: NotificationMessage) => {
        const aTime = new Date(a.timestamp).getTime()
        const bTime = new Date(b.timestamp).getTime()
        // If either timestamp is invalid, treat it as oldest
        if (isNaN(aTime)) return 1
        if (isNaN(bTime)) return -1
        return bTime - aTime
      })

      setMessages(messages)
      setMessagePage(1) // Reset to first page when data changes
    } catch (error) {
      console.error('Failed to fetch messages:', error)
    } finally {
      setLoading(false)
    }
  }, [selectedSeverities, selectedStatuses, selectedCategories])

  // Fetch channels
  const fetchChannels = useCallback(async () => {
    setLoading(true)
    try {
      const response = await api.listMessageChannels()
      setChannels(response.channels || [])
    } catch (error) {
      console.error('Failed to fetch channels:', error)
    } finally {
      setLoading(false)
    }
  }, [])

  // Initial load
  useEffect(() => {
    if (activeTab === 'messages') {
      fetchMessages()
    } else {
      fetchChannels()
    }
  }, [activeTab, fetchMessages, fetchChannels, selectedSeverities, selectedStatuses, selectedCategories])

  // Message actions - using messages API endpoints
  const handleAcknowledge = async (id: string) => {
    try {
      const response = await fetch(getApiUrl(`/messages/${id}/acknowledge`), {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
          'Content-Type': 'application/json',
        },
      })
      if (response.ok) {
        setMessages(prev => prev.map(m =>
          m.id === id ? { ...m, status: 'acknowledged' as MessageStatus, acknowledged: true } : m
        ))
        toast({ title: t('messages.acknowledgeSuccess', 'Acknowledged') })
      } else {
        throw new Error('Failed to acknowledge')
      }
    } catch (error) {
      console.error('Failed to acknowledge:', error)
      toast({ title: t('messages.acknowledgeError', 'Failed'), variant: 'destructive' })
    }
  }

  const handleResolve = async (id: string) => {
    try {
      const response = await fetch(getApiUrl(`/messages/${id}/resolve`), {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
          'Content-Type': 'application/json',
        },
      })
      if (response.ok) {
        setMessages(prev => prev.map(m =>
          m.id === id ? { ...m, status: 'resolved' as MessageStatus, acknowledged: true } : m
        ))
        toast({ title: t('messages.resolveSuccess', 'Resolved') })
      } else {
        throw new Error('Failed to resolve')
      }
    } catch (error) {
      console.error('Failed to resolve:', error)
      toast({ title: t('messages.resolveError', 'Failed'), variant: 'destructive' })
    }
  }

  const handleArchive = async (id: string) => {
    try {
      const response = await fetch(getApiUrl(`/messages/${id}/archive`), {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
          'Content-Type': 'application/json',
        },
      })
      if (response.ok) {
        setMessages(prev => prev.map(m =>
          m.id === id ? { ...m, status: 'archived' as MessageStatus } : m
        ))
        toast({ title: t('messages.archiveSuccess', 'Archived') })
      } else {
        throw new Error('Failed to archive')
      }
    } catch (error) {
      console.error('Failed to archive:', error)
      toast({ title: t('messages.archiveError', 'Failed'), variant: 'destructive' })
    }
  }

  const handleDelete = async (id: string) => {
    const confirmed = await confirm({
      title: t('delete'),
      description: t('messages.confirmBulkDeleteDesc', { count: 1 }),
      confirmText: t('delete'),
      cancelText: t('cancel'),
      variant: 'destructive',
    })
    if (!confirmed) return

    try {
      const response = await fetch(getApiUrl(`/messages/${id}`), {
        method: 'DELETE',
        headers: {
          'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
        },
      })
      if (response.ok) {
        setMessages(prev => prev.filter(m => m.id !== id))
        toast({ title: t('messages.deleteSuccess', 'Deleted') })
      } else {
        throw new Error('Failed to delete')
      }
    } catch (error) {
      console.error('Failed to delete:', error)
      toast({ title: t('messages.deleteError', 'Failed to delete'), variant: 'destructive' })
    }
  }

  const tabs = [
    { value: 'messages' as TabValue, label: t('messages.tabs.messages') },
    { value: 'channels' as TabValue, label: t('messages.tabs.channels') },
  ]

  return (
    <PageLayout
      title={t('messages.title')}
      subtitle={t('messages.description')}
      footer={
        activeTab === 'messages' && messages.length > messagesPerPage ? (
          <Pagination
            total={messages.length}
            pageSize={messagesPerPage}
            currentPage={messagePage}
            onPageChange={setMessagePage}
          />
        ) : undefined
      }
    >
      <PageTabs
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => handleTabChange(v as TabValue)}
        actions={[
          ...(activeTab === 'messages' ? [
            { label: t('messages.create'), onClick: () => setCreateDialogOpen(true) },
          ] : []),
          { label: t('refresh'), variant: 'outline' as const, onClick: activeTab === 'messages' ? fetchMessages : fetchChannels, disabled: loading },
        ]}
      >
        {/* Messages Tab */}
        <PageTabsContent value="messages" activeTab={activeTab} className="flex flex-col overflow-hidden">
          {/* Filters */}
          <div className="flex flex-wrap items-center gap-2 mb-4 shrink-0">
            {/* Filter Button with Dropdown */}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm" className="gap-2">
                  <Filter className="h-4 w-4" />
                  {t('messages.filter.title')}
                  {getActiveFilterCount() > 0 && (
                    <Badge variant="secondary" className="h-5 px-1.5 text-xs">
                      {getActiveFilterCount()}
                    </Badge>
                  )}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56 max-h-[70vh] overflow-y-auto">
                {/* Severity Filter */}
                <div className="px-2 py-1.5">
                  <p className="text-xs font-medium text-muted-foreground mb-1">{t('messages.severity.label')}</p>
                  {(['info', 'warning', 'critical', 'emergency'] as MessageSeverity[]).map((sev) => (
                    <DropdownMenuCheckboxItem
                      key={sev}
                      checked={selectedSeverities.has(sev)}
                      onCheckedChange={() => toggleSeverity(sev)}
                    >
                      <div className="flex items-center gap-2">
                        {sev === 'info' && <Info className="h-3.5 w-3.5 text-blue-500" />}
                        {sev === 'warning' && <AlertTriangle className="h-3.5 w-3.5 text-yellow-500" />}
                        {sev === 'critical' && <AlertCircle className="h-3.5 w-3.5 text-orange-500" />}
                        {sev === 'emergency' && <ShieldAlert className="h-3.5 w-3.5 text-red-500" />}
                        {t(`messages.severity.${sev}`)}
                      </div>
                    </DropdownMenuCheckboxItem>
                  ))}
                </div>

                <DropdownMenuSeparator />

                {/* Status Filter */}
                <div className="px-2 py-1.5">
                  <p className="text-xs font-medium text-muted-foreground mb-1">{t('messages.status.label')}</p>
                  {(['active', 'acknowledged', 'resolved', 'archived'] as MessageStatus[]).map((stat) => (
                    <DropdownMenuCheckboxItem
                      key={stat}
                      checked={selectedStatuses.has(stat)}
                      onCheckedChange={() => toggleStatus(stat)}
                    >
                      {t(`messages.status.${stat}`)}
                    </DropdownMenuCheckboxItem>
                  ))}
                </div>

                <DropdownMenuSeparator />

                {/* Category Filter */}
                <div className="px-2 py-1.5">
                  <p className="text-xs font-medium text-muted-foreground mb-1">{t('messages.category.label')}</p>
                  {(['alert', 'system', 'business'] as MessageCategory[]).map((cat) => (
                    <DropdownMenuCheckboxItem
                      key={cat}
                      checked={selectedCategories.has(cat)}
                      onCheckedChange={() => toggleCategory(cat)}
                    >
                      <div className="flex items-center gap-2">
                        {cat === 'alert' && <AlertCircle className="h-3.5 w-3.5" />}
                        {cat === 'system' && <Bell className="h-3.5 w-3.5" />}
                        {cat === 'business' && <Megaphone className="h-3.5 w-3.5" />}
                        {t(`messages.category.${cat}`)}
                      </div>
                    </DropdownMenuCheckboxItem>
                  ))}
                </div>

                {hasActiveFilters && (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={clearAllFilters}>
                      <X className="h-4 w-4 mr-2" />
                      {t('messages.filter.clear')}
                    </DropdownMenuItem>
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>

            {/* Active Filter Chips */}
            {Array.from(selectedSeverities).map((sev) => (
              <Badge
                key={`sev-${sev}`}
                variant="secondary"
                className="gap-1 pr-1 cursor-pointer hover:bg-secondary/80"
                onClick={() => toggleSeverity(sev)}
              >
                {sev === 'info' && <Info className="h-3 w-3 text-blue-500" />}
                {sev === 'warning' && <AlertTriangle className="h-3 w-3 text-yellow-500" />}
                {sev === 'critical' && <AlertCircle className="h-3 w-3 text-orange-500" />}
                {sev === 'emergency' && <ShieldAlert className="h-3 w-3 text-red-500" />}
                {t(`messages.severity.${sev}`)}
                <X className="h-3 w-3 ml-1 text-muted-foreground" />
              </Badge>
            ))}

            {Array.from(selectedStatuses).map((stat) => (
              <Badge
                key={`stat-${stat}`}
                variant="secondary"
                className="gap-1 pr-1 cursor-pointer hover:bg-secondary/80"
                onClick={() => toggleStatus(stat)}
              >
                {t(`messages.status.${stat}`)}
                <X className="h-3 w-3 ml-1 text-muted-foreground" />
              </Badge>
            ))}

            {Array.from(selectedCategories).map((cat) => (
              <Badge
                key={`cat-${cat}`}
                variant="secondary"
                className="gap-1 pr-1 cursor-pointer hover:bg-secondary/80"
                onClick={() => toggleCategory(cat)}
              >
                {cat === 'alert' && <AlertCircle className="h-3 w-3" />}
                {cat === 'system' && <Bell className="h-3 w-3" />}
                {cat === 'business' && <Megaphone className="h-3 w-3" />}
                {t(`messages.category.${cat}`)}
                <X className="h-3 w-3 ml-1 text-muted-foreground" />
              </Badge>
            ))}

            {hasActiveFilters && (
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs"
                onClick={clearAllFilters}
              >
                {t('messages.filter.clearAll')}
              </Button>
            )}
          </div>

          {/* Messages Table - Scrollable Area */}
          <Card className="overflow-hidden flex-1 min-h-0">
            <div className="overflow-auto h-full">
              <Table>
                <TableHeader className="sticky top-0 bg-background/95 backdrop-blur-sm z-10">
                  <TableRow className="hover:bg-transparent border-b bg-muted/30">
                    <TableHead className="w-[50px]"></TableHead>
                    <TableHead>
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('messages.formTitle.label')}
                      </div>
                    </TableHead>
                    <TableHead>
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('messages.content.label')}
                      </div>
                    </TableHead>
                    <TableHead className="w-[90px]">
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('messages.severity.label')}
                      </div>
                    </TableHead>
                    <TableHead className="w-[90px]">
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('messages.category.label')}
                      </div>
                    </TableHead>
                    <TableHead className="w-[90px]">
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('messages.status.label')}
                      </div>
                    </TableHead>
                    <TableHead className="w-[130px]">
                      <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                        {t('common.createdAt')}
                      </div>
                    </TableHead>
                    <TableHead className="w-12"></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {loading ? (
                    <TableRow>
                      <TableCell colSpan={8} className="py-8">
                        <div className="flex items-center justify-center gap-2">
                          <RefreshCw className="h-4 w-4 animate-spin" />
                          <span className="text-sm text-muted-foreground">{t('loading')}</span>
                        </div>
                      </TableCell>
                    </TableRow>
                  ) : messages.length === 0 ? (
                    <EmptyStateInline
                      title={t('messages.empty.title')}
                      colSpan={8}
                    />
                  ) : (
                    paginatedMessages.map((message) => {
                      const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                      const categoryConfig = CATEGORY_CONFIG[message.category] || CATEGORY_CONFIG.system
                      const statusConfig = STATUS_CONFIG[message.status] || STATUS_CONFIG.active
                      const SeverityIcon = severityConfig.icon
                      const CategoryIcon = categoryConfig.icon

                      return (
                        <TableRow
                          key={message.id}
                          className={cn(
                            "group transition-colors hover:bg-muted/50",
                            (message.status === 'resolved' || message.status === 'archived') && "opacity-60"
                          )}
                        >
                          <TableCell className="text-center">
                            <SeverityIcon className={cn("h-4 w-4 mx-auto", severityConfig.color)} />
                          </TableCell>
                          <TableCell>
                            <div className="font-medium text-sm truncate pr-4" title={message.title}>
                              {message.title}
                            </div>
                          </TableCell>
                          <TableCell>
                            <div className="text-sm text-muted-foreground line-clamp-2">
                              {message.message}
                            </div>
                            {message.tags.length > 0 && (
                              <div className="flex items-center gap-1 mt-1.5 flex-wrap">
                                {message.tags.slice(0, 4).map((tag, i) => (
                                  <Badge key={i} variant="secondary" className="text-xs h-5 px-1.5">
                                    {tag}
                                  </Badge>
                                ))}
                                {message.tags.length > 4 && (
                                  <span className="text-xs text-muted-foreground">+{message.tags.length - 4}</span>
                                )}
                              </div>
                            )}
                          </TableCell>
                          <TableCell>
                            <Badge variant="outline" className={cn("text-xs", severityConfig.bgColor, severityConfig.color)}>
                              {t(`messages.severity.${message.severity}`)}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <div className="flex items-center gap-1.5">
                              <CategoryIcon className="h-3.5 w-3.5 text-muted-foreground" />
                              <span className="text-xs">{t(categoryConfig.label)}</span>
                            </div>
                          </TableCell>
                          <TableCell>
                            <Badge variant={statusConfig.variant} className="text-xs">
                              {t(statusConfig.label)}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            <span className="text-xs text-muted-foreground">
                              {formatTimestamp(message.timestamp, false)}
                            </span>
                          </TableCell>
                          <TableCell>
                            <DropdownMenu>
                              <DropdownMenuTrigger asChild>
                                <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                                  <MoreVertical className="h-4 w-4" />
                                </Button>
                              </DropdownMenuTrigger>
                              <DropdownMenuContent align="end" className="w-40">
                                {message.status === 'active' && (
                                  <DropdownMenuItem onClick={() => handleAcknowledge(message.id)}>
                                    <Eye className="mr-2 h-4 w-4" />
                                    {t('messages.acknowledge')}
                                  </DropdownMenuItem>
                                )}
                                {message.status !== 'resolved' && message.status !== 'archived' && (
                                  <DropdownMenuItem onClick={() => handleResolve(message.id)}>
                                    <Eye className="mr-2 h-4 w-4" />
                                    {t('messages.resolve')}
                                  </DropdownMenuItem>
                                )}
                                <DropdownMenuSeparator />
                                <DropdownMenuItem
                                  onClick={() => handleDelete(message.id)}
                                  className="text-destructive"
                                >
                                  <Trash2 className="mr-2 h-4 w-4" />
                                  {t('delete')}
                                </DropdownMenuItem>
                              </DropdownMenuContent>
                            </DropdownMenu>
                          </TableCell>
                        </TableRow>
                      )
                    })
                  )}
                </TableBody>
              </Table>
            </div>
          </Card>
        </PageTabsContent>

        {/* Channels Tab */}
        <PageTabsContent value="channels" activeTab={activeTab}>
          {/* Channel count */}
          <div className="text-sm text-muted-foreground mb-4">
            {channels.filter(c => c.enabled).length} {t('enabled')} channels
          </div>

          <Card className="overflow-hidden">
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent border-b bg-muted/30">
                  <TableHead>
                    <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      <Bell className="h-4 w-4" />
                      {t('messages.channels.name')}
                    </div>
                  </TableHead>
                  <TableHead>
                    <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      {t('messages.channels.type')}
                    </div>
                  </TableHead>
                  <TableHead align="center">
                    <div className="flex items-center justify-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                      {t('status')}
                    </div>
                  </TableHead>
                  <TableHead className="w-12"></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {loading ? (
                  <TableRow>
                    <TableCell colSpan={4} className="py-8">
                      <div className="flex items-center justify-center gap-2">
                        <RefreshCw className="h-4 w-4 animate-spin" />
                        <span className="text-sm text-muted-foreground">{t('loading')}</span>
                      </div>
                    </TableCell>
                  </TableRow>
                ) : channels.length === 0 ? (
                  <EmptyStateInline
                    title={t('messages.channels.empty.title')}
                    colSpan={4}
                  />
                ) : (
                  channels.map((channel) => {
                    const config: Record<string, { icon: any; color: string }> = {
                      console: { icon: Bell, color: 'bg-gray-500/10 text-gray-500' },
                      memory: { icon: RefreshCw, color: 'bg-blue-500/10 text-blue-500' },
                      webhook: { icon: Megaphone, color: 'bg-green-500/10 text-green-500' },
                      email: { icon: Bell, color: 'bg-purple-500/10 text-purple-500' },
                    }
                    const channelConfig = config[channel.channel_type] || config.console
                    const ChannelIcon = channelConfig.icon

                    const testResult = testResults[channel.name]

                    return (
                      <TableRow key={channel.name} className="group transition-colors hover:bg-muted/50">
                        <TableCell>
                          <div className="flex items-center gap-3">
                            <div className={`w-9 h-9 rounded-lg flex items-center justify-center transition-colors ${channelConfig.color}`}>
                              <ChannelIcon className="h-4 w-4" />
                            </div>
                            <div className="flex-1">
                              <div className="flex items-center gap-2">
                                <span className="font-medium text-sm">{channel.name}</span>
                                {(channel.channel_type === 'webhook' || channel.channel_type === 'email') && (
                                  <Button
                                    variant="ghost"
                                    size="sm"
                                    className="h-6 px-2 text-xs"
                                    onClick={() => handleTestChannel(channel.name)}
                                    disabled={testingChannel === channel.name}
                                  >
                                    <TestTube className="h-3 w-3 mr-1" />
                                    {testingChannel === channel.name ? 'Testing...' : 'Test'}
                                  </Button>
                                )}
                              </div>
                              <div className="text-xs text-muted-foreground">{channel.channel_type}</div>
                              {testResult && (
                                <div className={`text-xs mt-1 ${testResult.success ? 'text-green-500' : 'text-red-500'}`}>
                                  {testResult.success ? '✓ ' : '✗ '}
                                  {testResult.message}
                                </div>
                              )}
                            </div>
                          </div>
                        </TableCell>
                        <TableCell>
                          <Badge variant="outline" className="text-xs">
                            {channel.channel_type}
                          </Badge>
                        </TableCell>
                        <TableCell align="center">
                          <Badge variant={channel.enabled ? 'default' : 'secondary'} className="text-xs">
                            {channel.enabled ? t('enabled') : t('disabled')}
                          </Badge>
                        </TableCell>
                        <TableCell>
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button variant="ghost" size="icon" className="h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity">
                                <MoreVertical className="h-4 w-4" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" className="w-40">
                              <DropdownMenuItem>
                                <Eye className="mr-2 h-4 w-4" />
                                {t('common.view')}
                              </DropdownMenuItem>
                              <DropdownMenuItem>
                                {channel.enabled ? (
                                  <>
                                    <RefreshCw className="mr-2 h-4 w-4" />
                                    {t('common.disable')}
                                  </>
                                ) : (
                                  <>
                                    <RefreshCw className="mr-2 h-4 w-4" />
                                    {t('common.enable')}
                                  </>
                                )}
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </TableCell>
                      </TableRow>
                    )
                  })
                )}
              </TableBody>
            </Table>
          </Card>
        </PageTabsContent>
      </PageTabs>

      <CreateMessageDialog
        open={createDialogOpen}
        onOpenChange={setCreateDialogOpen}
        onCreate={async (req) => {
          // Use the messages API to create a new message
          const response = await fetch(getApiUrl('/messages'), {
            method: 'POST',
            headers: {
              'Authorization': `Bearer ${localStorage.getItem('neomind_token') || sessionStorage.getItem('neomind_token_session') || ''}`,
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({
              category: req.category || 'alert',
              severity: req.severity || 'info',
              title: req.title,
              message: req.message,
              source: req.source || 'manual',
              source_type: req.source_type || 'ui',
              tags: req.tags || [],
            }),
          })
          if (response.ok) {
            await fetchMessages()
            toast({ title: t('messages.createSuccess', 'Message created') })
          } else {
            toast({ title: t('messages.createError', 'Failed to create'), variant: 'destructive' })
          }
        }}
      />
    </PageLayout>
  )
}

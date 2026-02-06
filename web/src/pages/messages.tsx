// Messages Page
// Unified notification/message system for NeoMind

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useLocation } from 'react-router-dom'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabs, PageTabsContent, EmptyStateInline, Pagination, ResponsiveTable } from '@/components/shared'
import { MessageSquare, Network } from 'lucide-react'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { confirm } from '@/hooks/use-confirm'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import type { NotificationMessage, MessageSeverity, MessageStatus, MessageCategory, MessageChannel } from '@/types'
import type { StandardError } from '@/lib/errors'

// Raw API response types
interface RawNotificationMessage {
  id?: string
  category?: string
  severity?: MessageSeverity
  title?: string
  message?: string
  source?: string
  source_type?: string
  timestamp?: string
  created_at?: string
  status?: MessageStatus
  tags?: string[]
  metadata?: Record<string, unknown>
}

interface MessagesApiResponse {
  messages?: RawNotificationMessage[]
  data?: {
    messages?: RawNotificationMessage[]
  } | RawNotificationMessage[]
}
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
  const { handleError } = useErrorHandler()
  const navigate = useNavigate()
  const location = useLocation()

  // Helper to get API base URL for Tauri environment
  const getApiUrl = (path: string) => {
    const apiBase = (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
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
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
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
  const [channels, setChannels] = useState<MessageChannel[]>([])

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
      const rawData: unknown = await response.json()

      // Handle different response formats from messages endpoint
      let messagesArray: RawNotificationMessage[] = []
      if (Array.isArray(rawData)) {
        messagesArray = rawData as RawNotificationMessage[]
      } else {
        // Try to extract messages from various response formats
        const apiResponse = rawData as MessagesApiResponse & { data?: RawNotificationMessage[] | { messages?: RawNotificationMessage[] } }
        if (apiResponse.messages && Array.isArray(apiResponse.messages)) {
          messagesArray = apiResponse.messages
        } else if (apiResponse.data) {
          if (Array.isArray(apiResponse.data)) {
            messagesArray = apiResponse.data
          } else if (apiResponse.data.messages && Array.isArray(apiResponse.data.messages)) {
            messagesArray = apiResponse.data.messages
          }
        }
      }

      // Convert to NotificationMessage format
      let messages: NotificationMessage[] = messagesArray.map((msg: RawNotificationMessage) => ({
        id: msg.id || '',
        category: (msg.category || 'alert') as MessageCategory,
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
      handleError(error, { operation: 'Fetch messages', showToast: false })
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
      handleError(error, { operation: 'Fetch channels', showToast: false })
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
      handleError(error, { operation: 'Acknowledge message', showToast: true })
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
      handleError(error, { operation: 'Resolve message', showToast: true })
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
      handleError(error, { operation: 'Archive message', showToast: true })
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
      handleError(error, { operation: 'Delete message', showToast: true })
      toast({ title: t('messages.deleteError', 'Failed to delete'), variant: 'destructive' })
    }
  }

  const tabs = [
    { value: 'messages' as TabValue, label: t('messages.tabs.messages'), icon: <MessageSquare className="h-4 w-4" /> },
    { value: 'channels' as TabValue, label: t('messages.tabs.channels'), icon: <Network className="h-4 w-4" /> },
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

          {/* Messages Table - Responsive (Desktop: Table, Mobile: Cards) */}
          <div className="overflow-auto h-full">
            <ResponsiveTable
              columns={[
                {
                  key: 'severity',
                  label: '',
                  width: 'w-[50px]',
                  className: 'text-center',
                },
                {
                  key: 'title',
                  label: t('messages.formTitle.label'),
                },
                {
                  key: 'message',
                  label: t('messages.content.label'),
                },
                {
                  key: 'severityBadge',
                  label: t('messages.severity.label'),
                  width: 'w-[90px]',
                  align: 'center',
                },
                {
                  key: 'category',
                  label: t('messages.category.label'),
                  width: 'w-[90px]',
                },
                {
                  key: 'status',
                  label: t('messages.status.label'),
                  width: 'w-[90px]',
                  align: 'center',
                },
                {
                  key: 'timestamp',
                  label: t('common.createdAt'),
                  width: 'w-[130px]',
                },
              ]}
              data={paginatedMessages as unknown as Record<string, unknown>[]}
              rowKey={(msg) => (msg as unknown as NotificationMessage).id}
              renderCell={(columnKey, rowData) => {
                const message = rowData as unknown as NotificationMessage
                const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                const categoryConfig = CATEGORY_CONFIG[message.category] || CATEGORY_CONFIG.system
                const statusConfig = STATUS_CONFIG[message.status] || STATUS_CONFIG.active
                const SeverityIcon = severityConfig.icon
                const CategoryIcon = categoryConfig.icon

                switch (columnKey) {
                  case 'severity':
                    return <SeverityIcon className={cn("h-4 w-4", severityConfig.color)} />
                  case 'title':
                    return (
                      <div className="font-medium text-sm truncate" title={message.title}>
                        {message.title}
                      </div>
                    )
                  case 'message':
                    return (
                      <div className="text-sm">
                        <div className="text-muted-foreground line-clamp-2">{message.message}</div>
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
                      </div>
                    )
                  case 'severityBadge':
                    return (
                      <Badge variant="outline" className={cn("text-xs", severityConfig.bgColor, severityConfig.color)}>
                        {t(`messages.severity.${message.severity}`)}
                      </Badge>
                    )
                  case 'category':
                    return (
                      <div className="flex items-center gap-1.5">
                        <CategoryIcon className="h-3.5 w-3.5 text-muted-foreground" />
                        <span className="text-xs">{t(categoryConfig.label)}</span>
                      </div>
                    )
                  case 'status':
                    return (
                      <Badge variant={statusConfig.variant} className="text-xs">
                        {t(statusConfig.label)}
                      </Badge>
                    )
                  case 'timestamp':
                    return (
                      <span className="text-xs text-muted-foreground">
                        {formatTimestamp(message.timestamp, false)}
                      </span>
                    )
                  default:
                    return null
                }
              }}
              getRowClassName={(rowData) => {
                const message = rowData as unknown as NotificationMessage
                return (message.status === 'resolved' || message.status === 'archived') ? 'opacity-60' : ''
              }}
              actions={[
                {
                  label: t('messages.acknowledge'),
                  icon: <Eye className="h-4 w-4" />,
                  show: (rowData) => (rowData as unknown as NotificationMessage).status === 'active',
                  onClick: (rowData) => {
                    const message = rowData as unknown as NotificationMessage
                    handleAcknowledge(message.id)
                  },
                },
                {
                  label: t('messages.resolve'),
                  icon: <Eye className="h-4 w-4" />,
                  show: (rowData) => {
                    const status = (rowData as unknown as NotificationMessage).status
                    return status !== 'resolved' && status !== 'archived'
                  },
                  onClick: (rowData) => {
                    const message = rowData as unknown as NotificationMessage
                    handleResolve(message.id)
                  },
                },
                {
                  label: t('delete'),
                  icon: <Trash2 className="h-4 w-4" />,
                  variant: 'destructive',
                  onClick: (rowData) => {
                    const message = rowData as unknown as NotificationMessage
                    handleDelete(message.id)
                  },
                },
              ]}
              loading={loading}
              emptyState={
                !loading && messages.length === 0 ? (
                  <EmptyStateInline
                    title={t('messages.empty.title')}
                  />
                ) : undefined
              }
            />
          </div>
        </PageTabsContent>

        {/* Channels Tab */}
        <PageTabsContent value="channels" activeTab={activeTab}>
          {/* Channel count */}
          <div className="text-sm text-muted-foreground mb-4">
            {channels.filter(c => c.enabled).length} {t('enabled')} channels
          </div>

          {/* Channels Responsive Table */}
          <ResponsiveTable
            columns={[
              {
                key: 'channel',
                label: (
                  <div className="flex items-center gap-2">
                    <Bell className="h-4 w-4" />
                    {t('messages.channels.name')}
                  </div>
                ),
              },
              {
                key: 'type',
                label: t('messages.channels.type'),
                align: 'center',
              },
              {
                key: 'status',
                label: t('status'),
                align: 'center',
              },
            ]}
            data={channels as unknown as Record<string, unknown>[]}
            rowKey={(ch) => (ch as unknown as MessageChannel).name}
            renderCell={(columnKey, rowData) => {
              const channel = rowData as unknown as MessageChannel
              const config: Record<string, { icon: typeof Bell; color: string }> = {
                console: { icon: Bell, color: 'bg-gray-500/10 text-gray-500' },
                memory: { icon: RefreshCw, color: 'bg-blue-500/10 text-blue-500' },
                webhook: { icon: Megaphone, color: 'bg-green-500/10 text-green-500' },
                email: { icon: Bell, color: 'bg-purple-500/10 text-purple-500' },
              }
              const channelConfig = config[channel.channel_type] || config.console
              const ChannelIcon = channelConfig.icon
              const testResult = testResults[channel.name]

              switch (columnKey) {
                case 'channel':
                  return (
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
                  )
                case 'type':
                  return (
                    <Badge variant="outline" className="text-xs">
                      {channel.channel_type}
                    </Badge>
                  )
                case 'status':
                  return (
                    <Badge variant={channel.enabled ? 'default' : 'secondary'} className="text-xs">
                      {channel.enabled ? t('enabled') : t('disabled')}
                    </Badge>
                  )
                default:
                  return null
              }
            }}
            actions={[
              {
                label: t('common.view'),
                icon: <Eye className="h-4 w-4" />,
                onClick: () => {
                  // Handle view
                },
              },
              {
                label: t('common.enable'),
                icon: <RefreshCw className="h-4 w-4" />,
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  // Handle enable/disable
                },
              },
            ]}
            loading={loading}
            emptyState={
              !loading && channels.length === 0 ? (
                <EmptyStateInline title={t('messages.channels.empty.title')} />
              ) : undefined
            }
          />
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

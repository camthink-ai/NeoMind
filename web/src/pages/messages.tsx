// Messages Page
// Unified notification/message system for NeoMind

import { useState, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useLocation } from 'react-router-dom'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabsBar, PageTabsContent, Pagination, ResponsiveTable, EmptyState, EmptyStateCard } from '@/components/shared'
import { MessageSquare, Network, Settings, Filter as FilterIcon, Loader2 } from 'lucide-react'
import { api } from '@/lib/api'
import { useStore } from '@/store'
import { useDataVersion } from '@/hooks/useDataVersion'
import { useLiveMessageRefresh } from '@/hooks/useLiveMessageRefresh'
import { useToast } from '@/hooks/use-toast'
import { Flag } from 'lucide-react'
import { confirm } from '@/hooks/use-confirm'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { useIsMobile } from '@/hooks/useMobile'
import type { NotificationMessage, MessageSeverity, MessageStatus, MessageChannel, ChannelFilter } from '@/types'
import type { MessageChain } from '@/lib/api/onboarding'

import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { SelectableChip } from '@/components/ui/selectable-chip'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Checkbox } from '@/components/ui/checkbox'
import {






} from '@/components/ui/table'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Label } from '@/components/ui/label'
import { FormSection } from '@/components/ui/form-section'
import { Input } from '@/components/ui/input'
import { FormField } from '@/components/ui/field'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import {
  AlertCircle,
  Info,
  AlertTriangle,
  ShieldAlert,
  Trash2,
  Bell,
  Megaphone,
  Eye,
  RefreshCw,

  X,
  CheckCircle2,
  TestTube,
  Mail,
  UserPlus,
  Check,
  Tag,
  Plus,
  Send,
  MessageCircle,
  Hash,
  MessagesSquare,
  Radio,
} from 'lucide-react'
import { CreateMessageDialog } from '@/components/messages/CreateMessageDialog'
import { ChannelEditorDialog } from '@/components/messages/ChannelEditorDialog'
import { ImSessionsTab } from '@/components/im/ImSessionsTab'
import { formatTimestamp } from '@/lib/utils/format'
import { cn } from '@/lib/utils'
import { textNano, textMini } from "@/design-system/tokens/typography"

type TabValue = 'messages' | 'channels' | 'im'

// Get tab from URL path
const getTabFromPath = (pathname: string): TabValue => {
  const pathSegments = pathname.split('/')
  const lastSegment = pathSegments[pathSegments.length - 1]
  if (lastSegment === 'channels') {
    return 'channels'
  }
  if (lastSegment === 'im') {
    return 'im'
  }
  return 'messages'
}

// Severity config
const SEVERITY_CONFIG: Record<string, { icon: typeof Info; color: string; bgColor: string }> = {
  info: { icon: Info, color: 'text-info', bgColor: 'bg-info-light border-info' },
  warning: { icon: AlertTriangle, color: 'text-warning', bgColor: 'bg-warning-light border-warning' },
  critical: { icon: AlertCircle, color: 'text-error', bgColor: 'bg-error-light border-error' },
  emergency: { icon: ShieldAlert, color: 'text-error', bgColor: 'bg-error-light border-error' },
}

// Status config
const STATUS_CONFIG: Record<string, { variant: 'default' | 'secondary' | 'outline'; label: string }> = {
  active: { variant: 'default', label: 'messages.status.active' },
  acknowledged: { variant: 'secondary', label: 'messages.status.acknowledged' },
  resolved: { variant: 'outline', label: 'messages.status.resolved' },
  archived: { variant: 'outline', label: 'messages.status.archived' },
  false_positive: { variant: 'outline', label: 'messages.status.false_positive' },
}

// Category config - supports any category from backend with fallback
const CATEGORY_CONFIG: Record<string, { label: string; icon: typeof Bell }> = {
  alert: { label: 'messages.category.alert', icon: AlertCircle },
  system: { label: 'messages.category.system', icon: Bell },
  business: { label: 'messages.category.business', icon: Megaphone },
  notification: { label: 'messages.category.notification', icon: Bell },
}

// Helper to get category config with fallback for unknown categories
const getCategoryConfig = (category: string) => {
  return CATEGORY_CONFIG[category] || {
    label: category, // Use raw category name as label for unknown categories
    icon: Bell,
  }
}

export default function MessagesPage() {
  const { t } = useTranslation()
  const openSettings = useStore((s) => s.openSettings)
  const { handleError } = useErrorHandler()
  const navigate = useNavigate()
  const location = useLocation()
  const isMobile = useIsMobile()

  // Tab state - sync with URL
  const [activeTab, setActiveTab] = useState<TabValue>(() => getTabFromPath(location.pathname))

  // Messages state
  const [messages, setMessages] = useState<NotificationMessage[]>([])
  const [totalCount, setTotalCount] = useState(0)
  const [loading, setLoading] = useState(false)

  // Pagination - server-side
  const [messagePage, setMessagePage] = useState(1)
  const messagesPerPage = 10
  // Mobile: track loaded page count for cumulative append
  const [_mobileLoadedPages, setMobileLoadedPages] = useState(1)

  // Filters — single-select per dimension. The server API takes ONE value
  // per dimension (ListMessagesQuery has scalar severity/status/category),
  // so every active filter maps to a server param and totals/pagination
  // stay correct. The former multi-select filtered only the current page
  // window client-side — matched rows on later pages were unreachable and
  // the total stayed stale. Multi-select needs a repeated-params server
  // contract; revisit there, not here.
  const [selectedSeverities, setSelectedSeverities] = useState<Set<MessageSeverity>>(new Set())
  const [selectedStatuses, setSelectedStatuses] = useState<Set<MessageStatus>>(new Set())
  const [selectedCategories, setSelectedCategories] = useState<Set<string>>(new Set())

  // Dynamic categories from API — unioned across fetched pages, so the
  // option list doesn't reshape itself every time the user turns a page.
  const [availableCategories, setAvailableCategories] = useState<string[]>([])
  const seenCategoriesRef = useRef<Set<string>>(new Set())

  // Check if any filters are active
  const hasActiveFilters = selectedSeverities.size > 0 || selectedStatuses.size > 0 || selectedCategories.size > 0

  // Clear all filters
  const clearAllFilters = () => {
    setSelectedSeverities(new Set())
    setSelectedStatuses(new Set())
    setSelectedCategories(new Set())
  }

  // Toggle filter — single-select: picking a value replaces the dimension's
  // previous pick; picking the active value clears it.
  const toggleSeverity = (severity: MessageSeverity) => {
    setSelectedSeverities(selectedSeverities.has(severity) ? new Set() : new Set([severity]))
  }

  const toggleStatus = (status: MessageStatus) => {
    setSelectedStatuses(selectedStatuses.has(status) ? new Set() : new Set([status]))
  }

  const toggleCategory = (category: string) => {
    setSelectedCategories(selectedCategories.has(category) ? new Set() : new Set([category]))
  }

  // Get active filter count
  const getActiveFilterCount = () => selectedSeverities.size + selectedStatuses.size + selectedCategories.size

  // Test channel state
  const [testingChannel, setTestingChannel] = useState<string | null>(null)
  const [testResults, setTestResults] = useState<Record<string, { success: boolean; message: string }>>({})

  // View channel dialog state
  const [viewChannel, setViewChannel] = useState<MessageChannel | null>(null)

  // Channel filter configuration state
  const [filterDialogChannel, setFilterDialogChannel] = useState<MessageChannel | null>(null)
  const [filterConfig, setFilterConfig] = useState<ChannelFilter>({
    source_types: [],
    categories: [],
    min_severity: null,
  })
  const [savingFilter, setSavingFilter] = useState(false)

  // Handle opening filter dialog
  const handleOpenFilterDialog = useCallback(async (channel: MessageChannel) => {
    setFilterDialogChannel(channel)
    try {
      const filter = await api.getChannelFilter(channel.name)
      setFilterConfig({
        source_types: filter.source_types || [],
        categories: filter.categories || [],
        min_severity: filter.min_severity || null,
      })
    } catch {
      // Use default filter on error
      setFilterConfig({
        source_types: [],
        categories: [],
        min_severity: null,
      })
    }
  }, [api])

  // Handle saving filter configuration
  const handleSaveFilter = async () => {
    if (!filterDialogChannel) return
    setSavingFilter(true)
    try {
      await api.updateChannelFilter(filterDialogChannel.name, filterConfig)
      setFilterDialogChannel(null)
      toast({
        title: t('common:success'),
        description: t('common:messages.channels.filterSaved', 'Filter configuration saved'),
      })
    } catch (error) {
      handleError(error, { operation: 'Save filter' })
    } finally {
      setSavingFilter(false)
    }
  }

  // Recipients management dialog state
  const [recipientsDialogChannel, setRecipientsDialogChannel] = useState<MessageChannel | null>(null)
  const [recipients, setRecipients] = useState<string[]>([])
  const [newRecipientEmail, setNewRecipientEmail] = useState('')
  const [loadingRecipients, setLoadingRecipients] = useState(false)
  const [addingRecipient, setAddingRecipient] = useState(false)
  const [recipientError, setRecipientError] = useState<string | null>(null)

  // Fetch recipients for a channel
  const fetchRecipients = async (channelName: string) => {
    setLoadingRecipients(true)
    try {
      const result = await api.listChannelRecipients(channelName)
      setRecipients(result.recipients || [])
    } catch (error) {
      handleError(error, { operation: 'Fetch recipients' })
    } finally {
      setLoadingRecipients(false)
    }
  }

  // Open recipients dialog
  const handleManageRecipients = async (channel: MessageChannel) => {
    setRecipientsDialogChannel(channel)
    setNewRecipientEmail('')
    await fetchRecipients(channel.name)
  }

  // Add recipient
  const handleAddRecipient = async () => {
    if (!recipientsDialogChannel || !newRecipientEmail.trim()) return

    setAddingRecipient(true)
    setRecipientError(null)
    try {
      const result = await api.addChannelRecipient(recipientsDialogChannel.name, newRecipientEmail.trim())
      setRecipients(result.recipients || [])
      setNewRecipientEmail('')
      toast({
        title: t('success'),
        description: t('messages.channels.recipientAdded', 'Recipient added successfully'),
      })
      fetchChannels()
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      setRecipientError(message)
    } finally {
      setAddingRecipient(false)
    }
  }

  // Remove recipient
  const handleRemoveRecipient = async (email: string) => {
    if (!recipientsDialogChannel) return

    try {
      const result = await api.removeChannelRecipient(recipientsDialogChannel.name, email)
      setRecipients(result.recipients || [])
      toast({
        title: t('success'),
        description: t('messages.channels.recipientRemoved', 'Recipient removed successfully'),
      })
      fetchChannels()
    } catch (error) {
      handleError(error, { operation: 'Remove recipient' })
    }
  }

  // Test a channel
  const handleTestChannel = async (channelName: string) => {
    setTestingChannel(channelName)
    try {
      const result = await api.testMessageChannel(channelName)
      if (result.success) {
        setTestResults(prev => ({ ...prev, [channelName]: { success: true, message: result.message || 'Test sent successfully' } }))
        toast({ title: t('success'), description: `Channel "${channelName}" test successful` })
      } else {
        throw new Error(result.message || 'Test failed')
      }
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
      setTestResults(prev => ({ ...prev, [channelName]: { success: false, message: errorMsg } }))
      toast({ title: t('failed'), description: errorMsg, variant: 'destructive' })
    } finally {
      setTestingChannel(null)
    }
  }

  // Toggle channel enabled state
  const handleToggleEnabled = async (channelName: string, enabled: boolean) => {
    try {
      await api.updateChannelEnabled(channelName, enabled)
      setChannels(prev => prev.map(c =>
        c.name === channelName ? { ...c, enabled } : c
      ))
      toast({
        title: t('success'),
        description: enabled
          ? t('messages.channels.enableSuccess', 'Channel enabled')
          : t('messages.channels.disableSuccess', 'Channel disabled')
      })
    } catch (error) {
      handleError(error, { operation: 'Toggle channel', showToast: true })
    }
  }

  // View channel details
  const handleViewChannel = async (channelName: string) => {
    const channel = channels.find(c => c.name === channelName)
    if (channel) {
      setViewChannel(channel)
    }
  }

  // Delete channel
  const handleDeleteChannel = async (channelName: string) => {
    const confirmed = await confirm({
      title: t('messages.channels.deleteChannel', 'Delete Channel'),
      description: t('messages.channels.confirmDelete', 'Are you sure you want to delete the channel "{{name}}"?', { name: channelName }),
      variant: 'destructive',
    })
    if (!confirmed) return

    try {
      await api.deleteMessageChannel(channelName)
      toast({
        title: t('success'),
        description: t('messages.channels.deleteSuccess', 'Channel deleted successfully'),
      })
      fetchChannels()
    } catch (error) {
      handleError(error, { operation: 'Delete channel', showToast: true })
    }
  }

  // Open edit channel dialog
  const handleEditChannel = (channel: MessageChannel) => {
    setEditingChannel(channel)
    setChannelEditorOpen(true)
  }

  // Server-side paginated — messages is already the current page
  const filteredCount = totalCount

  // Channels state
  const [channels, setChannels] = useState<MessageChannel[]>([])

  // Dialogs
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [selectedMessage, setSelectedMessage] = useState<NotificationMessage | null>(null)
  // Why the selected alert fired (M2-5). Fetched on open — it is the question
  // the user opened the alert with. Failure is silent: the section simply does
  // not render and the message itself stays fully usable.
  const [messageChain, setMessageChain] = useState<MessageChain | null>(null)
  const [channelEditorOpen, setChannelEditorOpen] = useState(false)
  const [editingChannel, setEditingChannel] = useState<MessageChannel | null>(null)

  const { toast } = useToast()

  // Update tab when URL changes
  useEffect(() => {
    setActiveTab(getTabFromPath(location.pathname))
  }, [location.pathname])

  useEffect(() => {
    if (!selectedMessage) {
      setMessageChain(null)
      return
    }
    let cancelled = false
    setMessageChain(null)
    api
      .getMessageChain(selectedMessage.id)
      .then(chain => {
        if (!cancelled) setMessageChain(chain)
      })
      .catch(() => {
        // An alert we cannot explain is still an alert; do not block it.
        if (!cancelled) setMessageChain(null)
      })
    return () => {
      cancelled = true
    }
  }, [selectedMessage])

  // Update URL when tab changes
  const handleTabChange = (tab: TabValue) => {
    setActiveTab(tab)
    if (tab === 'channels') {
      navigate('/messages/channels')
    } else if (tab === 'im') {
      navigate('/messages/im')
    } else {
      navigate('/messages')
    }
  }

  // Fetch messages using the unified messages API
  const fetchMessages = useCallback(async () => {
    setLoading(true)
    try {
      // Build query params for server-side filtering and pagination
      // Server-side pagination — only fetch current page
      const offset = (messagePage - 1) * messagesPerPage
      const params: Record<string, string> = {
        limit: String(messagesPerPage),
        offset: String(offset),
      }
      if (selectedSeverities.size === 1) {
        params.severity = [...selectedSeverities][0]
      }
      if (selectedStatuses.size === 1) {
        params.status = [...selectedStatuses][0]
      }
      if (selectedCategories.size === 1) {
        params.category = [...selectedCategories][0]
      }

      // Fetch messages
      const messagesResponse = await api.getMessages(params)

      const messagesArray: NotificationMessage[] = messagesResponse.messages || []
      const serverTotal = messagesResponse.total ?? 0

      // Extract categories — union across every fetch so the option list
      // is stable across pagination instead of reshaping per page.
      for (const m of messagesArray) {
        if (m.category) seenCategoriesRef.current.add(m.category)
      }
      setAvailableCategories([...seenCategoriesRef.current].sort())

      // Single-select filters are fully server-side (params above) — the
      // response IS the filtered page; totals and pagination are exact.

      // Sort by timestamp descending
      messagesArray.sort((a: NotificationMessage, b: NotificationMessage) => {
        const aTime = new Date(a.timestamp).getTime()
        const bTime = new Date(b.timestamp).getTime()
        if (isNaN(aTime)) return 1
        if (isNaN(bTime)) return -1
        return bTime - aTime
      })

      if (isMobile && messagePage > 1) {
        // Mobile: accumulate data for infinite scroll
        setMessages(prev => {
          const existingIds = new Set(prev.map(m => m.id))
          const unique = messagesArray.filter(m => !existingIds.has(m.id))
          return [...prev, ...unique]
        })
      } else {
        setMessages(messagesArray)
      }
      setMobileLoadedPages(messagePage)
      setTotalCount(serverTotal)
    } catch (error) {
      handleError(error, { operation: 'Fetch messages', showToast: false })
    } finally {
      setLoading(false)
    }
  }, [messagePage, selectedSeverities, selectedStatuses, selectedCategories, messagesPerPage])

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

  // Reset page when filters change (page change is handled by fetchMessages dependency)
  useEffect(() => {
    setMessagePage(1)
    setMobileLoadedPages(1)
  }, [selectedSeverities, selectedStatuses, selectedCategories])

  // Fetch on mount and when page/filters change
  const dataVersion = useDataVersion('messages', 'message-channels', 'im-bridges')
  useEffect(() => {
    if (activeTab === 'messages') {
      fetchMessages()
    } else if (activeTab === 'channels') {
      fetchChannels()
    }
    // 'im' tab manages its own data lifecycle via ImSessionsTab
  }, [activeTab, fetchMessages, fetchChannels, dataVersion])

  // Live updates: the backend publishes MessageCreated on the event bus and
  // the WS bridge forwards it, so new messages appear without Refresh. The
  // coalesced refetch reuses fetchMessages, so the user's filters, page and
  // scroll state survive the refresh.
  useLiveMessageRefresh(activeTab === 'messages', fetchMessages)

  // Message actions - using messages API endpoints
  const handleAcknowledge = async (id: string) => {
    try {
      await api.acknowledgeMessage(id)
      setMessages(prev => prev.map(m =>
        m.id === id ? { ...m, status: 'acknowledged' as MessageStatus, acknowledged: true } : m
      ))
      toast({ title: t('messages.acknowledgeSuccess', 'Acknowledged') })
    } catch (error) {
      handleError(error, { operation: 'Acknowledge message', showToast: true })
      toast({ title: t('messages.acknowledgeError', 'Failed'), variant: 'destructive' })
    }
  }

  const handleResolve = async (id: string) => {
    try {
      await api.resolveMessage(id)
      setMessages(prev => prev.map(m =>
        m.id === id ? { ...m, status: 'resolved' as MessageStatus, acknowledged: true } : m
      ))
      toast({ title: t('messages.resolveSuccess', 'Resolved') })
    } catch (error) {
      handleError(error, { operation: 'Resolve message', showToast: true })
      toast({ title: t('messages.resolveError', 'Failed'), variant: 'destructive' })
    }
  }

  /**
   * Dismiss an alert as a false positive (002 §3.4) — the human half of the
   * feedback loop. The verdict is the operator's; nothing here touches the rule
   * that fired, and the count that may later suggest a threshold change is read
   * back through the chain.
   */
  const handleFalsePositive = async (id: string) => {
    try {
      await api.markMessageFalsePositive(id)
      setMessages(prev => prev.map(m =>
        m.id === id ? { ...m, status: 'false_positive' as MessageStatus } : m
      ))
      toast({ title: t('messages.falsePositiveSuccess', 'Marked as a false positive') })
    } catch (error) {
      handleError(error, { operation: 'Mark message as false positive', showToast: true })
      toast({ title: t('messages.falsePositiveError', 'Failed'), variant: 'destructive' })
    }
  }

  const _handleArchive = async (id: string) => {
    try {
      await api.archiveMessage(id)
      setMessages(prev => prev.map(m =>
        m.id === id ? { ...m, status: 'archived' as MessageStatus } : m
      ))
      toast({ title: t('messages.archiveSuccess', 'Archived') })
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
      await api.deleteMessage(id)
      setMessages(prev => prev.filter(m => m.id !== id))
      toast({ title: t('messages.deleteSuccess', 'Deleted') })
    } catch (error) {
      handleError(error, { operation: 'Delete message', showToast: true })
      toast({ title: t('messages.deleteError', 'Failed to delete'), variant: 'destructive' })
    }
  }

  const tabs = [
    { value: 'messages' as TabValue, label: t('messages.tabs.messages'), icon: <MessageSquare className="h-4 w-4" /> },
    { value: 'channels' as TabValue, label: t('messages.tabs.channels'), icon: <Network className="h-4 w-4" /> },
    { value: 'im' as TabValue, label: t('messages.tabs.im', 'IM Sessions'), icon: <Send className="h-4 w-4" /> },
  ]

  const actions = [
    ...(activeTab === 'messages' ? [
      { label: t('messages.create'), onClick: () => setCreateDialogOpen(true) },
    ] : []),
    ...(activeTab === 'channels' ? [
      { label: t('messages.channels.create', 'Add Channel'), icon: <Plus className="h-4 w-4" />, onClick: () => { setEditingChannel(null); setChannelEditorOpen(true) } },
    ] : []),
    ...(activeTab === 'im' ? [
      { label: t('messages.im.goToSettings', 'Configure IM bridge'), icon: <Settings className="h-4 w-4" />, onClick: () => openSettings('im') },
    ] : []),
    ...(activeTab !== 'im' ? [{
      label: t('refresh'),
      variant: 'outline' as const,
      onClick: activeTab === 'messages' ? fetchMessages : fetchChannels,
      disabled: loading,
    }] : []),
  ]

  // Filter dropdown for actionsExtra
  const filterExtra = activeTab === 'messages' ? (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="gap-2">
          <FilterIcon className="h-4 w-4" />
          {t('messages.filter.title')}
          {getActiveFilterCount() > 0 && (
            <Badge variant="secondary" className="h-5 px-1.5 text-xs">
              {getActiveFilterCount()}
            </Badge>
          )}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[min(340px,calc(100vw-2rem))] p-0 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b bg-muted-30">
          <span className="text-sm font-semibold">{t('messages.filter.title')}</span>
          {hasActiveFilters && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs text-muted-foreground hover:text-foreground px-2 sm:h-7"
              onClick={clearAllFilters}
            >
              <X className="h-3 w-3 mr-1" />
              {t('messages.filter.clear')}
            </Button>
          )}
        </div>

        <div className="px-4 py-3 space-y-4 max-h-[60vh] overflow-y-auto">
          {/* Severity */}
          <div>
            <div className="flex items-center gap-2 mb-2">
              <AlertTriangle className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('messages.severity.label')}</span>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {(['emergency', 'critical', 'warning', 'info'] as MessageSeverity[]).map((sev) => {
                const icons = {
                  emergency: <ShieldAlert className="h-3.5 w-3.5" />,
                  critical: <AlertCircle className="h-3.5 w-3.5" />,
                  warning: <AlertTriangle className="h-3.5 w-3.5" />,
                  info: <Info className="h-3.5 w-3.5" />,
                }
                const colors = {
                  emergency: "text-error bg-error-light border-error",
                  critical: "text-error bg-error-light border-error",
                  warning: "text-warning bg-warning-light border-warning",
                  info: "text-info bg-info-light border-info",
                }
                return (
                  <button
                    key={sev}
                    onClick={() => toggleSeverity(sev)}
                    className={cn(
                      "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border text-xs font-medium transition-all",
                      selectedSeverities.has(sev)
                        ? colors[sev]
                        : "border-border text-muted-foreground hover:bg-muted-50 hover:text-foreground"
                    )}
                  >
                    {icons[sev]}
                    {t(`messages.severity.${sev}`)}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Status */}
          <div>
            <div className="flex items-center gap-2 mb-2">
              <CheckCircle2 className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('messages.status.label')}</span>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {(['active', 'acknowledged', 'resolved', 'archived', 'false_positive'] as MessageStatus[]).map((stat) => {
                const colors = {
                  active: "text-info bg-info-light border-info",
                  acknowledged: "text-warning bg-warning-light border-warning",
                  resolved: "text-success bg-success-light border-success",
                  archived: "text-muted-foreground bg-muted border-border",
                  // Quiet: a dismissed alert is not something to act on. It is
                  // filterable so the dismissals can be reviewed.
                  false_positive: "text-muted-foreground bg-muted border-border",
                }
                return (
                  <button
                    key={stat}
                    onClick={() => toggleStatus(stat)}
                    className={cn(
                      "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border text-xs font-medium transition-all",
                      selectedStatuses.has(stat)
                        ? colors[stat]
                        : "border-border text-muted-foreground hover:bg-muted-50 hover:text-foreground"
                    )}
                  >
                    {selectedStatuses.has(stat) && <Check className="h-3 w-3" />}
                    {t(`messages.status.${stat}`)}
                  </button>
                )
              })}
            </div>
          </div>

          {/* Category */}
          {availableCategories.length > 0 && (
            <div>
              <div className="flex items-center gap-2 mb-2">
                <Tag className="h-3.5 w-3.5 text-muted-foreground" />
                <span className="text-xs font-medium text-muted-foreground uppercase tracking-wide">{t('messages.category.label')}</span>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {availableCategories.map((cat) => {
                  const config = getCategoryConfig(cat)
                  const Icon = config.icon
                  return (
                    <button
                      key={cat}
                      onClick={() => toggleCategory(cat)}
                      className={cn(
                        "inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border text-xs font-medium transition-all",
                        selectedCategories.has(cat)
                          ? "bg-primary-light text-primary border-primary"
                          : "border-border text-muted-foreground hover:bg-muted-50 hover:text-foreground"
                      )}
                    >
                      <Icon className="h-3.5 w-3.5" />
                      {t(config.label)}
                    </button>
                  )
                })}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-4 py-2.5 border-t bg-muted-30">
          <p className="text-xs text-muted-foreground">
            {hasActiveFilters
              ? t('messages.filter.showing', { count: filteredCount })
              : t('messages.filter.allVisible')
            }
          </p>
        </div>
      </PopoverContent>
    </Popover>
  ) : null

  return (
    <>
      <PageLayout
        title={t('messages.title')}
        hideFooterOnMobile
        headerContent={
          <PageTabsBar
            tabs={tabs}
            activeTab={activeTab}
            onTabChange={(v) => handleTabChange(v as TabValue)}
            actions={actions}
            actionsExtra={isMobile ? undefined : filterExtra}
          />
        }
        footer={
          activeTab === 'messages' && totalCount > messagesPerPage ? (
            <Pagination
              total={totalCount}
              pageSize={messagesPerPage}
              currentPage={messagePage}
              onPageChange={setMessagePage}
              isLoading={loading}
            />
          ) : undefined
        }
      >
        {/* Messages Tab */}
        <PageTabsContent value="messages" activeTab={activeTab} className="flex flex-col">
          {/* Active Filter Chips */}
          {hasActiveFilters && (
            <div className="flex flex-wrap items-center gap-2 mb-4 shrink-0">
              <span className="text-sm text-muted-foreground mr-1">
                {t('messages.filter.activeCount', { count: getActiveFilterCount() })}:
              </span>

              {Array.from(selectedSeverities).map((sev) => (
                <Badge
                  key={`sev-${sev}`}
                  variant="secondary"
                  className="gap-1 pr-1 cursor-pointer hover:bg-secondary-hover"
                  onClick={() => toggleSeverity(sev)}
                >
                  {sev === 'info' && <Info className="h-4 w-4 text-info" />}
                  {sev === 'warning' && <AlertTriangle className="h-4 w-4 text-warning" />}
                  {sev === 'critical' && <AlertCircle className="h-4 w-4 text-error" />}
                  {sev === 'emergency' && <ShieldAlert className="h-4 w-4 text-error" />}
                  {t(`messages.severity.${sev}`)}
                  <X className="h-4 w-4 ml-1 text-muted-foreground" />
                </Badge>
              ))}

              {Array.from(selectedStatuses).map((stat) => (
                <Badge
                  key={`stat-${stat}`}
                  variant="secondary"
                  className="gap-1 pr-1 cursor-pointer hover:bg-secondary-hover"
                  onClick={() => toggleStatus(stat)}
                >
                  {t(`messages.status.${stat}`)}
                  <X className="h-4 w-4 ml-1 text-muted-foreground" />
                </Badge>
              ))}

              {Array.from(selectedCategories).map((cat) => {
                const config = getCategoryConfig(cat)
                const Icon = config.icon
                return (
                  <Badge
                    key={`cat-${cat}`}
                    variant="secondary"
                    className="gap-1 pr-1 cursor-pointer hover:bg-secondary-hover"
                    onClick={() => toggleCategory(cat)}
                  >
                    <Icon className="h-4 w-4" />
                    {t(config.label)}
                    <X className="h-4 w-4 ml-1 text-muted-foreground" />
                  </Badge>
                )
              })}

              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs sm:h-7"
                onClick={clearAllFilters}
              >
                {t('messages.filter.clearAll')}
              </Button>
            </div>
          )}

          {/* Messages Table - Responsive (Desktop: Table, Mobile: Cards) */}
          {isMobile ? (
            <div className="space-y-2">
              {messages.length === 0 && loading ? (
                Array.from({ length: 4 }).map((_, i) => (
                  <div key={i} className="rounded-2xl border border-border bg-card p-3.5">
                    <div className="space-y-2">
                      <div className="flex items-center gap-2">
                        <div className="flex-1 space-y-1">
                          <div className="h-4 bg-muted rounded w-2/3 animate-pulse" />
                          <div className="h-3 bg-muted rounded w-1/3 animate-pulse" />
                        </div>
                      </div>
                    </div>
                  </div>
                ))
              ) : messages.length === 0 ? (
                <EmptyStateCard>
                  <EmptyState
                    title={t('messages.empty.title')}
                    description={t('messages.empty.description')}
                  />
                </EmptyStateCard>
              ) : (
                messages.map((message) => {
                  const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                  const statusConfig = STATUS_CONFIG[message.status] || STATUS_CONFIG.active
                  const isDimmed = message.status === 'resolved' || message.status === 'archived'

                  return (
                    <button
                      type="button"
                      key={message.id}
                      className={cn(
                        "block w-full rounded-2xl border border-border bg-card px-4 py-3 text-left transition-[background-color,opacity] active:bg-muted",
                        isDimmed && "opacity-50"
                      )}
                      onClick={() => setSelectedMessage(message)}
                    >
                      <div className="flex items-center gap-3">
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center gap-2">
                            <span className="min-w-0 flex-1 truncate text-sm font-medium">{message.title}</span>
                            <span className={cn(textNano, "shrink-0 text-muted-foreground")}>{t(statusConfig.label)}</span>
                          </div>
                          <div className={cn(textMini, "mt-1 flex min-w-0 items-center gap-1.5 text-muted-foreground")}>
                            <span className={severityConfig.color}>{t(`messages.severity.${message.severity}`)}</span>
                            <span aria-hidden="true">·</span>
                            <span className="truncate">{message.category}</span>
                            <span aria-hidden="true">·</span>
                            <span className="shrink-0">{formatTimestamp(message.timestamp, false)}</span>
                          </div>
                        </div>
                      </div>
                    </button>
                  )
                })
              )}
              {/* Loading more indicator - only when appending data */}
              {messages.length > 0 && loading && (
                <div className="flex items-center justify-center py-4">
                  <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
                </div>
              )}
            </div>
          ) : (
          <ResponsiveTable
            columns={[
                {
                  key: 'title',
                  label: t('messages.formTitle.label'),
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
                  label: t('common:createdAt'),
                  width: 'w-[150px]',
                },
              ]}
              data={messages}
              rowKey={(msg: NotificationMessage) => msg.id}
              renderCell={(columnKey, rowData) => {
                const message = rowData
                const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                const categoryConfig = getCategoryConfig(message.category)
                const statusConfig = STATUS_CONFIG[message.status] || STATUS_CONFIG.active
                const CategoryIcon = categoryConfig.icon

                switch (columnKey) {
                  case 'title':
                    // Title + message preview + tags in one cell — the two
                    // columns were a truncated echo of each other.
                    return (
                      <div className="text-sm min-w-0">
                        <div className="font-medium truncate" title={message.title}>
                          {message.title}
                        </div>
                        <div className="text-muted-foreground line-clamp-1 text-xs mt-0.5">{message.message}</div>
                        {(message.tags || []).length > 0 && (
                          <div className="flex items-center gap-1 mt-1 flex-wrap">
                            {(message.tags || []).slice(0, 4).map((tag, i) => (
                              <Badge key={i} variant="outline" className="text-xs font-normal h-5 px-1.5 bg-muted-20 border-glass-border text-muted-foreground">
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
                      <Badge variant="outline" className={cn("text-xs font-normal border-transparent", severityConfig.bgColor, severityConfig.color)}>
                        {t(`messages.severity.${message.severity}`)}
                      </Badge>
                    )
                  case 'category':
                    return (
                      <div className="flex items-center gap-1.5 text-muted-foreground">
                        <CategoryIcon className="h-3.5 w-3.5 shrink-0" />
                        <span className="text-xs">{t(categoryConfig.label)}</span>
                      </div>
                    )
                  case 'status':
                    return (
                      <Badge variant={statusConfig.variant} className="text-xs font-normal">
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
                const message = rowData
                return (message.status === 'resolved' || message.status === 'archived') ? 'opacity-60' : ''
              }}
              onRowClick={(rowData) => {
                // Quick-open detail dialog on row click — same as "View Details"
                // action in the dropdown, just one tap instead of two.
                setSelectedMessage(rowData)
              }}
              actions={[
                {
                  label: t('messages.viewDetails', 'View Details'),
                  icon: <Eye className="h-4 w-4" />,
                  onClick: (rowData) => {
                    const message = rowData
                    setSelectedMessage(message)
                  },
                },
                {
                  label: t('messages.acknowledge'),
                  icon: <Check className="h-4 w-4" />,
                  show: (rowData: NotificationMessage) => rowData.status === 'active',
                  onClick: (rowData) => {
                    const message = rowData
                    handleAcknowledge(message.id)
                  },
                },
                {
                  label: t('messages.resolve'),
                  icon: <CheckCircle2 className="h-4 w-4" />,
                  show: (rowData) => {
                    const status = rowData.status
                    return status !== 'resolved' && status !== 'archived'
                  },
                  onClick: (rowData) => {
                    const message = rowData
                    handleResolve(message.id)
                  },
                },
                {
                  label: t('delete'),
                  icon: <Trash2 className="h-4 w-4" />,
                  variant: 'destructive',
                  onClick: (rowData) => {
                    const message = rowData
                    handleDelete(message.id)
                  },
                },
              ]}
              loading={loading}
              emptyState={
                !loading && messages.length === 0 ? (
                  <EmptyState
                    title={t('messages.empty.title')}
                    description={t('messages.empty.description')}
                  />
                ) : undefined
              }
            />
          )}
        </PageTabsContent>

        {/* Channels Tab */}
        <PageTabsContent value="channels" activeTab={activeTab}>
          {/* Channels Responsive Table */}
          {isMobile ? (
            <div className="space-y-2">
              {loading ? (
                Array.from({ length: 3 }).map((_, i) => (
                  <Card key={i} className="overflow-hidden rounded-none border-0 shadow-none">
                    <div className="p-3 space-y-2">
                      <div className="flex items-center gap-2">
                        <div className="w-8 h-8 rounded-lg bg-muted animate-pulse" />
                        <div className="flex-1 space-y-1">
                          <div className="h-4 bg-muted rounded w-2/3 animate-pulse" />
                          <div className="h-3 bg-muted rounded w-1/3 animate-pulse" />
                        </div>
                      </div>
                    </div>
                  </Card>
                ))
              ) : channels.length === 0 ? (
                <EmptyState
                  icon={<Radio className="h-12 w-12" />}
                  title={t('messages.channels.empty.title')}
                  description={t('messages.channels.empty.description')}
                  action={{
                    label: t('messages.channels.create'),
                    onClick: () => setCreateDialogOpen(true),
                  }}
                />
              ) : (
                channels.map((channel) => {
                  const neutralChannelIcon = 'border border-border bg-muted-30 text-foreground'
                  const config: Record<string, { icon: typeof Bell; color: string }> = {
                    console: { icon: Bell, color: neutralChannelIcon },
                    memory: { icon: RefreshCw, color: neutralChannelIcon },
                    webhook: { icon: Megaphone, color: neutralChannelIcon },
                    email: { icon: Bell, color: neutralChannelIcon },
                    telegram: { icon: Send, color: neutralChannelIcon },
                    wecom: { icon: MessageSquare, color: neutralChannelIcon },
                    dingtalk: { icon: MessageCircle, color: neutralChannelIcon },
                    slack: { icon: Hash, color: neutralChannelIcon },
                    feishu: { icon: MessagesSquare, color: neutralChannelIcon },
                  }
                  const channelConfig = config[channel.channel_type] || config.console
                  const ChannelIcon = channelConfig.icon
                  const testResult = testResults[channel.name]

                  return (
                    <button
                      type="button"
                      key={channel.name}
                      className="block w-full rounded-2xl border border-border bg-card px-4 py-3 text-left transition-colors active:bg-muted"
                      onClick={() => handleViewChannel(channel.name)}
                    >
                      <div className="flex items-center gap-3">
                          <div className={cn("flex h-9 w-9 shrink-0 items-center justify-center rounded-xl", channelConfig.color)}>
                            <ChannelIcon className="h-4 w-4" />
                          </div>
                          <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 items-center gap-2">
                              <span className="min-w-0 flex-1 truncate text-sm font-medium">{channel.name}</span>
                              <span className={cn(textNano, "inline-flex shrink-0 items-center gap-1 text-muted-foreground")}>
                                <span className={cn("h-1.5 w-1.5 rounded-full", channel.enabled ? "bg-success" : "bg-muted-foreground")} />
                                {channel.enabled ? t('enabled') : t('disabled')}
                              </span>
                            </div>
                            <div className={cn(textMini, "mt-1 flex items-center gap-1.5 text-muted-foreground")}>
                              <span>{channel.channel_type}</span>
                              {channel.channel_type === 'email' && channel.recipients && channel.recipients.length > 0 && (
                                <><span aria-hidden="true">·</span><span>{channel.recipients.length} {t('messages.channels.recipients', 'recipients')}</span></>
                              )}
                              {testResult && (
                                <><span aria-hidden="true">·</span><span className={cn("inline-flex items-center gap-1", testResult.success ? 'text-success' : 'text-error')}>{testResult.success ? <Check className="h-3 w-3 shrink-0" /> : <X className="h-3 w-3 shrink-0" />}{testResult.message}</span></>
                              )}
                            </div>
                          </div>
                      </div>
                    </button>
                  )
                })
              )}
            </div>
          ) : (
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
            data={channels}
            rowKey={(ch: MessageChannel) => ch.name}
            onRowClick={(row: MessageChannel) => handleViewChannel(row.name)}
            renderCell={(columnKey, rowData) => {
              const channel = rowData
              const neutralChannelIcon = 'border border-border bg-muted-30 text-foreground'
              const config: Record<string, { icon: typeof Bell; color: string }> = {
                console: { icon: Bell, color: neutralChannelIcon },
                memory: { icon: RefreshCw, color: neutralChannelIcon },
                webhook: { icon: Megaphone, color: neutralChannelIcon },
                email: { icon: Bell, color: neutralChannelIcon },
                telegram: { icon: Send, color: neutralChannelIcon },
                wecom: { icon: MessageSquare, color: neutralChannelIcon },
                dingtalk: { icon: MessageCircle, color: neutralChannelIcon },
                slack: { icon: Hash, color: neutralChannelIcon },
                feishu: { icon: MessagesSquare, color: neutralChannelIcon },
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
                          {['webhook', 'email', 'telegram', 'wecom', 'dingtalk', 'slack', 'feishu'].includes(channel.channel_type) && (
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 px-2 text-xs sm:h-6"
                              onClick={() => handleTestChannel(channel.name)}
                              disabled={testingChannel === channel.name}
                            >
                              <TestTube className="h-4 w-4 mr-1" />
                              {testingChannel === channel.name ? 'Testing...' : 'Test'}
                            </Button>
                          )}
                        </div>
                        <div className="flex items-center gap-2 text-xs text-muted-foreground">
                          <span>{channel.channel_type}</span>
                          {channel.channel_type === 'email' && channel.recipients && channel.recipients.length > 0 && (
                            <>
                              <span>·</span>
                              <span className="flex items-center gap-1">
                                <Mail className="h-4 w-4" />
                                {channel.recipients.length} {t('messages.channels.recipients', 'recipients')}
                              </span>
                            </>
                          )}
                        </div>
                        {testResult && (
                          <div className={cn("text-xs mt-1 inline-flex items-center gap-1", testResult.success ? 'text-success' : 'text-error')}>
                            {testResult.success ? <Check className="h-3 w-3 shrink-0" /> : <X className="h-3 w-3 shrink-0" />}
                            <span>{testResult.message}</span>
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
                label: t('view'),
                icon: <Eye className="h-4 w-4" />,
                onClick: (rowData) => {
                  const channel = rowData
                  handleViewChannel(channel.name)
                },
              },
              {
                label: t('edit', 'Edit'),
                icon: <Settings className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData
                  return ['webhook', 'email', 'telegram', 'wecom', 'dingtalk', 'slack', 'feishu'].includes(channel.channel_type)
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleEditChannel(channel)
                },
              },
              {
                label: t('messages.channels.configureFilter', 'Configure Filter'),
                icon: <FilterIcon className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData
                  return channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleOpenFilterDialog(channel)
                },
              },
              {
                label: t('messages.channels.manageRecipients', 'Manage Recipients'),
                icon: <UserPlus className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData
                  return channel.channel_type === 'email'
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleManageRecipients(channel)
                },
              },
              {
                label: t('enable'),
                icon: <CheckCircle2 className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData
                  return !channel.enabled && channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleToggleEnabled(channel.name, true)
                },
              },
              {
                label: t('disable'),
                icon: <X className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData
                  return channel.enabled && channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleToggleEnabled(channel.name, false)
                },
              },
              {
                label: t('delete'),
                icon: <Trash2 className="h-4 w-4" />,
                variant: 'destructive',
                show: (rowData) => {
                  const channel = rowData
                  return channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData
                  handleDeleteChannel(channel.name)
                },
              },
            ]}
            loading={loading}
            emptyState={
              !loading && channels.length === 0 ? (
                <EmptyState
                  icon={<Radio className="h-12 w-12" />}
                  title={t('messages.channels.empty.title')}
                  description={t('messages.channels.empty.description')}
                  action={{
                    label: t('messages.channels.create'),
                    onClick: () => setCreateDialogOpen(true),
                  }}
                />
              ) : undefined
            }
          />
          )}
        </PageTabsContent>

        {/* IM Sessions Tab */}
        <PageTabsContent value="im" activeTab={activeTab}>
          <ImSessionsTab />
        </PageTabsContent>
      </PageLayout>

      <CreateMessageDialog
        open={createDialogOpen}
        onOpenChange={setCreateDialogOpen}
        onCreate={async (req) => {
          try {
            await api.createMessage({
              category: req.category || 'alert',
              severity: req.severity || 'info',
              title: req.title,
              message: req.message,
              source: req.source || 'manual',
              source_type: req.source_type || 'ui',
              tags: req.tags || [],
            })
            await fetchMessages()
            toast({ title: t('messages.createSuccess', 'Message created') })
          } catch {
            toast({ title: t('messages.createError', 'Failed to create'), variant: 'destructive' })
          }
        }}
      />

      {/* Message Detail Dialog */}
      <UnifiedFormDialog
        open={!!selectedMessage}
        onOpenChange={(open) => !open && setSelectedMessage(null)}
        title={selectedMessage?.title || t('messages.messageDetails', 'Message Details')}
        icon={<Bell className="h-4 w-4 text-foreground" strokeWidth={1.75} />}
        width="xl"
        showCancelButton={false}
        footer={
          <div className="flex flex-wrap justify-end gap-2">
            {selectedMessage?.status === 'active' && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  handleAcknowledge(selectedMessage.id)
                  setSelectedMessage(null)
                }}
              >
                <Check className="h-3.5 w-3.5 mr-1.5" />
                {t('messages.acknowledge', 'Acknowledge')}
              </Button>
            )}
            {selectedMessage?.status !== 'false_positive' &&
              selectedMessage?.status !== 'archived' && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    if (selectedMessage) {
                      handleFalsePositive(selectedMessage.id)
                      setSelectedMessage(null)
                    }
                  }}
                >
                  <Flag className="h-3.5 w-3.5 mr-1.5" />
                  {t('messages.markFalsePositive', 'Not a real alert')}
                </Button>
              )}
            {selectedMessage?.status !== 'resolved' && selectedMessage?.status !== 'archived' && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  if (selectedMessage) {
                    handleResolve(selectedMessage.id)
                    setSelectedMessage(null)
                  }
                }}
              >
                <CheckCircle2 className="h-3.5 w-3.5 mr-1.5" />
                {t('messages.resolve', 'Resolve')}
              </Button>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={() => setSelectedMessage(null)}
            >
              {t('close', 'Close')}
            </Button>
          </div>
        }
      >
        {selectedMessage && (
          <div className="space-y-4">
            {/* Status & Timestamp Strip */}
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-border bg-muted-20 px-3.5 py-2.5">
              <div className="flex flex-wrap items-center gap-1.5">
                <Badge
                  variant="outline"
                  className={cn(
                    "text-xs font-normal border-transparent",
                    selectedMessage.severity === 'critical' || selectedMessage.severity === 'emergency'
                      ? 'bg-error-light text-error'
                      : selectedMessage.severity === 'warning'
                        ? 'bg-warning-light text-warning'
                        : 'bg-info-light text-info'
                  )}
                >
                  {t(`messages.severity.${selectedMessage.severity}`, selectedMessage.severity)}
                </Badge>
                <Badge variant={selectedMessage.status === 'active' ? 'default' : 'secondary'} className="text-xs font-normal">
                  {t(`messages.status.${selectedMessage.status}`, selectedMessage.status)}
                </Badge>
                <Badge variant="outline" className="text-xs font-normal bg-card border-glass-border text-muted-foreground">
                  {selectedMessage.category}
                </Badge>
              </div>
              <span className="text-xs text-muted-foreground tabular-nums">
                {formatTimestamp(selectedMessage.timestamp, true)}
              </span>
            </div>

            {/* Message Body */}
            <div className="rounded-xl border border-border bg-card p-4 text-sm leading-relaxed text-foreground whitespace-pre-wrap break-words">
              {selectedMessage.message}
            </div>

            {/* Source Meta Grid */}
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
              <div className="rounded-xl border border-border bg-muted-20 px-3.5 py-2.5">
                <div className={cn(textMini, "text-muted-foreground")}>{t('messages.source', 'Source')}</div>
                <div className="mt-0.5 text-sm font-medium text-foreground truncate">{selectedMessage.source || '—'}</div>
              </div>
              <div className="rounded-xl border border-border bg-muted-20 px-3.5 py-2.5">
                <div className={cn(textMini, "text-muted-foreground")}>{t('messages.sourceType.label', 'Source Type')}</div>
                <div className="mt-0.5 text-sm font-medium text-foreground truncate">{selectedMessage.source_type || '—'}</div>
              </div>
            </div>

            {/* Why this fired (M2-5) — clean key-value card matching Channel Details */}
            {messageChain &&
              (messageChain.resolved || messageChain.reason === 'execution_not_found') && (
                <div className="space-y-1.5">
                  <div className="text-xs font-medium text-muted-foreground">
                    {t('messages.chain.title', 'Why this fired')}
                  </div>
                  {messageChain.resolved ? (
                    <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden text-sm">
                      <div className="flex items-baseline justify-between gap-4 px-3.5 py-2.5">
                        <span className="text-xs text-muted-foreground shrink-0">
                          {t('messages.chain.rule', 'Rule')}
                        </span>
                        <span className="font-medium text-right truncate">
                          {messageChain.rule?.name || messageChain.rule?.id || '—'}
                        </span>
                      </div>
                      {messageChain.trigger?.source && (
                        <div className="flex items-baseline justify-between gap-4 px-3.5 py-2.5">
                          <span className="text-xs text-muted-foreground shrink-0">
                            {t('messages.chain.evidence', 'Evidence')}
                          </span>
                          <span className="font-mono text-xs text-right break-all">
                            {messageChain.trigger.source}
                            {messageChain.trigger.value ? ` = ${messageChain.trigger.value}` : ''}
                          </span>
                        </div>
                      )}
                      {messageChain.execution && (
                        <>
                          <div className="flex items-baseline justify-between gap-4 px-3.5 py-2.5">
                            <span className="text-xs text-muted-foreground shrink-0">
                              {t('messages.chain.firedAt', 'Fired')}
                            </span>
                            <span className="text-xs tabular-nums text-right">
                              {formatTimestamp(messageChain.execution.triggered_at, true)}
                            </span>
                          </div>
                          <div className="flex items-baseline justify-between gap-4 px-3.5 py-2.5">
                            <span className="text-xs text-muted-foreground shrink-0">
                              {t('messages.chain.actions', 'Actions')}
                            </span>
                            <span className="flex flex-wrap justify-end gap-1">
                              {messageChain.execution.actions_executed.length > 0 ? (
                                messageChain.execution.actions_executed.map((action, i) => (
                                  <Badge key={i} variant="outline" className="text-xs font-mono font-normal bg-muted-20 border-glass-border">
                                    {action}
                                  </Badge>
                                ))
                              ) : (
                                <span className="text-xs text-muted-foreground">
                                  {t('messages.chain.noActions', 'none')}
                                </span>
                              )}
                            </span>
                          </div>
                        </>
                      )}
                      {messageChain.rule_quality && messageChain.rule_quality.alerts > 0 && (
                        <div className="flex items-baseline justify-between gap-4 px-3.5 py-2.5">
                          <span className="text-xs text-muted-foreground shrink-0">
                            {t('messages.chain.trackRecord', 'Track record')}
                          </span>
                          <span className="text-xs text-right">
                            {t('messages.chain.dismissedOf', {
                              defaultValue: '{{fp}} of {{total}} alerts dismissed as false',
                              fp: messageChain.rule_quality.false_positives,
                              total: messageChain.rule_quality.alerts,
                            })}
                            {messageChain.rule_quality.suggest_threshold_review && (
                              <span className="text-warning">
                                {' · '}
                                {t(
                                  'messages.chain.thresholdAdvice',
                                  'this rule probably fires too easily — consider tightening its condition',
                                )}
                              </span>
                            )}
                          </span>
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="rounded-xl border border-border bg-muted-20 px-3.5 py-2.5 text-xs text-muted-foreground leading-relaxed">
                      {t(
                        'messages.chain.expired',
                        'This alert points at a rule execution whose record has expired — rule history is kept for 30 days.',
                      )}
                    </div>
                  )}
                </div>
              )}

            {/* Tags Section */}
            {selectedMessage.tags && selectedMessage.tags.length > 0 && (
              <div className="space-y-1.5">
                <div className="text-xs font-medium text-muted-foreground">{t('messages.tags.label', 'Tags')}</div>
                <div className="flex flex-wrap gap-1.5">
                  {(selectedMessage.tags || []).map((tag, i) => (
                    <Badge key={i} variant="outline" className="text-xs font-normal bg-muted-20 border-glass-border text-muted-foreground">
                      {tag}
                    </Badge>
                  ))}
                </div>
              </div>
            )}

            {/* Metadata Section */}
            {selectedMessage.metadata && Object.keys(selectedMessage.metadata).length > 0 && (
              <FormSection
                title={t('messages.metadata', 'Metadata')}
                collapsible
                defaultExpanded={false}
              >
                <pre className="bg-muted-20 border border-glass-border rounded-xl p-3 text-xs font-mono overflow-x-auto max-h-40 overflow-y-auto">
                  {JSON.stringify(selectedMessage.metadata, null, 2)}
                </pre>
              </FormSection>
            )}
          </div>
        )}
      </UnifiedFormDialog>

      {/* View Channel Dialog */}
      <UnifiedFormDialog
        open={!!viewChannel}
        onOpenChange={(open) => !open && setViewChannel(null)}
        title={t('messages.channels.channelDetails', 'Channel Details')}
        icon={<Network className="h-4 w-4 text-foreground" strokeWidth={1.75} />}
        width="md"
        showCancelButton={false}
        footer={
          <div className="flex w-full items-center justify-between gap-2">
            <div className="flex items-center gap-2">
              {viewChannel && viewChannel.channel_type !== 'console' && viewChannel.channel_type !== 'memory' && (
                <>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      handleTestChannel(viewChannel.name)
                      setViewChannel(null)
                    }}
                    disabled={testingChannel === viewChannel.name}
                  >
                    <TestTube className="h-3.5 w-3.5 mr-1.5" />
                    {t('messages.channels.test')}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      handleToggleEnabled(viewChannel.name, !viewChannel.enabled)
                      setViewChannel(null)
                    }}
                  >
                    {viewChannel.enabled ? t('disable') : t('enable')}
                  </Button>
                </>
              )}
            </div>
            <Button variant="outline" size="sm" onClick={() => setViewChannel(null)}>
              {t('close', 'Close')}
            </Button>
          </div>
        }
      >
        {viewChannel && (
          <div className="space-y-4">
            {/* Hero Identity Card */}
            <div className="flex items-center justify-between gap-3 rounded-xl border border-border bg-muted-20 px-4 py-3.5">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold text-foreground">{viewChannel.name}</div>
                <div className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
                  <span className={cn("inline-flex items-center rounded-md border border-glass-border bg-card px-2 py-0.5 font-mono capitalize", textMini)}>
                    {viewChannel.channel_type}
                  </span>
                </div>
              </div>
              <span className="inline-flex shrink-0 items-center gap-1.5 rounded-full border border-border bg-card px-2.5 py-1 text-xs font-medium text-foreground">
                <span className={cn("h-1.5 w-1.5 rounded-full", viewChannel.enabled ? "bg-success" : "bg-muted-foreground")} />
                {viewChannel.enabled ? t('enabled') : t('disabled')}
              </span>
            </div>

            {/* Configuration Key-Value List */}
            {viewChannel.config && typeof viewChannel.config === 'object' && Object.keys(viewChannel.config).length > 0 && (
              <div className="space-y-1.5">
                <div className="text-xs font-medium text-muted-foreground">
                  {t('messages.channels.config', 'Configuration')}
                </div>
                <div className="rounded-xl border border-border bg-card divide-y divide-glass-border overflow-hidden">
                  {Object.entries(viewChannel.config as Record<string, unknown>)
                    .filter(([, v]) => v !== undefined && v !== null && v !== '')
                    .map(([k, v]) => {
                      const configLabels: Record<string, string> = {
                        url: t('messages.channels.webhookUrl', 'Webhook URL'),
                        webhook_url: t('messages.channels.webhookUrl', 'Webhook URL'),
                        smtp_server: t('messages.channels.smtpServer', 'SMTP Server'),
                        smtp_port: t('messages.channels.smtpPort', 'SMTP Port'),
                        from_address: t('messages.channels.emailFrom', 'From'),
                        username: t('auth:username', 'Username'),
                        password: t('auth:password', 'Password'),
                        chat_id: 'Chat ID',
                        bot_token: 'Bot Token',
                      }
                      const isSecret = /(^|_)(api_?key|secret|token|password|access_?key|private_?key|app_?secret)$/i.test(k)
                      const rawStr = typeof v === 'object' ? JSON.stringify(v) : String(v)
                      const displayVal = isSecret && rawStr.length > 6 && !rawStr.includes('***')
                        ? `${rawStr.slice(0, 3)}••••••${rawStr.slice(-3)}`
                        : rawStr
                      return (
                        <div key={k} className="flex flex-col sm:flex-row sm:items-baseline sm:justify-between gap-1 sm:gap-4 px-3.5 py-2.5">
                          <span className="text-xs text-muted-foreground shrink-0">{configLabels[k] || k}</span>
                          <span className="text-xs font-mono text-foreground break-all sm:text-right">{displayVal}</span>
                        </div>
                      )
                    })}
                </div>
              </div>
            )}
          </div>
        )}
      </UnifiedFormDialog>

      {/* Recipients Management Dialog */}
      <UnifiedFormDialog
        open={!!recipientsDialogChannel}
        onOpenChange={(open) => {
          if (!open) {
            setRecipientsDialogChannel(null)
            setRecipientError(null)
            setNewRecipientEmail('')
          }
        }}
        title={t('messages.channels.manageRecipients', 'Manage Recipients')}
        icon={<Mail className="h-4 w-4 text-foreground" strokeWidth={1.75} />}
        width="sm"
        loading={loadingRecipients}
        submitError={recipientError}
        footer={
          <Button size="sm" onClick={() => setRecipientsDialogChannel(null)}>
            {t('done', 'Done')}
          </Button>
        }
      >
        <div className="space-y-4">
          {/* Add new recipient */}
          <FormField label={t('messages.channels.addRecipient', 'Add Recipient')}>
            <div className="flex gap-2">
              <Input
                type="email"
                placeholder={t('messages.channels.emailPlaceholder', 'email@example.com')}
                value={newRecipientEmail}
                onChange={(e) => {
                  setNewRecipientEmail(e.target.value)
                  if (recipientError) setRecipientError(null)
                }}
                onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), handleAddRecipient())}
                disabled={addingRecipient}
                className="flex-1"
              />
              <Button
                onClick={handleAddRecipient}
                disabled={addingRecipient || !newRecipientEmail.trim()}
                size="default"
              >
                {addingRecipient ? t('adding', 'Adding...') : t('add')}
              </Button>
            </div>
          </FormField>

          {/* Recipients list */}
          <div className="space-y-2">
            <Label className="text-muted-foreground text-xs">
              {t('messages.channels.currentRecipients', 'Current Recipients')} ({recipients.length})
            </Label>
            {loadingRecipients ? (
              <div className="text-center py-4 text-sm text-muted-foreground">
                {t('loading')}
              </div>
            ) : recipients.length === 0 ? (
              <div className="text-center py-6 text-muted-foreground border border-dashed border-border rounded-xl bg-muted-20">
                <Mail className="h-6 w-6 mx-auto mb-2 opacity-50" strokeWidth={1.75} />
                <p className="text-xs">{t('messages.channels.noRecipients', 'No recipients configured')}</p>
              </div>
            ) : (
              <div className="rounded-xl border border-border bg-card divide-y divide-glass-border max-h-[280px] overflow-y-auto">
                {recipients.map((email) => (
                  <div
                    key={email}
                    className="flex items-center justify-between px-3.5 py-2.5"
                  >
                    <span className="min-w-0 flex-1 truncate text-sm font-mono">{email}</span>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 w-7 p-0 text-muted-foreground hover:text-error shrink-0 ml-2 sm:h-7 sm:w-7"
                      onClick={() => handleRemoveRecipient(email)}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </div>
      </UnifiedFormDialog>

      {/* Channel Filter Configuration Dialog */}
      <UnifiedFormDialog
        open={!!filterDialogChannel}
        onOpenChange={(open) => !open && setFilterDialogChannel(null)}
        title={t('messages.channels.filterConfig', 'Filter Configuration')}
        icon={<FilterIcon className="h-4 w-4 text-foreground" strokeWidth={1.75} />}
        width="md"
        onSubmit={handleSaveFilter}
        isSubmitting={savingFilter}
        submitLabel={t('save')}
        cancelLabel={t('cancel')}
      >
        <div className="space-y-5">
          {/* Source Types Section */}
          <div className="space-y-2.5">
            <div>
              <div className="text-sm font-medium text-foreground">
                {t('messages.channels.sourceTypes', 'Source Types')}
              </div>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t('messages.channels.sourceTypesHint', 'Leave unchecked to accept all sources')}
              </p>
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {['device', 'rule', 'telemetry', 'schedule', 'llm', 'system'].map((st) => {
                const checked = filterConfig.source_types.includes(st)
                return (
                  <SelectableChip
                    key={st}
                    selected={checked}
                    onToggle={() => {
                      setFilterConfig((prev) => ({
                        ...prev,
                        source_types: checked
                          ? prev.source_types.filter((item) => item !== st)
                          : [...prev.source_types, st],
                      }))
                    }}
                  >
                    <span className="capitalize">{st}</span>
                  </SelectableChip>
                )
              })}
            </div>
          </div>

          {/* Categories Section */}
          <div className="space-y-2.5">
            <div>
              <div className="text-sm font-medium text-foreground">
                {t('messages.channels.categories', 'Categories')}
              </div>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t('messages.channels.categoriesHint', 'Leave unchecked to accept all categories')}
              </p>
            </div>
            <div className="grid grid-cols-2 gap-2">
              {['alert', 'system', 'business', 'notification'].map((cat) => {
                const checked = filterConfig.categories.includes(cat)
                return (
                  <SelectableChip
                    key={cat}
                    selected={checked}
                    onToggle={() => {
                      setFilterConfig((prev) => ({
                        ...prev,
                        categories: checked
                          ? prev.categories.filter((item) => item !== cat)
                          : [...prev.categories, cat],
                      }))
                    }}
                  >
                    {t(`messages.category.${cat}`, cat)}
                  </SelectableChip>
                )
              })}
            </div>
          </div>

          {/* Minimum Severity Section */}
          <div className="space-y-2">
            <div>
              <div className="text-sm font-medium text-foreground">
                {t('messages.channels.minSeverity', 'Minimum Severity')}
              </div>
              <p className="text-xs text-muted-foreground mt-0.5">
                {t('messages.channels.minSeverityHint', 'Only receive messages at or above this severity level')}
              </p>
            </div>
            <Select
              value={filterConfig.min_severity || 'all'}
              onValueChange={(value) => {
                setFilterConfig((prev) => ({
                  ...prev,
                  min_severity: value === 'all' ? null : (value as MessageSeverity),
                }))
              }}
            >
              <SelectTrigger>
                <SelectValue placeholder={t('messages.channels.allSeverities', 'All Severities')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('messages.channels.allSeverities', 'All Severities')}</SelectItem>
                <SelectItem value="info">{t('messages.severity.info', 'Info')}</SelectItem>
                <SelectItem value="warning">{t('messages.severity.warning', 'Warning')}</SelectItem>
                <SelectItem value="critical">{t('messages.severity.critical', 'Critical')}</SelectItem>
                <SelectItem value="emergency">{t('messages.severity.emergency', 'Emergency')}</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {/* Filter Summary */}
          <div className="rounded-xl border border-border bg-muted-20 px-4 py-3 space-y-1">
            <div className="text-xs font-medium text-foreground">
              {t('messages.channels.filterPreview', 'Filter Preview')}
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              {filterConfig.source_types.length === 0 && filterConfig.categories.length === 0 && !filterConfig.min_severity
                ? t('messages.channels.filterAcceptAll', 'This channel will receive all messages')
                : [
                    `${t('messages.channels.sourceTypes', 'Source Types')}: ${filterConfig.source_types.length > 0 ? filterConfig.source_types.join(', ') : t('messages.channels.all', 'All')}`,
                    `${t('messages.channels.categories', 'Categories')}: ${filterConfig.categories.length > 0 ? filterConfig.categories.map((c) => t(`messages.category.${c}`, c)).join(', ') : t('messages.channels.all', 'All')}`,
                    `${t('messages.channels.minSeverity', 'Minimum Severity')}: ${filterConfig.min_severity ? t(`messages.severity.${filterConfig.min_severity}`, filterConfig.min_severity) : t('messages.channels.allSeverities', 'All Severities')}`,
                  ].join(' · ')}
            </p>
          </div>
        </div>
      </UnifiedFormDialog>

      {/* Create/Edit Channel Dialog */}
      <ChannelEditorDialog
        open={channelEditorOpen}
        onOpenChange={(open) => {
          setChannelEditorOpen(open)
          if (!open) setEditingChannel(null)
        }}
        editingChannel={editingChannel}
        onSaved={fetchChannels}
      />

    </>
  )
}

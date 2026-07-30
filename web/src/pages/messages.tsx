// Messages Page
// Unified notification/message system for NeoMind

import { useState, useCallback, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useLocation } from 'react-router-dom'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabsBar, PageTabsContent, PageTabsBottomNav, Pagination, ResponsiveTable, EmptyState } from '@/components/shared'
import { MessageSquare, Network, Settings, Filter as FilterIcon, Inbox, Loader2 } from 'lucide-react'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'
import { confirm } from '@/hooks/use-confirm'
import { useErrorHandler } from '@/hooks/useErrorHandler'
import { useIsMobile } from '@/hooks/useMobile'
import type { NotificationMessage, MessageSeverity, MessageStatus, MessageCategory, MessageChannel, ChannelFilter } from '@/types'
import type { StandardError } from '@/lib/errors'

import { Card } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button, IconButton } from '@/components/ui/button'
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
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { Label } from '@/components/ui/label'
import { FormSection } from '@/components/ui/form-section'
import { Input } from '@/components/ui/input'
import { FormField } from '@/components/ui/field'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  DropdownMenuCheckboxItem,
} from '@/components/ui/dropdown-menu'
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
  MoreVertical,
  Eye,
  RefreshCw,
  Filter,
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
  const [mobileLoadedPages, setMobileLoadedPages] = useState(1)

  // Filters - support multiple selections
  const [selectedSeverities, setSelectedSeverities] = useState<Set<MessageSeverity>>(new Set())
  const [selectedStatuses, setSelectedStatuses] = useState<Set<MessageStatus>>(new Set())
  const [selectedCategories, setSelectedCategories] = useState<Set<string>>(new Set())

  // Dynamic categories from API
  const [availableCategories, setAvailableCategories] = useState<string[]>([])

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

  const toggleCategory = (category: string) => {
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
  const [channelEditorOpen, setChannelEditorOpen] = useState(false)
  const [editingChannel, setEditingChannel] = useState<MessageChannel | null>(null)

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

      let messagesArray: NotificationMessage[] = messagesResponse.messages || []
      const serverTotal = messagesResponse.total ?? 0

      // Extract categories
      const categories = [...new Set(messagesArray.map(m => m.category))].sort()
      setAvailableCategories(categories)

      // Multi-value filters (server only supports single value)
      if (selectedSeverities.size > 1) {
        messagesArray = messagesArray.filter((m: NotificationMessage) =>
          selectedSeverities.has(m.severity as MessageSeverity))
      }
      if (selectedStatuses.size > 1) {
        messagesArray = messagesArray.filter((m: NotificationMessage) =>
          selectedStatuses.has(m.status as MessageStatus))
      }
      if (selectedCategories.size > 1) {
        messagesArray = messagesArray.filter((m: NotificationMessage) =>
          selectedCategories.has(m.category))
      }

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
  useEffect(() => {
    if (activeTab === 'messages') {
      fetchMessages()
    } else if (activeTab === 'channels') {
      fetchChannels()
    }
    // 'im' tab manages its own data lifecycle via ImSessionsTab
  }, [activeTab, fetchMessages, fetchChannels])

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

  const handleArchive = async (id: string) => {
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
    // 'im' tab renders its own refresh inside ImSessionsTab; no page-level action.
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
      <PopoverContent align="end" className="w-[340px] p-0 overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b bg-muted-30">
          <span className="text-sm font-semibold">{t('messages.filter.title')}</span>
          {hasActiveFilters && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs text-muted-foreground hover:text-foreground px-2"
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
              {(['active', 'acknowledged', 'resolved', 'archived'] as MessageStatus[]).map((stat) => {
                const colors = {
                  active: "text-info bg-info-light border-info",
                  acknowledged: "text-warning bg-warning-light border-warning",
                  resolved: "text-success bg-success-light border-success",
                  archived: "text-muted-foreground bg-muted border-border",
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
        subtitle={t('messages.description')}
        hideFooterOnMobile
        hasBottomNav
        headerContent={
          <PageTabsBar
            tabs={tabs}
            activeTab={activeTab}
            onTabChange={(v) => handleTabChange(v as TabValue)}
            actions={actions}
            actionsExtra={filterExtra}
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
                className="h-7 px-2 text-xs"
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
                  <Card key={i} className="overflow-hidden border-border">
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
              ) : messages.length === 0 ? (
                <EmptyState
                  icon={<Mail className="h-12 w-12" />}
                  title={t('messages.empty.title')}
                  description={t('messages.empty.description')}
                />
              ) : (
                messages.map((message) => {
                  const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                  const statusConfig = STATUS_CONFIG[message.status] || STATUS_CONFIG.active
                  const SeverityIcon = severityConfig.icon
                  const isDimmed = message.status === 'resolved' || message.status === 'archived'

                  return (
                    <Card
                      key={message.id}
                      className={cn(
                        "overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all",
                        isDimmed && "opacity-50"
                      )}
                      onClick={() => setSelectedMessage(message)}
                    >
                      <div className="px-3 py-2.5">
                        {/* Row 1: severity icon + title + status + actions */}
                        <div className="flex items-center gap-2.5">
                          <div className={cn(
                            "w-8 h-8 rounded-lg flex items-center justify-center shrink-0 border",
                            severityConfig.bgColor
                          )}>
                            <SeverityIcon className={cn("h-4 w-4", severityConfig.color)} />
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="font-medium text-sm truncate">{message.title}</div>
                          </div>
                          <Badge variant={statusConfig.variant} className={cn(textNano, "h-5 px-1.5 shrink-0")}>
                            {t(statusConfig.label)}
                          </Badge>
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                              <IconButton>
                                <MoreVertical className="h-4 w-4" />
                              </IconButton>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" className="w-40">
                              <DropdownMenuItem onClick={(e) => { e.stopPropagation(); setSelectedMessage(message) }}>
                                <Eye className="mr-2 h-4 w-4" />
                                {t('messages.viewDetails', 'View Details')}
                              </DropdownMenuItem>
                              {message.status === 'active' && (
                                <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleAcknowledge(message.id) }}>
                                  <Eye className="mr-2 h-4 w-4" />
                                  {t('messages.acknowledge')}
                                </DropdownMenuItem>
                              )}
                              {message.status !== 'resolved' && message.status !== 'archived' && (
                                <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleResolve(message.id) }}>
                                  <CheckCircle2 className="mr-2 h-4 w-4" />
                                  {t('messages.resolve')}
                                </DropdownMenuItem>
                              )}
                              <DropdownMenuItem
                                className="text-error"
                                onClick={(e) => { e.stopPropagation(); handleDelete(message.id) }}
                              >
                                <Trash2 className="mr-2 h-4 w-4" />
                                {t('delete')}
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                        {/* Row 2: type badge + category + time */}
                        <div className="flex items-center gap-1.5 mt-1.5 ml-[42px]">
                          <Badge variant="outline" className={cn(
                            textNano, "h-5 px-1.5 shrink-0",
                            "bg-info-light text-info border-info"
                          )}>
                            {t('messages.type.notification')}
                          </Badge>
                          <Badge variant="outline" className={cn(textNano, "h-5 px-1.5 shrink-0")}>
                            {message.category}
                          </Badge>
                          <span className={cn(textMini, "text-muted-foreground ml-auto shrink-0")}>
                            {formatTimestamp(message.timestamp, false)}
                          </span>
                        </div>
                      </div>
                    </Card>
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
                  key: 'messageType',
                  label: t('messages.type.label'),
                  width: 'w-[100px]',
                  align: 'center',
                },
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
                  label: t('common:createdAt'),
                  width: 'w-[130px]',
                },
              ]}
              data={messages as unknown as Record<string, unknown>[]}
              rowKey={(msg) => (msg as unknown as NotificationMessage).id}
              renderCell={(columnKey, rowData) => {
                const message = rowData as unknown as NotificationMessage
                const severityConfig = SEVERITY_CONFIG[message.severity] || SEVERITY_CONFIG.info
                const categoryConfig = getCategoryConfig(message.category)
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
                        {(message.tags || []).length > 0 && (
                          <div className="flex items-center gap-1 mt-1.5 flex-wrap">
                            {(message.tags || []).slice(0, 4).map((tag, i) => (
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
                        <CategoryIcon className="h-4 w-4 text-muted-foreground" />
                        <span className="text-xs">{t(categoryConfig.label)}</span>
                      </div>
                    )
                  case 'messageType':
                    return (
                      <Badge
                        variant="outline"
                        className={cn(
                          "text-xs",
                          "bg-info-light text-info border-info"
                        )}
                      >
                        <Bell className="h-4 w-4 mr-1" />
                        {t('messages.type.notification')}
                      </Badge>
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
              onRowClick={(rowData) => {
                // Quick-open detail dialog on row click — same as "View Details"
                // action in the dropdown, just one tap instead of two.
                setSelectedMessage(rowData as unknown as NotificationMessage)
              }}
              actions={[
                {
                  label: t('messages.viewDetails', 'View Details'),
                  icon: <Eye className="h-4 w-4" />,
                  onClick: (rowData) => {
                    const message = rowData as unknown as NotificationMessage
                    setSelectedMessage(message)
                  },
                },
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
                  <EmptyState
                    icon={<Mail className="h-12 w-12" />}
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
                  <Card key={i} className="overflow-hidden border-border">
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
                />
              ) : (
                channels.map((channel) => {
                  const config: Record<string, { icon: typeof Bell; color: string }> = {
                    console: { icon: Bell, color: 'bg-muted text-muted-foreground' },
                    memory: { icon: RefreshCw, color: 'bg-info-light text-info' },
                    webhook: { icon: Megaphone, color: 'bg-success-light text-success' },
                    email: { icon: Bell, color: 'bg-accent-purple-light text-accent-purple' },
                    telegram: { icon: Send, color: 'bg-accent-blue-light text-accent-blue' },
                    wecom: { icon: MessageSquare, color: 'bg-success-light text-success' },
                    dingtalk: { icon: MessageCircle, color: 'bg-accent-blue-light text-accent-blue' },
                    slack: { icon: Hash, color: 'bg-accent-orange-light text-accent-orange' },
                    feishu: { icon: MessagesSquare, color: 'bg-accent-purple-light text-accent-purple' },
                  }
                  const channelConfig = config[channel.channel_type] || config.console
                  const ChannelIcon = channelConfig.icon
                  const testResult = testResults[channel.name]

                  return (
                    <Card
                      key={channel.name}
                      className="overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all"
                      onClick={() => handleViewChannel(channel.name)}
                    >
                      <div className="px-3 py-2.5">
                        {/* Row 1: icon + name + status + actions */}
                        <div className="flex items-center gap-2.5">
                          <div className={cn("w-8 h-8 rounded-lg flex items-center justify-center shrink-0", channelConfig.color)}>
                            <ChannelIcon className="h-4 w-4" />
                          </div>
                          <div className="flex-1 min-w-0">
                            <div className="font-medium text-sm truncate">{channel.name}</div>
                          </div>
                          <Badge variant={channel.enabled ? 'default' : 'secondary'} className={cn(textNano, "h-5 px-1.5 shrink-0")}>
                            {channel.enabled ? t('enabled') : t('disabled')}
                          </Badge>
                          <DropdownMenu>
                            <DropdownMenuTrigger asChild onClick={(e) => e.stopPropagation()}>
                              <IconButton>
                                <MoreVertical className="h-4 w-4" />
                              </IconButton>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end" className="w-40">
                              <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleViewChannel(channel.name) }}>
                                <Eye className="mr-2 h-4 w-4" />
                                {t('view')}
                              </DropdownMenuItem>
                              {['webhook', 'email', 'telegram', 'wecom', 'dingtalk', 'slack', 'feishu'].includes(channel.channel_type) && (
                                <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleEditChannel(channel) }}>
                                  <Settings className="mr-2 h-4 w-4" />
                                  {t('edit', 'Edit')}
                                </DropdownMenuItem>
                              )}
                              {channel.channel_type !== 'console' && channel.channel_type !== 'memory' && (
                                <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleOpenFilterDialog(channel) }}>
                                  <FilterIcon className="mr-2 h-4 w-4" />
                                  {t('messages.channels.configureFilter', 'Configure Filter')}
                                </DropdownMenuItem>
                              )}
                              {channel.channel_type === 'email' && (
                                <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleManageRecipients(channel) }}>
                                  <UserPlus className="mr-2 h-4 w-4" />
                                  {t('messages.channels.manageRecipients', 'Manage Recipients')}
                                </DropdownMenuItem>
                              )}
                              {channel.channel_type !== 'console' && channel.channel_type !== 'memory' && (
                                <>
                                  {!channel.enabled && (
                                    <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleToggleEnabled(channel.name, true) }}>
                                      <CheckCircle2 className="mr-2 h-4 w-4" />
                                      {t('enable')}
                                    </DropdownMenuItem>
                                  )}
                                  {channel.enabled && (
                                    <DropdownMenuItem onClick={(e) => { e.stopPropagation(); handleToggleEnabled(channel.name, false) }}>
                                      <X className="mr-2 h-4 w-4" />
                                      {t('disable')}
                                    </DropdownMenuItem>
                                  )}
                                  <DropdownMenuSeparator />
                                  <DropdownMenuItem
                                    className="text-error"
                                    onClick={(e) => { e.stopPropagation(); handleDeleteChannel(channel.name) }}
                                  >
                                    <Trash2 className="mr-2 h-4 w-4" />
                                    {t('delete')}
                                  </DropdownMenuItem>
                                </>
                              )}
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>
                        {/* Row 2: type badge + details + test */}
                        <div className="flex items-center gap-1.5 mt-1.5 ml-[42px]">
                          <Badge variant="outline" className={cn(textNano, "h-5 px-1.5 shrink-0")}>
                            {channel.channel_type}
                          </Badge>
                          {['webhook', 'email', 'telegram', 'wecom', 'dingtalk', 'slack', 'feishu'].includes(channel.channel_type) && (
                            <Button
                              variant="ghost"
                              size="sm"
                              className={cn("h-5 px-1.5", textNano)}
                              onClick={(e) => { e.stopPropagation(); handleTestChannel(channel.name) }}
                              disabled={testingChannel === channel.name}
                            >
                              <TestTube className="h-3 w-3 mr-0.5" />
                              {testingChannel === channel.name ? '...' : 'Test'}
                            </Button>
                          )}
                          {channel.channel_type === 'email' && channel.recipients && channel.recipients.length > 0 && (
                            <span className={cn(textNano, "text-muted-foreground flex items-center gap-0.5")}>
                              <Mail className="h-3 w-3" />
                              {channel.recipients.length}
                            </span>
                          )}
                          {testResult && (
                            <span className={cn(textNano, testResult.success ? 'text-success' : 'text-error')}>
                              {testResult.success ? '✓' : '✗'} {testResult.message}
                            </span>
                          )}
                        </div>
                      </div>
                    </Card>
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
            data={channels as unknown as Record<string, unknown>[]}
            rowKey={(ch) => (ch as unknown as MessageChannel).name}
            onRowClick={(row) => handleViewChannel((row as unknown as MessageChannel).name)}
            renderCell={(columnKey, rowData) => {
              const channel = rowData as unknown as MessageChannel
              const config: Record<string, { icon: typeof Bell; color: string }> = {
                console: { icon: Bell, color: 'bg-muted text-muted-foreground' },
                memory: { icon: RefreshCw, color: 'bg-info-light text-info' },
                webhook: { icon: Megaphone, color: 'bg-success-light text-success' },
                email: { icon: Bell, color: 'bg-accent-purple-light text-accent-purple' },
                telegram: { icon: Send, color: 'bg-accent-blue-light text-accent-blue' },
                wecom: { icon: MessageSquare, color: 'bg-success-light text-success' },
                dingtalk: { icon: MessageCircle, color: 'bg-accent-blue-light text-accent-blue' },
                slack: { icon: Hash, color: 'bg-accent-orange-light text-accent-orange' },
                feishu: { icon: MessagesSquare, color: 'bg-accent-purple-light text-accent-purple' },
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
                              className="h-6 px-2 text-xs"
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
                          <div className={`text-xs mt-1 ${testResult.success ? 'text-success' : 'text-error'}`}>
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
                label: t('view'),
                icon: <Eye className="h-4 w-4" />,
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleViewChannel(channel.name)
                },
              },
              {
                label: t('edit', 'Edit'),
                icon: <Settings className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return ['webhook', 'email', 'telegram', 'wecom', 'dingtalk', 'slack', 'feishu'].includes(channel.channel_type)
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleEditChannel(channel)
                },
              },
              {
                label: t('messages.channels.configureFilter', 'Configure Filter'),
                icon: <FilterIcon className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleOpenFilterDialog(channel)
                },
              },
              {
                label: t('messages.channels.manageRecipients', 'Manage Recipients'),
                icon: <UserPlus className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return channel.channel_type === 'email'
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleManageRecipients(channel)
                },
              },
              {
                label: t('enable'),
                icon: <CheckCircle2 className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return !channel.enabled && channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleToggleEnabled(channel.name, true)
                },
              },
              {
                label: t('disable'),
                icon: <X className="h-4 w-4" />,
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return channel.enabled && channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  handleToggleEnabled(channel.name, false)
                },
              },
              {
                label: t('delete'),
                icon: <Trash2 className="h-4 w-4" />,
                variant: 'destructive',
                show: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
                  return channel.channel_type !== 'console' && channel.channel_type !== 'memory'
                },
                onClick: (rowData) => {
                  const channel = rowData as unknown as MessageChannel
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

      {/* Mobile Bottom Navigation */}
      <PageTabsBottomNav
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => handleTabChange(v as TabValue)}
      />

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
        icon={
          <Bell className="h-5 w-5 text-info" />
        }
        width="xl"
        showCancelButton={false}
        footer={
          <div className="flex justify-end gap-2">
            {selectedMessage?.status === 'active' && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => {
                  handleAcknowledge(selectedMessage.id)
                  setSelectedMessage(null)
                }}
              >
                <Eye className="h-4 w-4 mr-1" />
                {t('messages.acknowledge', 'Acknowledge')}
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
                <CheckCircle2 className="h-4 w-4 mr-1" />
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
          <div className="space-y-1">
            {/* Status Badges */}
            <div className="flex items-center gap-2 pb-2">
              <Badge
                variant="outline"
                className="bg-info-light text-info border-info"
              >
                <>
                  <Bell className="h-4 w-4 mr-1" />
                  {t('messages.type.notification', 'Notification')}
                </>
              </Badge>
              <Badge variant="outline" className={
                selectedMessage.severity === 'critical' ? 'bg-error-light text-error' :
                selectedMessage.severity === 'warning' ? 'bg-warning-light text-warning' :
                'bg-info-light text-info'
              }>
                {t(`messages.severity.${selectedMessage.severity}`, selectedMessage.severity)}
              </Badge>
              <Badge variant={selectedMessage.status === 'active' ? 'default' : 'secondary'}>
                {t(`messages.status.${selectedMessage.status}`, selectedMessage.status)}
              </Badge>
            </div>

            {/* Basic Info Section */}
            <FormSection
              title={t('messages.basicInfo', 'Basic Information')}
              description={t('messages.basicInfoDesc', 'Message category and source details')}
            >
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <Label className="text-muted-foreground text-xs">{t('messages.category.label', 'Category')}</Label>
                  <div className="font-medium">{selectedMessage.category}</div>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">{t('messages.source', 'Source')}</Label>
                  <div className="font-medium">{selectedMessage.source || '-'}</div>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">{t('messages.sourceType.label', 'Source Type')}</Label>
                  <div className="font-medium">{selectedMessage.source_type || '-'}</div>
                </div>
                <div className="col-span-2">
                  <Label className="text-muted-foreground text-xs">{t('messages.timestamp', 'Timestamp')}</Label>
                  <div className="font-medium">{formatTimestamp(selectedMessage.timestamp, true)}</div>
                </div>
              </div>
            </FormSection>

            {/* Content Section */}
            <FormSection
              title={t('messages.contentSection', 'Content')}
              description={t('messages.contentSectionDesc', 'Message body')}
            >
              <div className="bg-muted-30 rounded-lg p-3 text-sm whitespace-pre-wrap">
                {selectedMessage.message}
              </div>
            </FormSection>

            {/* Tags Section */}
            {selectedMessage.tags && selectedMessage.tags.length > 0 && (
              <FormSection
                title={t('messages.tags.label', 'Tags')}
                description={t('messages.tags.hint', 'Comma-separated tags for categorization')}
              >
                <div className="flex flex-wrap gap-1">
                  {(selectedMessage.tags || []).map((tag, i) => (
                    <Badge key={i} variant="secondary" className="text-xs">{tag}</Badge>
                  ))}
                </div>
              </FormSection>
            )}

            {/* Metadata Section */}
            {selectedMessage.metadata && Object.keys(selectedMessage.metadata).length > 0 && (
              <FormSection
                title={t('messages.metadata', 'Metadata')}
                description={t('messages.metadataDesc', 'Additional message metadata')}
                collapsible
                defaultExpanded={false}
              >
                <pre className="bg-muted-50 rounded-lg p-3 text-xs font-mono overflow-x-auto max-h-40 overflow-y-auto">
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
        description={viewChannel?.name || ''}
        icon={<Network className="h-5 w-5" />}
        width="md"
        showCancelButton={false}
        footer={
          <div className="flex justify-end gap-2">
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
                  <TestTube className="h-4 w-4 mr-1" />
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
        }
      >
        {viewChannel && (
          <div className="space-y-1">
            {/* Basic Info Section */}
            <FormSection
              title={t('messages.basicInfo', 'Basic Information')}
              description={t('messages.basicInfoDesc', 'Channel name and type')}
            >
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
                <div>
                  <Label className="text-muted-foreground text-xs">{t('messages.channels.name')}</Label>
                  <div className="font-medium">{viewChannel.name}</div>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">{t('messages.channels.type')}</Label>
                  <div className="font-medium capitalize">{viewChannel.channel_type}</div>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">{t('status')}</Label>
                  <Badge variant={viewChannel.enabled ? 'default' : 'secondary'}>
                    {viewChannel.enabled ? t('enabled') : t('disabled')}
                  </Badge>
                </div>
              </div>
            </FormSection>

            {/* Config Section - Webhook */}
            {viewChannel.channel_type === 'webhook' && viewChannel.config && (
              <FormSection
                title={t('messages.channels.config', 'Configuration')}
                description={t('messages.channels.webhookConfigDesc', 'Webhook endpoint URL')}
              >
                <div className="bg-muted-50 rounded-lg p-3 text-sm font-mono break-all">
                  {(() => {
                    const cfg = viewChannel.config as Record<string, unknown>
                    return typeof cfg === 'object' && cfg !== null && 'url' in cfg
                      ? String(cfg.url)
                      : JSON.stringify(viewChannel.config, null, 2)
                  })()}
                </div>
              </FormSection>
            )}

            {/* Config Section - Email */}
            {viewChannel.channel_type === 'email' && viewChannel.config && (
              <FormSection
                title={t('messages.channels.config', 'Configuration')}
                description={t('messages.channels.emailConfigDesc', 'Email server settings')}
              >
                <div className="bg-muted-50 rounded-lg p-3 text-sm space-y-1">
                  {(() => {
                    const cfg = viewChannel.config as Record<string, unknown>
                    return (
                      <>
                        {cfg && 'smtp_server' in cfg && (
                          <div>{t('messages.channels.smtpServer')}: {String(cfg.smtp_server)}</div>
                        )}
                        {cfg && 'smtp_port' in cfg && (
                          <div>{t('messages.channels.smtpPort')}: {String(cfg.smtp_port)}</div>
                        )}
                        {cfg && 'from_address' in cfg && (
                          <div>{t('messages.channels.emailFrom')}: {String(cfg.from_address)}</div>
                        )}
                      </>
                    )
                  })()}
                </div>
              </FormSection>
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
        description={recipientsDialogChannel?.name || ''}
        icon={<Mail className="h-5 w-5" />}
        width="sm"
        loading={loadingRecipients}
        submitError={recipientError}
        showCancelButton={true}
        cancelLabel={t('close')}
        onSubmit={async () => {
          // Dialog doesn't have a primary submit action - just close
          setRecipientsDialogChannel(null)
        }}
        submitLabel={t('done', 'Done')}
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
              <div className="text-center py-4 text-muted-foreground">
                {t('loading')}
              </div>
            ) : recipients.length === 0 ? (
              <div className="text-center py-6 text-muted-foreground border border-dashed rounded-lg">
                <Mail className="h-8 w-8 mx-auto mb-2 opacity-50" />
                <p className="text-sm">{t('messages.channels.noRecipients', 'No recipients configured')}</p>
              </div>
            ) : (
              <div className="space-y-2 max-h-[300px] overflow-y-auto">
                {recipients.map((email) => (
                  <div
                    key={email}
                    className="flex items-center justify-between bg-muted-50 rounded-lg px-3 py-2"
                  >
                    <span className="text-sm truncate flex-1">{email}</span>
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 w-6 p-0 text-muted-foreground hover:text-error shrink-0 ml-2"
                      onClick={() => handleRemoveRecipient(email)}
                    >
                      <X className="h-4 w-4" />
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
        description={t('messages.channels.filterConfigDesc', 'Configure which messages this channel should receive')}
        icon={<FilterIcon className="h-5 w-5" />}
        width="md"
        onSubmit={handleSaveFilter}
        isSubmitting={savingFilter}
        submitLabel={t('save')}
        cancelLabel={t('cancel')}
      >
        <div className="space-y-1">
          {/* Source Types Section */}
          <FormSection
            title={t('messages.channels.sourceTypes', 'Source Types')}
            description={t('messages.channels.sourceTypesHint', 'Leave unchecked to accept all sources')}
          >
            <div className="flex flex-wrap gap-2">
              {['device', 'rule', 'telemetry', 'schedule', 'llm', 'system'].map(st => (
                <label key={st} className="flex items-center gap-2 cursor-pointer">
                  <Checkbox
                    checked={filterConfig.source_types.includes(st)}
                    onCheckedChange={(checked) => {
                      if (checked) {
                        setFilterConfig(prev => ({
                          ...prev,
                          source_types: [...prev.source_types, st]
                        }))
                      } else {
                        setFilterConfig(prev => ({
                          ...prev,
                          source_types: prev.source_types.filter(t => t !== st)
                        }))
                      }
                    }}
                  />
                  {st}
                </label>
              ))}
            </div>
          </FormSection>

          {/* Categories Section */}
          <FormSection
            title={t('messages.channels.categories', 'Categories')}
            description={t('messages.channels.categoriesHint', 'Leave unchecked to accept all categories')}
          >
            <div className="flex flex-wrap gap-2">
              {['alert', 'system', 'business', 'notification'].map(cat => (
                <label key={cat} className="flex items-center gap-2 cursor-pointer">
                  <Checkbox
                    checked={filterConfig.categories.includes(cat)}
                    onCheckedChange={(checked) => {
                      if (checked) {
                        setFilterConfig(prev => ({
                          ...prev,
                          categories: [...prev.categories, cat]
                        }))
                      } else {
                        setFilterConfig(prev => ({
                          ...prev,
                          categories: prev.categories.filter(t => t !== cat)
                        }))
                      }
                    }}
                  />
                  {t(`messages.category.${cat}`, cat)}
                </label>
              ))}
            </div>
          </FormSection>

          {/* Minimum Severity Section */}
          <FormSection
            title={t('messages.channels.minSeverity', 'Minimum Severity')}
            description={t('messages.channels.minSeverityHint', 'Only receive messages at or above this severity level')}
          >
            <Select
              value={filterConfig.min_severity || 'all'}
              onValueChange={(value) => {
                setFilterConfig(prev => ({
                  ...prev,
                  min_severity: value === 'all' ? null : value as MessageSeverity
                }))
              }}
            >
              <SelectTrigger>
                <SelectValue placeholder={t('messages.channels.allSeverities', 'All Severities')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('messages.channels.allSeverities', 'All Severities')}</SelectItem>
                <SelectItem value="info">Info</SelectItem>
                <SelectItem value="warning">Warning</SelectItem>
                <SelectItem value="critical">Critical</SelectItem>
                <SelectItem value="emergency">Emergency</SelectItem>
              </SelectContent>
            </Select>
          </FormSection>

          {/* Filter Preview */}
          <div className="p-3 bg-muted-50 rounded-md">
            <p className="text-sm font-medium mb-1">{t('messages.channels.filterPreview', 'Filter Preview')}</p>
            <p className="text-xs text-muted-foreground">
              {filterConfig.source_types.length === 0 && filterConfig.categories.length === 0 && !filterConfig.min_severity
                ? t('messages.channels.filterAcceptAll', 'This channel will receive all messages')
                : t('messages.channels.filterWillMatch', 'Sources: {{sources}}', {
                    sources: filterConfig.source_types.length > 0 ? filterConfig.source_types.join(', ') : t('messages.channels.all', 'All')
                  })}
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

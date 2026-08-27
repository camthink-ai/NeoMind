import { useEffect, useState, useMemo, useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { PageLayout } from '@/components/layout/PageLayout'
import { useStore } from '@/store'
import { Card } from '@/components/ui/card'
import { ResponsiveTable, type TableColumn, Pagination, EmptyState } from '@/components/shared'
import { PageTabsBar, PageTabsContent, PageTabsBottomNav } from '@/components/shared/PageTabs'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { debounce } from '@/lib/utils/async'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogMain,
} from '@/components/automation/dialog/FullScreenDialog'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Search, Database, Cpu, Puzzle, Workflow, Brain, History, Loader2, Eye, Download, Clock, Copy, Check, Send, Plus, AlertTriangle, RefreshCw } from 'lucide-react'
import { api } from '@/lib/api'
import { isBase64Image, getImageDataUrl } from '@/pages/devices/utils'
import { cn } from '@/lib/utils'
import type { UnifiedDataSourceInfo } from '@/types'
import { useIsMobile } from '@/hooks/useMobile'
import { useEvents } from '@/hooks/useEvents'
import { useAbortController } from '@/hooks/useAbortController'
import { textNano, textMini } from "@/design-system/tokens/typography"
import { ExportDataDialog } from '@/components/data/ExportDataDialog'
import { formatTimestamp } from '@/lib/utils/format'
import { PushTargetsTab } from '@/components/datapush/PushTargetsTab'
import { copyToClipboard } from '@/lib/clipboard'

type TabValue = 'data' | 'push'

function SourceTypeBadge({ type }: { type: string }) {
  const colorMap: Record<string, string> = {
    device: 'bg-info-light text-info border-info',
    extension: 'bg-accent-purple-light text-accent-purple border-accent-purple-light',
    transform: 'bg-warning-light text-warning border-warning',
    ai: 'bg-accent-emerald-light text-accent-emerald border-accent-emerald-light',
  }
  const iconMap: Record<string, React.ComponentType<{ className?: string }>> = {
    device: Cpu, extension: Puzzle, transform: Workflow, ai: Brain,
  }
  const Icon = iconMap[type] || Database
  return (
    <Badge variant="outline" className={`${textMini} px-1.5 py-0 h-6 gap-1 ${colorMap[type] || ''}`}>
      <Icon className="h-4 w-4" />
      {type}
    </Badge>
  )
}

function formatTime(timestamp?: number): string {
  if (!timestamp) return '-'
  const ms = timestamp < 1e12 ? timestamp * 1000 : timestamp
  const d = new Date(ms)
  const now = new Date()
  const isToday = d.toDateString() === now.toDateString()
  const pad = (n: number) => String(n).padStart(2, '0')
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  if (isToday) return time
  return `${d.getMonth() + 1}/${d.getDate()} ${time}`
}

function formatDateTime(timestamp: number): string {
  const ms = timestamp < 1e12 ? timestamp * 1000 : timestamp
  const d = new Date(ms)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

export function DataExplorerPage() {
  const { t } = useTranslation(['common', 'data'])
  const isMobile = useIsMobile()
  const { setPushTargetDialogOpen } = useStore()

  // Tab state
  const [activeTab, setActiveTab] = useState<TabValue>('data')

  // Server-side paginated state
  const [pageData, setPageData] = useState<UnifiedDataSourceInfo[]>([])
  const [totalCount, setTotalCount] = useState(0)
  const [sourceOptions, setSourceOptions] = useState<[string, string][]>([])
  const [loading, setLoading] = useState(true)

  // Mobile: track loaded page count for cumulative append
  const [mobileLoadedPages, setMobileLoadedPages] = useState(1)

  // Filters
  const [search, setSearch] = useState('')
  const [selectedSourceName, setSelectedSourceName] = useState<string>('__all__')
  const [page, setPage] = useState(1)
  const pageSize = 10

  // Debounced search value
  const [debouncedSearch, setDebouncedSearch] = useState('')
  const searchTimerRef = useRef<ReturnType<typeof setTimeout>>()
  const updateSearch = useMemo(() => debounce(setDebouncedSearch, 300), [])

  // Detail dialog
  const [selectedSource, setSelectedSource] = useState<UnifiedDataSourceInfo | null>(null)
  const [exportSource, setExportSource] = useState<UnifiedDataSourceInfo | null>(null)
  const [historyRange, setHistoryRange] = useState<string>('1h')
  const [historyData, setHistoryData] = useState<Array<{ timestamp: number; value: unknown; quality: number | null }>>([])
  const [historyLoading, setHistoryLoading] = useState(false)
  // Distinguishes "query failed" from "genuinely no data" — collapsing the two
  // made an unreachable backend look identical to an empty series.
  const [historyError, setHistoryError] = useState(false)
  const [historyRetryTick, setHistoryRetryTick] = useState(0)
  const [historyPage, setHistoryPage] = useState(1)
  const historyPageSize = 10
  const [copiedValue, setCopiedValue] = useState(false)

  // Abort controller for cancelling in-flight requests on unmount
  const abortRef = useRef<AbortController | null>(null)

  // Fetch page from server
  const fetchDataSources = useCallback(async () => {
    // Abort previous in-flight request
    abortRef.current?.abort()
    const controller = new AbortController()
    abortRef.current = controller

    setLoading(true)
    try {
      const params: Record<string, string | number> = {
        offset: (page - 1) * pageSize,
        limit: pageSize,
      }
      if (selectedSourceName !== '__all__') params.source = selectedSourceName
      if (debouncedSearch.trim()) params.search = debouncedSearch.trim()

      const res = await api.listUnifiedDataSources(params)
      if (controller.signal.aborted) return
      const newData = res?.data || []
      if (isMobile && page > 1) {
        // Mobile: accumulate data for infinite scroll
        setPageData(prev => {
          // Deduplicate by id
          const existingIds = new Set(prev.map(d => d.id))
          const unique = newData.filter((d: UnifiedDataSourceInfo) => !existingIds.has(d.id))
          return [...prev, ...unique]
        })
      } else {
        setPageData(newData)
      }
      setMobileLoadedPages(page)
      setTotalCount(res?.total || 0)
      setSourceOptions(res?.source_options || [])
    } catch (err) {
      if (controller.signal.aborted) return
      console.error('[DataExplorer] Failed to fetch data sources:', err)
    } finally {
      if (!controller.signal.aborted) setLoading(false)
    }
  }, [page, selectedSourceName, debouncedSearch, pageSize])

  // Fetch on mount and when filters/page change
  useEffect(() => {
    fetchDataSources()
    return () => { abortRef.current?.abort() }
  }, [fetchDataSources])

  // Reset page when filters change
  useEffect(() => {
    setPage(1)
    setMobileLoadedPages(1)
  }, [debouncedSearch, selectedSourceName])

  // Debounce search input
  useEffect(() => {
    updateSearch(search)
  }, [search, updateSearch])

  // Refresh on device events (debounced to avoid burst refetches)
  const eventFetchRef = useRef<ReturnType<typeof setTimeout>>()
  useEvents({
    enabled: true,
    category: 'device',
    onEvent: () => {
      clearTimeout(eventFetchRef.current)
      eventFetchRef.current = setTimeout(fetchDataSources, 1000)
    },
  })

  // Fetch historical telemetry when a source is selected or range changes
  useEffect(() => {
    if (!selectedSource) {
      setHistoryData([])
      setHistoryLoading(false)
      setHistoryPage(1)
      return
    }
    const rangeSeconds: Record<string, number> = {
      '1h': 3600, '6h': 21600, '24h': 86400, '7d': 604800,
    }
    const now = Math.floor(Date.now() / 1000)
    const start = now - (rangeSeconds[historyRange] || 3600)

    const parts = selectedSource.id.split(':')
    if (parts.length < 3) return
    const source = `${parts[0]}:${parts[1]}`
    const metric = parts.slice(2).join(':')

    let stale = false
    setHistoryLoading(true)
    setHistoryError(false)
    setHistoryPage(1)
    api.queryTelemetry(source, metric, start, now, 500).then(res => {
      if (stale) return
      setHistoryData((res?.data || []).map(p => ({
        timestamp: p.timestamp,
        value: p.value,
        quality: p.quality,
      })))
    }).catch(err => {
      if (stale) return
      console.error('[DataExplorer] Failed to fetch history:', err)
      setHistoryData([])
      setHistoryError(true)
    }).finally(() => {
      if (!stale) setHistoryLoading(false)
    })
    return () => { stale = true }
  }, [selectedSource, historyRange, historyRetryTick])

  const tabs = useMemo(() => [
    { value: 'data', label: t('data:tabs.all', 'Data'), icon: <Database className="h-4 w-4" /> },
    { value: 'push', label: t('data:tabs.push', 'Data Push'), icon: <Send className="h-4 w-4" /> },
  ], [t])

  const columns: TableColumn[] = [
    { key: 'source_display_name', label: t('data:columns.source', 'Source'), width: '18%' },
    { key: 'field_display_name', label: t('data:columns.field', 'Field'), width: '16%' },
    { key: 'unit', label: t('data:unit', 'Unit'), width: '8%' },
    { key: 'source_type', label: t('data:columns.type', 'Type'), width: '10%' },
    { key: 'data_type', label: t('data:columns.dataType', 'Data Type'), width: '10%' },
    { key: 'last_update', label: t('data:columns.updated', 'Updated'), width: '12%' },
    { key: 'actions', label: '', width: '8%' },
  ]

  const renderCell = (columnKey: string, rowData: Record<string, unknown>) => {
    const source = rowData as unknown as UnifiedDataSourceInfo
    switch (columnKey) {
      case 'source_display_name':
        return (
          <span className="text-sm font-medium text-foreground">{source.source_display_name}</span>
        )
      case 'field_display_name':
        return (
          <span className="text-sm font-medium">{source.field_display_name}</span>
        )
      case 'unit':
        return (
          <span className="text-xs text-muted-foreground">{source.unit || '-'}</span>
        )
      case 'source_type':
        return <SourceTypeBadge type={source.source_type} />
      case 'data_type':
        return <Badge variant="secondary" className={textNano}>{source.data_type}</Badge>
      case 'last_update':
        return <span className="text-xs text-muted-foreground">{formatTime(source.last_update)}</span>
      case 'actions':
        return (
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2"
              onClick={(e) => { e.stopPropagation(); setSelectedSource(source) }}
            >
              <Eye className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2"
              onClick={(e) => { e.stopPropagation(); setExportSource(source) }}
            >
              <Download className="h-4 w-4" />
            </Button>
          </div>
        )
      default:
        return String(rowData[columnKey] ?? '')
    }
  }

  // Whether search is pending (user typed but debounce hasn't fired yet)
  const isSearchPending = search !== debouncedSearch

  const dataTable = (
    isMobile ? (
      <div className="space-y-2">
        {pageData.length === 0 && !loading ? (
          <EmptyState
            icon={<Database className="h-12 w-12" />}
            title={search ? t('data:noResults', 'No data sources match your search') : t('data:noSources', 'No data sources available')}
            description={search ? undefined : t('data:noSourcesDesc', 'Data sources will appear here once devices are connected or extensions are registered')}
          />
        ) : pageData.map((source) => (
          <Card
            key={source.id}
            className="overflow-hidden border-border shadow-sm cursor-pointer active:scale-[0.99] transition-all"
            onClick={() => setSelectedSource(source)}
          >
            <div className="px-3 py-2.5">
              {/* Row 1: source + field + eye button */}
              <div className="flex items-center gap-2.5">
                <SourceTypeBadge type={source.source_type} />
                <div className="flex-1 min-w-0">
                  <div className="font-medium text-sm truncate">{source.field_display_name}</div>
                </div>
                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2"
                    onClick={(e) => { e.stopPropagation(); setSelectedSource(source) }}
                  >
                    <Eye className="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2"
                    onClick={(e) => { e.stopPropagation(); setExportSource(source) }}
                  >
                    <Download className="h-4 w-4" />
                  </Button>
                </div>
              </div>
              {/* Row 2: ID + data type + time */}
              <div className="flex items-center gap-1.5 mt-1.5">
                <code className={cn(textMini, "text-muted-foreground font-mono truncate flex-1")}>{source.id}</code>
                <Badge variant="secondary" className={cn(textNano, "h-5 px-1.5")}>{source.data_type}</Badge>
                <span className={cn(textMini, "text-muted-foreground")}>{formatTime(source.last_update)}</span>
              </div>
            </div>
          </Card>
        ))}
      </div>
    ) : (
    <ResponsiveTable
      columns={columns}
      data={pageData as unknown as Record<string, unknown>[]}
      renderCell={renderCell}
      rowKey={(row) => (row as unknown as UnifiedDataSourceInfo).id}
      onRowClick={(row) => setSelectedSource(row as unknown as UnifiedDataSourceInfo)}
      loading={loading}
      flexHeight
      className={isSearchPending ? 'opacity-60 transition-opacity duration-normal' : undefined}
      emptyState={
        <EmptyState
          icon={<Database className="h-12 w-12" />}
          title={search ? t('data:noResults', 'No data sources match your search') : t('data:noSources', 'No data sources available')}
          description={search ? undefined : t('data:noSourcesDesc', 'Data sources will appear here once devices are connected or extensions are registered')}
        />
      }
    />
    )
  )

  const sourceFilter = sourceOptions.length > 1 ? (
    <Select value={selectedSourceName} onValueChange={setSelectedSourceName}>
      <SelectTrigger className="w-[160px] md:w-[200px] h-9 text-sm">
        <SelectValue placeholder={t('data:filterSource', 'Filter source...')} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="__all__">{t('data:allSources', 'All Sources')}</SelectItem>
        {sourceOptions.map(([name, displayName]) => (
          <SelectItem key={name} value={name}>{displayName}</SelectItem>
        ))}
      </SelectContent>
    </Select>
  ) : null

  const formatHistoryValue = (val: unknown): string => {
    if (val === null || val === undefined) return '-'
    if (typeof val === 'object') return JSON.stringify(val)
    return String(val)
  }

  return (
    <>
      <PageLayout
        title={t('data:title', 'Data Explorer')}
        subtitle={t('data:subtitle', 'Browse all data sources across devices, extensions, and transforms')}
        hideFooterOnMobile
        hasBottomNav
        headerContent={
          <PageTabsBar
            tabs={tabs}
            activeTab={activeTab}
            onTabChange={(v) => setActiveTab(v as TabValue)}
            actions={
              activeTab === 'push'
                ? [{
                    label: t('common:dataPush.create', 'Create Target'),
                    icon: <Plus className="h-4 w-4" />,
                    variant: 'outline' as const,
                    onClick: () => setPushTargetDialogOpen(true),
                  }]
                : []
            }
            actionsExtra={
              activeTab === 'data' ? (
                <div className="flex items-center gap-2">
                  {sourceFilter}
                  <div className="relative">
                    <span className="absolute left-2.5 top-0 bottom-0 flex items-center">
                      {isSearchPending ? (
                        <Loader2 className="h-4 w-4 text-muted-foreground animate-spin" />
                      ) : (
                        <Search className="h-4 w-4 text-muted-foreground" />
                      )}
                    </span>
                    <Input
                      placeholder={t('data:search', 'Search data sources...')}
                      value={search}
                      onChange={e => setSearch(e.target.value)}
                      className="pl-9 w-[180px] md:w-[240px] h-9"
                      autoFocus
                    />
                  </div>
                </div>
              ) : undefined
            }
          />
        }
        footer={
          activeTab === 'data' && totalCount > pageSize ? (
            <Pagination
              total={totalCount}
              pageSize={pageSize}
              currentPage={page}
              onPageChange={setPage}
              isLoading={loading}
            />
          ) : undefined
        }
      >
        <PageTabsContent value="data" activeTab={activeTab}>
          {dataTable}
        </PageTabsContent>
        <PageTabsContent value="push" activeTab={activeTab}>
          <PushTargetsTab />
        </PageTabsContent>
      </PageLayout>

      <PageTabsBottomNav
        tabs={tabs}
        activeTab={activeTab}
        onTabChange={(v) => setActiveTab(v as TabValue)}
      />

      <FullScreenDialog
        open={!!selectedSource}
        onOpenChange={(open) => !open && setSelectedSource(null)}
      >
        <FullScreenDialogHeader
          icon={<Database className="h-5 w-5" />}
          iconBg="bg-info-light"
          iconColor="text-info"
          title={selectedSource?.field_display_name || ''}
          subtitle={`${selectedSource?.source_display_name || ''} · ${selectedSource?.source_type || ''}`}
          onClose={() => setSelectedSource(null)}
        />
        <FullScreenDialogContent>
          <FullScreenDialogMain className="p-4 md:p-6 lg:p-8">
            {selectedSource && (
              <div className="max-w-4xl mx-auto space-y-6">
                {/* Tier 1: Current Value (prominent) */}
                <div className="rounded-xl border p-5">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      {(() => {
                        const v = selectedSource.current_value
                        if (v === undefined || v === null) {
                          return <span className="text-sm text-muted-foreground">{t('data:noData', 'No current data')}</span>
                        }
                        // Image base64
                        if (typeof v === 'string' && isBase64Image(v)) {
                          return <img src={getImageDataUrl(v) ?? undefined} alt="metric" className="max-h-32 rounded-lg object-contain" />
                        }
                        // Object/JSON: clamp to 5 lines + copy full value
                        if (typeof v === 'object') {
                          const jsonText = JSON.stringify(v, null, 2)
                          const overflows = jsonText.split('\n').length > 5
                          return (
                            <div className="space-y-1">
                              <div className="flex items-start gap-2">
                                <pre className={cn(
                                  "flex-1 min-w-0 font-mono text-sm whitespace-pre-wrap break-all",
                                  overflows && "line-clamp-5"
                                )}>
                                  {jsonText}
                                </pre>
                                {overflows && (
                                  <button
                                    type="button"
                                    onClick={async () => {
                                      try { await copyToClipboard(jsonText); setCopiedValue(true); setTimeout(() => setCopiedValue(false), 2000) } catch { /* clipboard unavailable */ }
                                    }}
                                    className="shrink-0 mt-0.5 text-muted-foreground hover:text-foreground"
                                    title={t('common:copy', 'Copy')}
                                  >
                                    {copiedValue ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
                                  </button>
                                )}
                              </div>
                            </div>
                          )
                        }
                        // String scalar: short → prominent, long/base64 → clamped code block + copy
                        const str = String(v)
                        const isLong = str.length > 80 || str.includes('\n')
                        if (isLong) {
                          const overflows = str.length > 200 || str.split('\n').length > 3
                          return (
                            <div className="space-y-1">
                              <div className="flex items-start gap-2">
                                <pre className={cn(
                                  "flex-1 min-w-0 font-mono text-sm whitespace-pre-wrap break-all",
                                  overflows && "line-clamp-3"
                                )}>
                                  {str}
                                </pre>
                                {overflows && (
                                  <button
                                    type="button"
                                    onClick={async () => {
                                      try { await copyToClipboard(str); setCopiedValue(true); setTimeout(() => setCopiedValue(false), 2000) } catch { /* clipboard unavailable */ }
                                    }}
                                    className="shrink-0 mt-0.5 text-muted-foreground hover:text-foreground"
                                    title={t('common:copy', 'Copy')}
                                  >
                                    {copiedValue ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
                                  </button>
                                )}
                              </div>
                            </div>
                          )
                        }
                        // Short scalar: prominent display
                        return (
                          <div className="flex items-baseline gap-2 flex-wrap">
                            <span className="font-mono text-2xl md:text-3xl font-semibold break-all">{str}</span>
                            {selectedSource.unit && (
                              <span className="font-mono text-sm text-muted-foreground">{selectedSource.unit}</span>
                            )}
                          </div>
                        )
                      })()}
                    </div>
                    {selectedSource.data_type && (
                      <Badge variant="secondary" className={cn(textNano, "shrink-0")}>{selectedSource.data_type}</Badge>
                    )}
                  </div>
                  <div className="mt-3 flex items-center gap-1.5 text-sm text-muted-foreground">
                    <Clock className="h-3.5 w-3.5" />
                    <span>{t('data:lastUpdate', 'Last Update')} · {formatTime(selectedSource.last_update)}</span>
                  </div>
                  {selectedSource.description && (
                    <p className="mt-2 text-sm text-muted-foreground">{selectedSource.description}</p>
                  )}
                </div>

                {/* Tier 2: History (main body) */}
                <div>
                  <div className="flex items-center justify-between gap-2 mb-3">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      <History className="h-4 w-4" />
                      {t('data:history', 'History')}
                      {historyData.length > 0 && (
                        <Badge variant="secondary" className={cn(textNano, "ml-1")}>{historyData.length}</Badge>
                      )}
                    </div>
                    <Select value={historyRange} onValueChange={setHistoryRange}>
                      <SelectTrigger className="w-[140px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="1h">{t('data:range.1h', '1 Hour')}</SelectItem>
                        <SelectItem value="6h">{t('data:range.6h', '6 Hours')}</SelectItem>
                        <SelectItem value="24h">{t('data:range.24h', '24 Hours')}</SelectItem>
                        <SelectItem value="7d">{t('data:range.7d', '7 Days')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  {historyLoading ? (
                    <div className="flex items-center justify-center h-32 text-muted-foreground">
                      <Loader2 className="h-4 w-4 animate-spin mr-2" />
                      <span className="text-xs">{t('common:loading', 'Loading...')}</span>
                    </div>
                  ) : historyData.length > 0 ? (() => {
                    // Hide the Quality column when no row has a non-null value — it's
                    // usually empty in practice and wastes horizontal space.
                    const hasQuality = historyData.some(p => p.quality !== null)
                    const columns: TableColumn[] = [
                      { key: 'timestamp', label: t('data:timestamp', 'Timestamp'), width: '180px' },
                      { key: 'value', label: t('data:value', 'Value') },
                      ...(hasQuality ? [{ key: 'quality', label: t('data:quality', 'Quality'), width: '80px', align: 'right' as const }] : []),
                    ]
                    // Backend returns telemetry ascending (oldest first) — reverse so
                    // page 1 shows the newest records.
                    const orderedHistory = [...historyData].sort((a, b) => b.timestamp - a.timestamp)
                    const startIdx = (historyPage - 1) * historyPageSize
                    const pagedHistoryData = orderedHistory.slice(startIdx, startIdx + historyPageSize)
                    return (
                      <>
                      <ResponsiveTable
                        columns={columns}
                        data={pagedHistoryData as unknown as Record<string, unknown>[]}
                        rowKey={(row) => String((row as { timestamp: number }).timestamp)}
                        renderCell={(columnKey, rowData) => {
                          const point = rowData as { timestamp: number; value: unknown; quality: number | null }
                          switch (columnKey) {
                            case 'timestamp':
                              return (
                                <span className="font-mono text-xs text-muted-foreground whitespace-nowrap">
                                  {formatTimestamp(point.timestamp)}
                                </span>
                              )
                            case 'value':
                              if (typeof point.value === 'string' && isBase64Image(point.value)) {
                                return (
                                  <img
                                    src={getImageDataUrl(point.value) ?? undefined}
                                    alt="metric"
                                    className="h-10 w-10 object-cover rounded shrink-0"
                                  />
                                )
                              }
                              return (
                                <span
                                  className="font-mono text-xs truncate min-w-0"
                                  title={formatHistoryValue(point.value)}
                                >
                                  {formatHistoryValue(point.value)}
                                </span>
                              )
                            case 'quality':
                              return (
                                <span className="font-mono text-xs text-muted-foreground text-right">
                                  {point.quality !== null ? (point.quality * 100).toFixed(0) + '%' : '—'}
                                </span>
                              )
                            default:
                              return null
                          }
                        }}
                      />
                      {historyData.length > historyPageSize && (
                        <div className="mt-3 flex justify-center">
                          <Pagination
                            total={historyData.length}
                            pageSize={historyPageSize}
                            currentPage={historyPage}
                            onPageChange={setHistoryPage}
                            isLoading={historyLoading}
                            hideOnMobile={false}
                          />
                        </div>
                      )}
                      </>
                    )
                  })() : historyError ? (
                    <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
                      <AlertTriangle className="h-8 w-8 mb-2 text-warning" />
                      <p className="text-xs">{t('data:historyLoadFailed', 'Failed to load historical data')}</p>
                      <Button
                        variant="outline"
                        size="sm"
                        className="mt-3"
                        onClick={() => setHistoryRetryTick(t => t + 1)}
                      >
                        <RefreshCw className="h-4 w-4 mr-1" />
                        {t('data:retry', 'Retry')}
                      </Button>
                    </div>
                  ) : (
                    <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
                      <History className="h-8 w-8 mb-2 opacity-30" />
                      <p className="text-xs">{t('data:noHistory', 'No historical data available for this period')}</p>
                    </div>
                  )}
                </div>
              </div>
            )}
          </FullScreenDialogMain>
        </FullScreenDialogContent>
      </FullScreenDialog>

      <ExportDataDialog
        open={!!exportSource}
        onOpenChange={(open) => !open && setExportSource(null)}
        source={exportSource}
      />
    </>
  )
}

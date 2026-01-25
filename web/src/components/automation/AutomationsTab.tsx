import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import type { Automation, AutomationType } from '@/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { LoadingState, EmptyState } from '@/components/shared'
import { AutomationCreatorDialog, AutomationConverterDialog } from '@/components/automation'
import { formatTimestamp } from '@/lib/utils/format'
import { useToast } from '@/hooks/use-toast'
import { confirm } from '@/hooks/use-confirm'
import {
  RefreshCw,
  Plus,
  Search,
  Play,
  Edit,
  Trash2,
  ArrowRightLeft,
  MoreVertical,
  Sparkles,
} from 'lucide-react'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Switch } from '@/components/ui/switch'

export interface AutomationsTabProps {
  searchQuery?: string
  onSearchChange?: (query: string) => void
}

type FilterType = 'all' | 'rule' | 'transform'
type StatusFilter = 'all' | 'enabled' | 'disabled'

export function AutomationsTab({ searchQuery: externalSearchQuery, onSearchChange }: AutomationsTabProps) {
  const { t } = useTranslation(['automation', 'common'])
  const { toast } = useToast()

  // Data state
  const [automations, setAutomations] = useState<Automation[]>([])
  const [loading, setLoading] = useState(true)

  // Filter state
  const [typeFilter, setTypeFilter] = useState<FilterType>('all')
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all')
  const [searchQuery, setSearchQuery] = useState(externalSearchQuery || '')

  // Dialog state
  const [creatorOpen, setCreatorOpen] = useState(false)
  const [converterOpen, setConverterOpen] = useState(false)
  const [selectedAutomation, setSelectedAutomation] = useState<Automation | null>(null)

  // Sync external search query
  useEffect(() => {
    if (externalSearchQuery !== undefined) {
      setSearchQuery(externalSearchQuery)
    }
  }, [externalSearchQuery])

  const loadAutomations = async () => {
    setLoading(true)
    try {
      const response = await api.listAutomations({
        type: typeFilter === 'all' ? undefined : typeFilter,
        enabled: statusFilter === 'all' ? undefined : statusFilter === 'enabled',
        search: searchQuery || undefined,
      })
      setAutomations(response.automations || [])
    } catch (error) {
      console.error('Failed to load automations:', error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    loadAutomations()
  }, [typeFilter, statusFilter, searchQuery])

  const handleToggleEnabled = async (automation: Automation) => {
    const newEnabled = !automation.enabled
    try {
      await api.setAutomationStatus(automation.id, newEnabled)
      toast({
        title: t('common:success'),
        description: newEnabled ? t('automation:enabled') : t('automation:disabled'),
      })
      loadAutomations()
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleExecute = async (automation: Automation) => {
    try {
      if (automation.type === 'rule') {
        await api.testRule(automation.id)
      }
      toast({
        title: t('common:success'),
        description: t('automation:executed'),
      })
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleDelete = async (automation: Automation) => {
    const confirmed = await confirm({
      title: t('common:delete'),
      description: t('automation:deleteConfirm'),
      confirmText: t('common:delete'),
      cancelText: t('common:cancel'),
      variant: "destructive"
    })
    if (!confirmed) return

    try {
      await api.deleteAutomation(automation.id)
      toast({
        title: t('common:success'),
        description: t('automation:deleted'),
      })
      loadAutomations()
    } catch (error) {
      toast({
        title: t('common:failed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    }
  }

  const handleConvert = (automation: Automation) => {
    setSelectedAutomation(automation)
    setConverterOpen(true)
  }

  const handleConversionComplete = (_newId: string, _newType: AutomationType) => {
    setConverterOpen(false)
    setSelectedAutomation(null)
    toast({
      title: t('common:success'),
      description: t('automation:conversionComplete', { defaultValue: 'Automation converted successfully' }),
    })
    loadAutomations()
  }

  const handleSearchChange = (value: string) => {
    setSearchQuery(value)
    onSearchChange?.(value)
  }

  const getTypeLabel = (type: AutomationType) =>
    type === 'rule' ? t('automation:rules') : t('automation:transforms')

  const getTypeColor = (type: AutomationType) =>
    type === 'rule'
      ? 'bg-blue-100 text-blue-700 border-blue-200 dark:bg-blue-900 dark:text-blue-300'
      : 'bg-green-100 text-green-700 border-green-200 dark:bg-green-900 dark:text-green-300'

  const getComplexityDots = (complexity: number) => {
    return Array.from({ length: 5 }, (_, i) => (
      <span
        key={i}
        className={`w-2 h-2 rounded-full ${
          i < complexity ? 'bg-yellow-500' : 'bg-gray-300 dark:bg-gray-600'
        }`}
      />
    ))
  }

  return (
    <>
      {/* Header */}
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4 mb-6">
        <div className="flex flex-wrap items-center gap-3">
          <Select value={typeFilter} onValueChange={(v) => setTypeFilter(v as FilterType)}>
            <SelectTrigger className="w-[140px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('automation:all')}</SelectItem>
              <SelectItem value="rule">{t('automation:rules')}</SelectItem>
              <SelectItem value="transform">{t('automation:transforms')}</SelectItem>
            </SelectContent>
          </Select>

          <Select value={statusFilter} onValueChange={(v) => setStatusFilter(v as StatusFilter)}>
            <SelectTrigger className="w-[140px]">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t('automation:all')}</SelectItem>
              <SelectItem value="enabled">{t('automation:enabled')}</SelectItem>
              <SelectItem value="disabled">{t('automation:disabled')}</SelectItem>
            </SelectContent>
          </Select>

          <div className="relative">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              placeholder={t('automation:searchPlaceholder', { defaultValue: 'Search...' })}
              value={searchQuery}
              onChange={(e) => handleSearchChange(e.target.value)}
              className="pl-8 w-[200px]"
            />
          </div>
        </div>

        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={loadAutomations}>
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button size="sm" onClick={() => setCreatorOpen(true)}>
            <Plus className="h-4 w-4 mr-1" />
            {t('automation:createAutomation')}
          </Button>
        </div>
      </div>

      {/* Content */}
      {loading ? (
        <LoadingState text={t('automation:loading')} />
      ) : !automations || automations.length === 0 ? (
        <EmptyState
          icon={<Sparkles className="h-12 w-12 text-muted-foreground" />}
          title={t('automation:noAutomations', { defaultValue: 'No automations' })}
          description={t('automation:noAutomationsDesc', {
            defaultValue: 'Create your first automation using AI, templates, or manual configuration',
          })}
          action={{
            label: t('automation:createAutomation'),
            onClick: () => setCreatorOpen(true),
          }}
        />
      ) : (
        <Card>
          <CardContent className="p-0">
            <div className="overflow-x-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-[40px]"></TableHead>
                    <TableHead>{t('automation:automationName')}</TableHead>
                    <TableHead>{t('automation:recommendedType')}</TableHead>
                    <TableHead>{t('automation:complexity')}</TableHead>
                    <TableHead>{t('automation:status')}</TableHead>
                    <TableHead>{t('automation:executionCount')}</TableHead>
                    <TableHead>{t('automation:updatedAt')}</TableHead>
                    <TableHead className="text-right">{t('automation:actions')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {automations.map((automation) => (
                    <TableRow key={automation.id}>
                      <TableCell>
                        <Switch
                          checked={automation.enabled}
                          onCheckedChange={() => handleToggleEnabled(automation)}
                        />
                      </TableCell>
                      <TableCell>
                        <div>
                          <p className="font-medium">{automation.name}</p>
                          <p className="text-sm text-muted-foreground line-clamp-1">
                            {automation.description}
                          </p>
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge variant="outline" className={getTypeColor(automation.type)}>
                          {getTypeLabel(automation.type)}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="flex gap-0.5">{getComplexityDots(automation.complexity)}</div>
                      </TableCell>
                      <TableCell>
                        <Badge variant={automation.enabled ? 'default' : 'secondary'}>
                          {automation.enabled ? t('automation:enabled') : t('automation:disabled')}
                        </Badge>
                      </TableCell>
                      <TableCell>{automation.execution_count}</TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {formatTimestamp(automation.updated_at)}
                      </TableCell>
                      <TableCell className="text-right">
                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button variant="ghost" size="sm">
                              <MoreVertical className="h-4 w-4" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="end">
                            <DropdownMenuItem onClick={() => handleExecute(automation)}>
                              <Play className="h-4 w-4 mr-2" />
                              {t('automation:execute')}
                            </DropdownMenuItem>
                            <DropdownMenuItem>
                              <Edit className="h-4 w-4 mr-2" />
                              {t('automation:edit')}
                            </DropdownMenuItem>
                            <DropdownMenuItem onClick={() => handleConvert(automation)}>
                              <ArrowRightLeft className="h-4 w-4 mr-2" />
                              {t('automation:convertAutomation')}
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onClick={() => handleDelete(automation)}
                              className="text-destructive"
                            >
                              <Trash2 className="h-4 w-4 mr-2" />
                              {t('automation:delete')}
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          </CardContent>
        </Card>
      )}

      {/* Dialogs */}
      <AutomationCreatorDialog
        open={creatorOpen}
        onOpenChange={setCreatorOpen}
        onAutomationCreated={loadAutomations}
      />

      {selectedAutomation && (
        <AutomationConverterDialog
          open={converterOpen}
          onOpenChange={setConverterOpen}
          automationId={selectedAutomation.id}
          automationName={selectedAutomation.name}
          currentType={selectedAutomation.type}
          onConversionComplete={handleConversionComplete}
        />
      )}
    </>
  )
}

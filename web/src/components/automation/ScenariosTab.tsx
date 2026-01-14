import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Textarea } from '@/components/ui/textarea'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Plus, Trash2, Edit, Play, Home } from 'lucide-react'
import { ActionBar, EmptyState } from '@/components/shared'
import { api } from '@/lib/api'
import type { Scenario, ScenarioAction } from '@/types'
import { cn } from '@/lib/utils'

interface ScenariosTabProps {
  onRefresh?: () => void
}

const SCENARIO_ICONS: Record<string, string> = {
  '回家模式': '🏠',
  '离家模式': '🚪',
  '睡眠模式': '💤',
  '起床模式': '☀️',
  '观影模式': '🎬',
  '阅读模式': '📖',
  '会客模式': '👋',
  '节能模式': '⚡',
}

export function ScenariosTab({ onRefresh }: ScenariosTabProps) {
  const { t } = useTranslation(['automation', 'common'])
  const [scenarios, setScenarios] = useState<Scenario[]>([])
  const [loading, setLoading] = useState(true)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editScenario, setEditScenario] = useState<Scenario | null>(null)
  const [newScenarioName, setNewScenarioName] = useState('')
  const [newScenarioIcon, setNewScenarioIcon] = useState('🏠')
  const [newScenarioActions, setNewScenarioActions] = useState('')

  const fetchScenarios = async () => {
    setLoading(true)
    try {
      const result = await api.listScenarios()
      setScenarios(result.scenarios || [])
    } catch (error) {
      console.error('Failed to fetch scenarios:', error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchScenarios()
  }, [])

  const handleToggleScenario = async (scenario: Scenario) => {
    try {
      if (scenario.active) {
        await api.deactivateScenario(scenario.id)
      } else {
        await api.activateScenario(scenario.id)
      }
      await fetchScenarios()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to toggle scenario:', error)
    }
  }

  const handleDeleteScenario = async (id: string) => {
    if (!confirm(t('automation:deleteConfirm'))) return
    try {
      await api.deleteScenario(id)
      await fetchScenarios()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to delete scenario:', error)
    }
  }

  const handleActivateScenario = async (id: string) => {
    try {
      await api.activateScenario(id)
      await fetchScenarios()
    } catch (error) {
      console.error('Failed to activate scenario:', error)
    }
  }

  const handleCreateScenario = async () => {
    if (!newScenarioName.trim()) return
    try {
      // Parse actions from simple format
      const actions: ScenarioAction[] = newScenarioActions
        .split('\n')
        .filter(line => line.trim())
        .map((line, i) => ({
          id: `action-${Date.now()}-${i}`,
          type: 'device_command' as const,
          device_id: '',
          command: line,
        }))

      await api.createScenario({
        name: newScenarioName,
        icon: newScenarioIcon,
        actions,
        enabled: true,
        active: false,
      })
      setCreateDialogOpen(false)
      setNewScenarioName('')
      setNewScenarioIcon('🏠')
      setNewScenarioActions('')
      await fetchScenarios()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to create scenario:', error)
    }
  }

  const handleEditScenario = async () => {
    if (!editScenario) return
    try {
      await api.updateScenario(editScenario.id, {
        name: editScenario.name,
        icon: editScenario.icon,
        actions: editScenario.actions,
      })
      setEditScenario(null)
      await fetchScenarios()
    } catch (error) {
      console.error('Failed to update scenario:', error)
    }
  }

  const getActionIcon = (action: ScenarioAction) => {
    switch (action.type) {
      case 'device_command': return '⚡'
      case 'scene': return '🎬'
      case 'delay': return '⏱️'
      case 'notification': return '🔔'
      default: return '📄'
    }
  }

  const getActionDescription = (action: ScenarioAction) => {
    switch (action.type) {
      case 'device_command':
        return action.device_id
          ? `${action.device_id}: ${action.command}`
          : action.command || t('automation:execute')
      case 'scene':
        return `${t('automation:scenario')}: ${action.device_id}`
      case 'delay':
        return `${t('automation:delay')} ${action.delay_ms ? Math.round(action.delay_ms / 1000) : 0}s`
      case 'notification':
        return t('automation:notification')
      default:
        return t('automation:actions')
    }
  }

  const presetScenarios = [
    { name: '回家模式', icon: '🏠', description: '打开灯光、调节空调至舒适温度' },
    { name: '离家模式', icon: '🚪', description: '关闭所有设备、开启安防' },
    { name: '睡眠模式', icon: '💤', description: '关闭灯光、调高空调、静音' },
    { name: '起床模式', icon: '☀️', description: '渐亮灯光、播放轻音乐、调节温度' },
  ]

  return (
    <>
      {/* Header with actions */}
      <ActionBar
        title={t('automation:scenariosTitle')}
        titleIcon={<Home className="h-5 w-5" />}
        description={t('automation:scenariosDesc')}
        actions={[
          {
            label: t('automation:scenariosAdd'),
            icon: <Plus className="h-4 w-4" />,
            onClick: () => setCreateDialogOpen(true),
          },
        ]}
        onRefresh={onRefresh}
      />

      {/* Preset Scenarios */}
      {scenarios.length === 0 && !loading && (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
          {presetScenarios.map((preset) => (
            <Card
              key={preset.name}
              className="cursor-pointer hover:border-primary/50 transition-colors"
              onClick={() => {
                setNewScenarioName(preset.name)
                setNewScenarioIcon(preset.icon)
                setCreateDialogOpen(true)
              }}
            >
              <CardContent className="p-4 text-center">
                <div className="text-4xl mb-2">{preset.icon}</div>
                <div className="font-medium">{preset.name}</div>
                <div className="text-xs text-muted-foreground mt-1">{preset.description}</div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}

      {/* Scenario Cards */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
        {loading ? (
          <div className="col-span-full flex items-center justify-center py-12">
            <p className="text-muted-foreground">{t('automation:loading')}</p>
          </div>
        ) : scenarios.length === 0 ? (
          <div className="col-span-full">
            <EmptyState
              icon="scenario"
              title={t('automation:noScenarios')}
              description={t('automation:scenariosEmptyHint')}
            />
          </div>
        ) : (
          scenarios.map((scenario) => (
            <Card
              key={scenario.id}
              className={cn(
                'cursor-pointer transition-all',
                scenario.active && 'ring-2 ring-primary',
                !scenario.enabled && 'opacity-50'
              )}
            >
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between">
                  <div className="flex items-center gap-2">
                    <span className="text-2xl">{scenario.icon || SCENARIO_ICONS[scenario.name] || '🎬'}</span>
                    <CardTitle className="text-base">{scenario.name}</CardTitle>
                  </div>
                  <Switch
                    checked={scenario.enabled}
                    onCheckedChange={() => handleToggleScenario(scenario)}
                  />
                </div>
              </CardHeader>
              <CardContent className="space-y-3">
                {/* Actions preview */}
                <div className="space-y-1">
                  {scenario.actions.slice(0, 3).map((action, i) => (
                    <div key={i} className="flex items-center gap-2 text-xs">
                      <span>{getActionIcon(action)}</span>
                      <span className="truncate text-muted-foreground">
                        {getActionDescription(action)}
                      </span>
                    </div>
                  ))}
                  {scenario.actions.length > 3 && (
                    <div className="text-xs text-muted-foreground">
                      {t('automation:moreActions', { count: scenario.actions.length - 3 })}
                    </div>
                  )}
                </div>

                {/* Status and Actions */}
                <div className="flex items-center gap-2 pt-2 border-t">
                  <Button
                    variant={scenario.active ? 'default' : 'outline'}
                    size="sm"
                    className="flex-1"
                    onClick={() => handleActivateScenario(scenario.id)}
                    disabled={!scenario.enabled || scenario.active}
                  >
                    <Play className="h-3 w-3 mr-1" />
                    {scenario.active ? t('automation:scenariosActive') : t('automation:scenariosExecute')}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setEditScenario(scenario)}
                  >
                    <Edit className="h-3 w-3" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleDeleteScenario(scenario.id)}
                  >
                    <Trash2 className="h-3 w-3 text-destructive" />
                  </Button>
                </div>

                {/* Active badge */}
                {scenario.active && (
                  <Badge className="w-full justify-center bg-green-500">
                    {t('automation:scenariosActive')}
                  </Badge>
                )}
              </CardContent>
            </Card>
          ))
        )}
      </div>

      {/* Create Scenario Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('automation:createScenario')}</DialogTitle>
            <DialogDescription>
              {t('automation:scenariosDesc')}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="scenario-name">{t('automation:scenarioName')}</Label>
              <Input
                id="scenario-name"
                value={newScenarioName}
                onChange={(e) => setNewScenarioName(e.target.value)}
                placeholder={t('automation:scenarioNamePlaceholder')}
              />
            </div>
            <div>
              <Label>{t('automation:scenarioIcon')}</Label>
              <div className="flex gap-2 mt-2">
                {['🏠', '🚪', '💤', '☀️', '🎬', '📖', '⚡', '🔔'].map((icon) => (
                  <button
                    key={icon}
                    type="button"
                    onClick={() => setNewScenarioIcon(icon)}
                    className={cn(
                      'w-10 h-10 rounded-lg border-2 flex items-center justify-center text-xl transition-all',
                      newScenarioIcon === icon
                        ? 'border-primary bg-primary/10'
                        : 'border-transparent hover:border-muted-foreground/30'
                    )}
                  >
                    {icon}
                  </button>
                ))}
              </div>
            </div>
            <div>
              <Label htmlFor="scenario-actions">{t('automation:actions')}</Label>
              <Textarea
                id="scenario-actions"
                value={newScenarioActions}
                onChange={(e) => setNewScenarioActions(e.target.value)}
                placeholder={t('automation:scenarioActionsPlaceholder')}
                className="font-mono text-sm min-h-[120px]"
              />
              <p className="text-xs text-muted-foreground mt-1">
                {t('automation:scenarioActionsHint')}
              </p>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
                {t('automation:cancel')}
              </Button>
              <Button onClick={handleCreateScenario} disabled={!newScenarioName}>
                {t('automation:createScenario')}
              </Button>
            </DialogFooter>
          </div>
        </DialogContent>
      </Dialog>

      {/* Edit Scenario Dialog */}
      <Dialog open={!!editScenario} onOpenChange={() => setEditScenario(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('automation:edit')}</DialogTitle>
            <DialogDescription>
              {t('automation:editActionsHint')}
            </DialogDescription>
          </DialogHeader>
          {editScenario && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="edit-scenario-name">{t('automation:scenarioName')}</Label>
                <Input
                  id="edit-scenario-name"
                  value={editScenario.name}
                  onChange={(e) => setEditScenario({ ...editScenario, name: e.target.value })}
                />
              </div>
              <div>
                <Label>{t('automation:scenarioIcon')}</Label>
                <div className="flex gap-2 mt-2">
                  {['🏠', '🚪', '💤', '☀️', '🎬', '📖', '⚡', '🔔'].map((icon) => (
                    <button
                      key={icon}
                      type="button"
                      onClick={() => setEditScenario({ ...editScenario, icon })}
                      className={cn(
                        'w-10 h-10 rounded-lg border-2 flex items-center justify-center text-xl transition-all',
                        editScenario.icon === icon
                          ? 'border-primary bg-primary/10'
                          : 'border-transparent hover:border-muted-foreground/30'
                      )}
                    >
                      {icon}
                    </button>
                  ))}
                </div>
              </div>
              <div>
                <Label>{t('automation:actions')}</Label>
                <Textarea
                  value={editScenario.actions.map(a => {
                    if (a.type === 'device_command') {
                      return a.device_id ? `${a.device_id}.${a.command}` : a.command
                    }
                    return ''
                  }).join('\n')}
                  readOnly
                  className="font-mono text-sm min-h-[100px] bg-muted"
                />
                <p className="text-xs text-muted-foreground mt-1">
                  {t('automation:editActionsHint')}
                </p>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setEditScenario(null)}>
                  {t('automation:cancel')}
                </Button>
                <Button onClick={handleEditScenario}>
                  {t('automation:saveChanges')}
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  )
}

import { useState, useEffect } from 'react'
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
    if (!confirm('确定要删除这个场景吗？')) return
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
          : action.command || '执行命令'
      case 'scene':
        return `切换场景: ${action.device_id}`
      case 'delay':
        return `延迟 ${action.delay_ms ? Math.round(action.delay_ms / 1000) : 0} 秒`
      case 'notification':
        return '发送通知'
      default:
        return '未知操作'
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
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-xl font-semibold">场景</h2>
          <p className="text-sm text-muted-foreground">
            一键执行多个设备操作
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateDialogOpen(true)}>
          <Plus className="h-4 w-4 mr-2" />
          新建场景
        </Button>
      </div>

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
          <div className="col-span-full py-8 text-center text-muted-foreground">
            加载中...
          </div>
        ) : scenarios.length === 0 ? (
          <div className="col-span-full py-8">
            <div className="flex flex-col items-center gap-3">
              <Home className="h-12 w-12 text-muted-foreground/50" />
              <p className="text-muted-foreground">暂无场景</p>
              <p className="text-xs text-muted-foreground">选择上方预设场景或创建新场景</p>
            </div>
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
                      还有 {scenario.actions.length - 3} 个操作...
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
                    {scenario.active ? '激活中' : '执行'}
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
                    当前激活
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
            <DialogTitle>创建新场景</DialogTitle>
            <DialogDescription>
              创建一键执行多个设备操作的场景
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="scenario-name">场景名称</Label>
              <Input
                id="scenario-name"
                value={newScenarioName}
                onChange={(e) => setNewScenarioName(e.target.value)}
                placeholder="例如: 回家模式"
              />
            </div>
            <div>
              <Label>图标</Label>
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
              <Label htmlFor="scenario-actions">操作 (每行一个)</Label>
              <Textarea
                id="scenario-actions"
                value={newScenarioActions}
                onChange={(e) => setNewScenarioActions(e.target.value)}
                placeholder={`客厅灯.开关=on
空调.温度=26
窗帘.关闭`}
                className="font-mono text-sm min-h-[120px]"
              />
              <p className="text-xs text-muted-foreground mt-1">
                格式: 设备名.命令=值
              </p>
            </div>
            <DialogFooter>
              <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
                取消
              </Button>
              <Button onClick={handleCreateScenario} disabled={!newScenarioName}>
                创建场景
              </Button>
            </DialogFooter>
          </div>
        </DialogContent>
      </Dialog>

      {/* Edit Scenario Dialog */}
      <Dialog open={!!editScenario} onOpenChange={() => setEditScenario(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>编辑场景</DialogTitle>
            <DialogDescription>
              修改场景配置
            </DialogDescription>
          </DialogHeader>
          {editScenario && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="edit-scenario-name">场景名称</Label>
                <Input
                  id="edit-scenario-name"
                  value={editScenario.name}
                  onChange={(e) => setEditScenario({ ...editScenario, name: e.target.value })}
                />
              </div>
              <div>
                <Label>图标</Label>
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
                <Label>操作</Label>
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
                  编辑操作功能即将推出
                </p>
              </div>
              <DialogFooter>
                <Button variant="outline" onClick={() => setEditScenario(null)}>
                  取消
                </Button>
                <Button onClick={handleEditScenario}>
                  保存修改
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>
    </>
  )
}

import { useState, useEffect } from 'react'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
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
import { Plus, Trash2, Edit, Play, Workflow as WorkflowIcon, ArrowRight } from 'lucide-react'
import { api } from '@/lib/api'
import type { Workflow, WorkflowStep } from '@/types'
import { cn } from '@/lib/utils'

interface WorkflowsTabProps {
  onRefresh?: () => void
}

export function WorkflowsTab({ onRefresh }: WorkflowsTabProps) {
  const [workflows, setWorkflows] = useState<Workflow[]>([])
  const [loading, setLoading] = useState(true)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editWorkflow, setEditWorkflow] = useState<Workflow | null>(null)
  const [newWorkflowName, setNewWorkflowName] = useState('')
  const [newWorkflowDescription, setNewWorkflowDescription] = useState('')
  const [executingId, setExecutingId] = useState<string | null>(null)

  const fetchWorkflows = async () => {
    setLoading(true)
    try {
      const result = await api.listWorkflows()
      setWorkflows(result.workflows || [])
    } catch (error) {
      console.error('Failed to fetch workflows:', error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchWorkflows()
  }, [])

  const handleToggleWorkflow = async (workflow: Workflow) => {
    try {
      await api.updateWorkflow(workflow.id, {
        enabled: !workflow.enabled,
      })
      await fetchWorkflows()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to toggle workflow:', error)
    }
  }

  const handleDeleteWorkflow = async (id: string) => {
    if (!confirm('确定要删除这个工作流吗？')) return
    try {
      await api.deleteWorkflow(id)
      await fetchWorkflows()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to delete workflow:', error)
    }
  }

  const handleExecuteWorkflow = async (id: string) => {
    setExecutingId(id)
    try {
      const result = await api.executeWorkflow(id)
      alert(`工作流已开始执行: ${result.execution_id}`)
    } catch (error) {
      console.error('Failed to execute workflow:', error)
    } finally {
      setExecutingId(null)
    }
  }

  const handleCreateWorkflow = async () => {
    if (!newWorkflowName.trim()) return
    try {
      await api.createWorkflow({
        name: newWorkflowName,
        description: newWorkflowDescription,
        triggers: [{ type: 'manual', config: {} }],
        steps: [],
        enabled: true,
        trigger_count: 0,
        step_count: 0,
        status: 'active',
      })
      setCreateDialogOpen(false)
      setNewWorkflowName('')
      setNewWorkflowDescription('')
      await fetchWorkflows()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to create workflow:', error)
    }
  }

  const handleEditWorkflow = async () => {
    if (!editWorkflow) return
    try {
      await api.updateWorkflow(editWorkflow.id, {
        name: editWorkflow.name,
        description: editWorkflow.description,
      })
      setEditWorkflow(null)
      await fetchWorkflows()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to update workflow:', error)
    }
  }

  const getStepIcon = (type: WorkflowStep['type']) => {
    switch (type) {
      case 'command': return '⚡'
      case 'condition': return '❓'
      case 'delay': return '⏱️'
      case 'notification': return '🔔'
      case 'llm': return '🧠'
      default: return '📄'
    }
  }

  const formatTimestamp = (timestamp: string | number) => {
    if (typeof timestamp === 'string') {
      return new Date(timestamp).toLocaleString('zh-CN', {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
    }
    const date = new Date(timestamp * 1000)
    return date.toLocaleString('zh-CN', {
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  return (
    <>
      {/* Header with actions */}
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-xl font-semibold">工作流</h2>
          <p className="text-sm text-muted-foreground">
            多步骤自动化流程编排
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateDialogOpen(true)}>
          <Plus className="h-4 w-4 mr-2" />
          新建工作流
        </Button>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {loading ? (
          <div className="col-span-full py-8 text-center text-muted-foreground">
            加载中...
          </div>
        ) : workflows.length === 0 ? (
          <div className="col-span-full py-8">
            <div className="flex flex-col items-center gap-3">
              <WorkflowIcon className="h-12 w-12 text-muted-foreground/50" />
              <p className="text-muted-foreground">暂无工作流</p>
              <Button variant="outline" size="sm" onClick={() => setCreateDialogOpen(true)}>
                <Plus className="h-4 w-4 mr-2" />
                创建第一个工作流
              </Button>
            </div>
          </div>
        ) : (
          workflows.map((workflow) => (
            <Card key={workflow.id} className={cn(!workflow.enabled && 'opacity-60')}>
              <CardHeader className="pb-3">
                <div className="flex items-start justify-between">
                  <div>
                    <CardTitle className="text-base">{workflow.name}</CardTitle>
                    {workflow.description && (
                      <CardDescription className="mt-1">
                        {workflow.description}
                      </CardDescription>
                    )}
                  </div>
                  <Switch
                    checked={workflow.enabled}
                    onCheckedChange={() => handleToggleWorkflow(workflow)}
                  />
                </div>
              </CardHeader>
              <CardContent className="space-y-4">
                {/* Triggers */}
                {workflow.triggers && workflow.triggers.length > 0 && (
                <div>
                  <div className="text-xs text-muted-foreground mb-2">触发器</div>
                  <div className="flex flex-wrap gap-1">
                    {workflow.triggers.map((trigger, i) => (
                      <Badge key={i} variant="outline" className="text-xs">
                        {trigger.type === 'manual' && '手动'}
                        {trigger.type === 'event' && '事件'}
                        {trigger.type === 'schedule' && '定时'}
                        {trigger.type === 'device_state' && '设备状态'}
                      </Badge>
                    ))}
                  </div>
                </div>
                )}

                {/* Steps */}
                {workflow.steps && workflow.steps.length > 0 && (
                <div>
                  <div className="text-xs text-muted-foreground mb-2">步骤</div>
                  <div className="flex items-center gap-1 overflow-x-auto pb-1">
                    {workflow.steps.slice(0, 4).map((step, i) => (
                      <div key={step.id} className="flex items-center">
                        <div className="flex items-center gap-1 px-2 py-1 bg-muted rounded-md">
                          <span>{getStepIcon(step.type)}</span>
                          <span className="text-xs truncate max-w-[80px]">{step.name}</span>
                        </div>
                        {i < Math.min((workflow.steps?.length ?? 0) - 1, 3) && (
                          <ArrowRight className="h-3 w-3 text-muted-foreground shrink-0" />
                        )}
                      </div>
                    ))}
                    {workflow.steps.length > 4 && (
                      <Badge variant="outline" className="ml-1">
                        +{workflow.steps.length - 4}
                      </Badge>
                    )}
                  </div>
                </div>
                )}

                {/* Stats */}
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>执行次数: {workflow.execution_count || 0}</span>
                  <span>更新于: {formatTimestamp(workflow.updated_at)}</span>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-2 pt-2 border-t">
                  <Button
                    variant="outline"
                    size="sm"
                    className="flex-1"
                    onClick={() => handleExecuteWorkflow(workflow.id)}
                    disabled={!workflow.enabled || executingId === workflow.id}
                  >
                    <Play className="h-3 w-3 mr-1" />
                    {executingId === workflow.id ? '执行中...' : '执行'}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setEditWorkflow(workflow)}
                  >
                    <Edit className="h-3 w-3" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => handleDeleteWorkflow(workflow.id)}
                  >
                    <Trash2 className="h-3 w-3 text-destructive" />
                  </Button>
                </div>
              </CardContent>
            </Card>
          ))
        )}
      </div>

      {/* Create Workflow Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>创建新工作流</DialogTitle>
            <DialogDescription>
              定义多步骤自动化流程
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="workflow-name">工作流名称</Label>
              <Input
                id="workflow-name"
                value={newWorkflowName}
                onChange={(e) => setNewWorkflowName(e.target.value)}
                placeholder="例如: 每日晨间唤醒"
              />
            </div>
            <div>
              <Label htmlFor="workflow-description">描述</Label>
              <Textarea
                id="workflow-description"
                value={newWorkflowDescription}
                onChange={(e) => setNewWorkflowDescription(e.target.value)}
                placeholder="描述这个工作流的作用..."
                className="min-h-[80px]"
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
              取消
            </Button>
            <Button onClick={handleCreateWorkflow} disabled={!newWorkflowName.trim()}>
              创建工作流
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Workflow Dialog */}
      <Dialog open={!!editWorkflow} onOpenChange={(open) => !open && setEditWorkflow(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>编辑工作流</DialogTitle>
            <DialogDescription>
              修改工作流配置
            </DialogDescription>
          </DialogHeader>
          {editWorkflow && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="edit-workflow-name">工作流名称</Label>
                <Input
                  id="edit-workflow-name"
                  value={editWorkflow.name}
                  onChange={(e) => setEditWorkflow({ ...editWorkflow, name: e.target.value })}
                />
              </div>
              <div>
                <Label htmlFor="edit-workflow-description">描述</Label>
                <Textarea
                  id="edit-workflow-description"
                  value={editWorkflow.description || ''}
                  onChange={(e) => setEditWorkflow({ ...editWorkflow, description: e.target.value })}
                  className="min-h-[80px]"
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditWorkflow(null)}>
              取消
            </Button>
            <Button onClick={handleEditWorkflow}>
              保存修改
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

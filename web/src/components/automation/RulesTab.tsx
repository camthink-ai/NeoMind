import { useState, useEffect } from 'react'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
import { Plus, Trash2, Edit, Zap, Clock } from 'lucide-react'
import { api } from '@/lib/api'
import type { Rule } from '@/types'

interface RulesTabProps {
  onRefresh?: () => void
}

export function RulesTab({ onRefresh }: RulesTabProps) {
  const [rules, setRules] = useState<Rule[]>([])
  const [loading, setLoading] = useState(true)
  const [createDialogOpen, setCreateDialogOpen] = useState(false)
  const [editRule, setEditRule] = useState<Rule | null>(null)
  const [newRuleName, setNewRuleName] = useState('')
  const [newRuleDSL, setNewRuleDSL] = useState('')
  const [validation, setValidation] = useState<{ valid: boolean; errors?: string[] } | null>(null)
  const [validating, setValidating] = useState(false)

  const fetchRules = async () => {
    setLoading(true)
    try {
      const result = await api.listRules()
      setRules(result.rules || [])
    } catch (error) {
      console.error('Failed to fetch rules:', error)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    fetchRules()
  }, [])

  const handleToggleRule = async (rule: Rule) => {
    try {
      if (rule.enabled) {
        await api.disableRule(rule.id)
      } else {
        await api.enableRule(rule.id)
      }
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to toggle rule:', error)
    }
  }

  const handleDeleteRule = async (id: string) => {
    if (!confirm('确定要删除这个规则吗？')) return
    try {
      await api.deleteRule(id)
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to delete rule:', error)
    }
  }

  const handleValidate = async () => {
    setValidating(true)
    try {
      const result = await api.validateRuleDSL(newRuleDSL)
      setValidation(result)
    } catch (error) {
      setValidation({ valid: false, errors: ['验证失败'] })
    } finally {
      setValidating(false)
    }
  }

  const handleCreateRule = async () => {
    if (!newRuleName.trim() || !newRuleDSL.trim()) return
    try {
      await api.createRule({
        name: newRuleName,
        dsl: newRuleDSL,
        enabled: true,
        trigger_count: 0,
      })
      setCreateDialogOpen(false)
      setNewRuleName('')
      setNewRuleDSL('')
      setValidation(null)
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to create rule:', error)
    }
  }

  const handleEditRule = async () => {
    if (!editRule) return
    try {
      await api.updateRule(editRule.id, {
        name: editRule.name,
        dsl: editRule.dsl,
      })
      setEditRule(null)
      await fetchRules()
      onRefresh?.()
    } catch (error) {
      console.error('Failed to update rule:', error)
    }
  }

  const formatTimestamp = (timestamp: string | number | undefined) => {
    if (!timestamp) return '-'
    if (typeof timestamp === 'string') {
      return new Date(timestamp).toLocaleString('zh-CN', {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      })
    }
    const date = new Date(timestamp * 1000)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return '刚刚'
    if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`

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
          <h2 className="text-xl font-semibold">规则引擎</h2>
          <p className="text-sm text-muted-foreground">
            基于条件的自动化触发规则
          </p>
        </div>
        <Button size="sm" onClick={() => setCreateDialogOpen(true)}>
          <Plus className="h-4 w-4 mr-2" />
          新建规则
        </Button>
      </div>

      <Card>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>规则名称</TableHead>
              <TableHead>状态</TableHead>
              <TableHead>触发次数</TableHead>
              <TableHead>最后触发</TableHead>
              <TableHead className="text-right">操作</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center py-8 text-muted-foreground">
                  加载中...
                </TableCell>
              </TableRow>
            ) : rules.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center py-8">
                  <div className="flex flex-col items-center gap-3">
                    <Zap className="h-12 w-12 text-muted-foreground/50" />
                    <p className="text-muted-foreground">暂无规则</p>
                    <Button variant="outline" size="sm" onClick={() => setCreateDialogOpen(true)}>
                      <Plus className="h-4 w-4 mr-2" />
                      创建第一个规则
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ) : (
              rules.map((rule) => (
                <TableRow key={rule.id}>
                  <TableCell>
                    <div>
                      <div className="font-medium">{rule.name}</div>
                      <div className="text-xs text-muted-foreground font-mono truncate max-w-md">
                        {rule.dsl}
                      </div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2">
                      <Switch
                        checked={rule.enabled}
                        onCheckedChange={() => handleToggleRule(rule)}
                      />
                      <Badge variant={rule.enabled ? 'default' : 'secondary'}>
                        {rule.enabled ? '启用' : '禁用'}
                      </Badge>
                    </div>
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-1">
                      <Zap className="h-3 w-3 text-yellow-500" />
                      {rule.trigger_count || 0}
                    </div>
                  </TableCell>
                  <TableCell>
                    {rule.last_triggered ? (
                      <div className="flex items-center gap-1 text-xs text-muted-foreground">
                        <Clock className="h-3 w-3" />
                        {formatTimestamp(rule.last_triggered)}
                      </div>
                    ) : (
                      <span className="text-xs text-muted-foreground">从未触发</span>
                    )}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-2">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => setEditRule(rule)}
                      >
                        <Edit className="h-3 w-3" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleDeleteRule(rule.id)}
                      >
                        <Trash2 className="h-3 w-3 text-destructive" />
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </Card>

      {/* Create Rule Dialog */}
      <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>创建新规则</DialogTitle>
            <DialogDescription>
              使用规则 DSL 定义触发条件和执行动作
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            <div>
              <Label htmlFor="rule-name">规则名称</Label>
              <Input
                id="rule-name"
                value={newRuleName}
                onChange={(e) => setNewRuleName(e.target.value)}
                placeholder="例如: 高温开空调"
              />
            </div>
            <div>
              <Label htmlFor="rule-dsl">规则 DSL</Label>
              <Textarea
                id="rule-dsl"
                value={newRuleDSL}
                onChange={(e) => setNewRuleDSL(e.target.value)}
                placeholder="// 示例规则
WHEN sensor.temp > 30
THEN device.ac.turn_on()"
                className="min-h-[120px] font-mono text-sm"
              />
            </div>
            {validation && (
              <div className={validation.valid ? "text-green-600 text-sm" : "text-red-600 text-sm"}>
                {validation.valid ? "✓ DSL 语法正确" : `✗ 验证失败: ${validation.errors?.join(', ')}`}
              </div>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateDialogOpen(false)}>
              取消
            </Button>
            <Button variant="outline" onClick={handleValidate} disabled={validating || !newRuleDSL.trim()}>
              {validating ? '验证中...' : '验证 DSL'}
            </Button>
            <Button onClick={handleCreateRule} disabled={!newRuleName.trim() || !newRuleDSL.trim()}>
              创建规则
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Rule Dialog */}
      <Dialog open={!!editRule} onOpenChange={(open) => !open && setEditRule(null)}>
        <DialogContent className="max-w-2xl">
          <DialogHeader>
            <DialogTitle>编辑规则</DialogTitle>
            <DialogDescription>
              修改规则名称和 DSL 定义
            </DialogDescription>
          </DialogHeader>
          {editRule && (
            <div className="space-y-4">
              <div>
                <Label htmlFor="edit-rule-name">规则名称</Label>
                <Input
                  id="edit-rule-name"
                  value={editRule.name}
                  onChange={(e) => setEditRule({ ...editRule, name: e.target.value })}
                />
              </div>
              <div>
                <Label htmlFor="edit-rule-dsl">规则 DSL</Label>
                <Textarea
                  id="edit-rule-dsl"
                  value={editRule.dsl}
                  onChange={(e) => setEditRule({ ...editRule, dsl: e.target.value })}
                  className="min-h-[120px] font-mono text-sm"
                />
              </div>
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditRule(null)}>
              取消
            </Button>
            <Button onClick={handleEditRule}>
              保存修改
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

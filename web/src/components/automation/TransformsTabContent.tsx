import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Database, Plus, Play, Edit, Trash2, ChevronDown, ChevronUp } from 'lucide-react'
import { api } from '@/lib/api'
import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
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
import { ActionBar, EmptyState } from '@/components/shared'
import { TransformBuilder } from './TransformBuilder'
import { TransformTestDialog } from './TransformTestDialog'
import type { TransformAutomation } from '@/types'

interface TransformsTabContentProps {
  onRefresh?: () => void
}

export function TransformsTabContent({ onRefresh }: TransformsTabContentProps) {
  const { t } = useTranslation(['automation', 'common'])
  const [transforms, setTransforms] = useState<TransformAutomation[]>([])
  const [loading, setLoading] = useState(true)
  const [builderOpen, setBuilderOpen] = useState(false)
  const [testDialogOpen, setTestDialogOpen] = useState(false)
  const [editingTransform, setEditingTransform] = useState<TransformAutomation | null>(null)
  const [testingTransformId, setTestingTransformId] = useState<string | null>(null)
  const [expandedRows, setExpandedRows] = useState<Set<string>>(new Set())
  const [devices, setDevices] = useState<Array<{ id: string; name: string; device_type?: string }>>([])

  const fetchTransforms = async () => {
    setLoading(true)
    try {
      const result = await api.listTransforms()
      setTransforms(result.transforms || [])
    } catch (error) {
      console.error('Failed to fetch transforms:', error)
    } finally {
      setLoading(false)
    }
  }

  const fetchDevices = async () => {
    try {
      const result = await api.getDevices()
      setDevices((result.devices || []).map((d: any) => ({
        id: d.id,
        name: d.name,
        device_type: d.device_type,
      })))
    } catch {
      // ignore
    }
  }

  useEffect(() => {
    fetchTransforms()
    fetchDevices()
  }, [])

  const handleToggleTransform = async (transform: TransformAutomation) => {
    try {
      await api.setAutomationStatus(transform.id, !transform.enabled)
      await fetchTransforms()
      onRefresh?.()
    } catch {
      // ignore
    }
  }

  const handleDeleteTransform = async (id: string) => {
    if (!confirm(t('automation:confirmDeleteTransform', { defaultValue: 'Are you sure you want to delete this transform?' }))) return
    try {
      await api.deleteAutomation(id)
      await fetchTransforms()
      onRefresh?.()
    } catch {
      // ignore
    }
  }

  const handleSaveTransform = async (data: Partial<TransformAutomation>) => {
    try {
      const definition = {
        scope: data.scope,
        device_type_filter: data.device_type_filter,
        operations: data.operations || [],
      }

      if (editingTransform) {
        await api.updateAutomation(editingTransform.id, {
          name: data.name || '',
          description: data.description,
          definition,
        })
      } else {
        await api.createAutomation({
          name: data.name || '',
          description: data.description,
          type: 'transform',
          enabled: data.enabled ?? true,
          definition,
        })
      }
      await fetchTransforms()
      setBuilderOpen(false)
      setEditingTransform(null)
      onRefresh?.()
    } catch (error) {
      console.error('Failed to save transform:', error)
    }
  }

  const toggleRowExpanded = (id: string) => {
    setExpandedRows((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const getScopeLabel = (scope: TransformAutomation['scope']) => {
    switch (scope.type) {
      case 'global':
        return t('automation:scopeGlobal', { defaultValue: 'Global' })
      case 'device_type':
        return t('automation:scopeDeviceType', { device_type: scope.device_type })
      case 'device':
        return t('automation:scopeDevice', { device_id: scope.device_id })
      case 'user':
        return t('automation:scopeUser', { user_id: scope.user_id })
    }
  }

  const getScopeBadgeVariant = (scope: TransformAutomation['scope']) => {
    switch (scope.type) {
      case 'global':
        return 'default'
      case 'device_type':
        return 'secondary'
      case 'device':
        return 'outline'
      case 'user':
        return 'destructive' as const
      default:
        return 'default'
    }
  }

  const getComplexityDots = (complexity: number) => {
    return Array.from({ length: 5 }, (_, i) => (
      <span
        key={i}
        className={`h-2 w-2 rounded-full ${i < complexity ? 'bg-blue-500' : 'bg-gray-300'}`}
      />
    ))
  }

  return (
    <div className="space-y-4">
      {/* Header */}
      <ActionBar
        actions={[
          {
            label: t('automation:createTransform', { defaultValue: 'Create Transform' }),
            icon: <Plus className="h-4 w-4" />,
            onClick: () => {
              setEditingTransform(null)
              setBuilderOpen(true)
            },
          },
        ]}
      />

      {/* Transforms Table */}
      <Card>
        {loading ? (
          <div className="p-8 text-center text-muted-foreground">
            {t('common:loading', { defaultValue: 'Loading...' })}
          </div>
        ) : transforms.length === 0 ? (
          <EmptyState
            icon={<Database className="h-12 w-12" />}
            title={t('automation:noTransforms', { defaultValue: 'No transforms yet' })}
            description={t('automation:noTransformsDesc', {
              defaultValue: 'Transforms process raw device data into virtual metrics. Create your first transform to get started.',
            })}
            action={{
              label: t('automation:createTransform', { defaultValue: 'Create Transform' }),
              onClick: () => {
                setEditingTransform(null)
                setBuilderOpen(true)
              },
            }}
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-[30px]" />
                <TableHead>{t('automation:name', { defaultValue: 'Name' })}</TableHead>
                <TableHead>{t('automation:scope', { defaultValue: 'Scope' })}</TableHead>
                <TableHead>{t('automation:operations', { defaultValue: 'Operations' })}</TableHead>
                <TableHead>{t('automation:complexity', { defaultValue: 'Complexity' })}</TableHead>
                <TableHead>{t('common:status', { defaultValue: 'Status' })}</TableHead>
                <TableHead className="text-right">{t('common:actions', { defaultValue: 'Actions' })}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {transforms.map((transform) => (
                <>
                  <TableRow>
                    <TableCell>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-6 w-6 p-0"
                        onClick={() => toggleRowExpanded(transform.id)}
                      >
                        {expandedRows.has(transform.id) ? (
                          <ChevronUp className="h-4 w-4" />
                        ) : (
                          <ChevronDown className="h-4 w-4" />
                        )}
                      </Button>
                    </TableCell>
                    <TableCell>
                      <div className="font-medium">{transform.name}</div>
                      <div className="text-sm text-muted-foreground">{transform.description}</div>
                    </TableCell>
                    <TableCell>
                      <Badge variant={getScopeBadgeVariant(transform.scope)}>
                        {getScopeLabel(transform.scope)}
                      </Badge>
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {transform.js_code ? (
                          <Badge variant="outline" className="text-xs bg-purple-100 dark:bg-purple-900">
                            {t('automation:jsCode', { defaultValue: 'JavaScript' })}
                          </Badge>
                        ) : transform.operations && transform.operations.length > 0 ? (
                          transform.operations.map((op, idx) => (
                            <Badge key={idx} variant="outline" className="text-xs">
                              {op.op_type}
                            </Badge>
                          ))
                        ) : (
                          <Badge variant="outline" className="text-xs text-muted-foreground">
                            {t('automation:noCode', { defaultValue: 'No code' })}
                          </Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <div className="flex gap-1">{getComplexityDots(transform.complexity)}</div>
                    </TableCell>
                    <TableCell>
                      <Switch
                        checked={transform.enabled}
                        onCheckedChange={() => handleToggleTransform(transform)}
                      />
                    </TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-2">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => {
                            setTestingTransformId(transform.id)
                            setTestDialogOpen(true)
                          }}
                          title={t('automation:testTransform', { defaultValue: 'Test Transform' })}
                        >
                          <Play className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => {
                            setEditingTransform(transform)
                            setBuilderOpen(true)
                          }}
                          title={t('common:edit', { defaultValue: 'Edit' })}
                        >
                          <Edit className="h-4 w-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDeleteTransform(transform.id)}
                          title={t('common:delete', { defaultValue: 'Delete' })}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>

                  {/* Expanded Details */}
                  {expandedRows.has(transform.id) && (
                    <TableRow>
                      <TableCell colSpan={7} className="bg-muted/50">
                        <div className="space-y-4 py-4">
                          {/* Intent or Operations */}
                          <div>
                            <h4 className="font-medium mb-2">{t('automation:intent', { defaultValue: 'Intent' })}</h4>
                            <p className="text-sm text-muted-foreground">
                              {transform.intent || t('automation:noIntent', { defaultValue: 'No intent description' })}
                            </p>
                          </div>

                          {/* JavaScript Code */}
                          {transform.js_code && (
                            <div>
                              <h4 className="font-medium mb-2">{t('automation:generatedCode', { defaultValue: 'Generated Code' })}</h4>
                              <Card className="p-3 bg-muted">
                                <pre className="text-xs overflow-x-auto font-mono whitespace-pre-wrap">
                                  <code>{transform.js_code}</code>
                                </pre>
                              </Card>
                            </div>
                          )}

                          {/* Legacy Operations */}
                          {transform.operations && transform.operations.length > 0 && (
                            <div>
                              <h4 className="font-medium mb-2">{t('automation:operations', { defaultValue: 'Operations (Legacy)' })}</h4>
                              <div className="space-y-2">
                                {transform.operations.map((op, idx) => (
                                  <div key={idx} className="pl-4 border-l-2 border-blue-500">
                                    <div className="text-sm font-medium">{op.op_type}</div>
                                    <pre className="text-xs text-muted-foreground mt-1 overflow-x-auto">
                                      {JSON.stringify(op, null, 2)}
                                    </pre>
                                  </div>
                                ))}
                              </div>
                            </div>
                          )}

                          <div className="grid grid-cols-2 gap-4 text-sm">
                            <div>
                              <span className="text-muted-foreground">{t('automation:created', { defaultValue: 'Created' })}: </span>
                              {new Date(transform.created_at * 1000).toLocaleString()}
                            </div>
                            <div>
                              <span className="text-muted-foreground">{t('automation:updated', { defaultValue: 'Updated' })}: </span>
                              {new Date(transform.updated_at * 1000).toLocaleString()}
                            </div>
                            <div>
                              <span className="text-muted-foreground">{t('automation:executions', { defaultValue: 'Executions' })}: </span>
                              {transform.execution_count}
                            </div>
                            <div>
                              <span className="text-muted-foreground">{t('automation:lastExecuted', { defaultValue: 'Last Executed' })}: </span>
                              {transform.last_executed
                                ? new Date(transform.last_executed * 1000).toLocaleString()
                                : t('common:never', { defaultValue: 'Never' })}
                            </div>
                          </div>
                        </div>
                      </TableCell>
                    </TableRow>
                  )}
                </>
              ))}
            </TableBody>
          </Table>
        )}
      </Card>

      {/* Transform Builder Dialog */}
      {builderOpen && (
        <TransformBuilder
          open={builderOpen}
          onOpenChange={setBuilderOpen}
          transform={editingTransform}
          devices={devices}
          onSave={handleSaveTransform}
        />
      )}

      {/* Transform Test Dialog */}
      {testDialogOpen && testingTransformId && (
        <TransformTestDialog
          open={testDialogOpen}
          onOpenChange={setTestDialogOpen}
          transformId={testingTransformId}
          devices={devices}
        />
      )}
    </div>
  )
}


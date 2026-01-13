import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { api } from '@/lib/api'
import type { DecisionDto } from '@/types'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { PageLayout } from '@/components/layout/PageLayout'
import { PageTabs, PageTabsContent, LoadingState, EmptyState } from '@/components/shared'
import { useApiData } from '@/hooks/useApiData'
import { formatTimestamp } from '@/lib/utils/format'
import { useToast } from '@/hooks/use-toast'
import {
  RefreshCw, CheckCircle, X, Trash2, Play, Brain,
  AlertCircle, Eye
} from 'lucide-react'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

type DecisionFilter = 'all' | 'proposed' | 'approved' | 'executed' | 'rejected'

const fetchDecisions = async (filter: DecisionFilter): Promise<DecisionDto[]> => {
  const status = filter === 'all' ? undefined : filter
  const response = await api.listDecisions({ status, limit: 100 })
  return response.decisions || []
}

export function DecisionsPage() {
  const { t } = useTranslation(['common', 'decisions'])
  const [filter, setFilter] = useState<DecisionFilter>('all')
  const [selectedDecision, setSelectedDecision] = useState<DecisionDto | null>(null)
  const [processingId, setProcessingId] = useState<string | null>(null)
  const { toast } = useToast()

  const { data: decisions, loading, refetch } = useApiData(
    () => fetchDecisions(filter),
    { deps: [filter] }
  )

  const handleExecute = async (id: string) => {
    setProcessingId(id)
    try {
      await api.executeDecision(id)
      toast({ title: t('common:success'), description: t('decisions:executedSuccess') })
      refetch()
    } catch (error) {
      toast({ title: t('common:failed'), description: (error as Error).message || t('decisions:actionFailed'), variant: 'destructive' })
    } finally {
      setProcessingId(null)
    }
  }

  const handleApprove = async (id: string) => {
    setProcessingId(id)
    try {
      await api.approveDecision(id)
      toast({ title: t('common:success'), description: t('decisions:approvedSuccess') })
      refetch()
    } catch (error) {
      toast({ title: t('common:failed'), description: (error as Error).message || t('decisions:actionFailed'), variant: 'destructive' })
    } finally {
      setProcessingId(null)
    }
  }

  const handleReject = async (id: string) => {
    setProcessingId(id)
    try {
      await api.rejectDecision(id)
      toast({ title: t('common:success'), description: t('decisions:rejectedSuccess') })
      refetch()
    } catch (error) {
      toast({ title: t('common:failed'), description: (error as Error).message || t('decisions:actionFailed'), variant: 'destructive' })
    } finally {
      setProcessingId(null)
    }
  }

  const handleDelete = async (id: string) => {
    if (!confirm(t('decisions:deleteConfirm'))) return
    setProcessingId(id)
    try {
      await api.deleteDecision(id)
      toast({ title: t('common:success'), description: t('decisions:deletedSuccess') })
      refetch()
    } catch (error) {
      toast({ title: t('common:failed'), description: (error as Error).message || t('decisions:actionFailed'), variant: 'destructive' })
    } finally {
      setProcessingId(null)
    }
  }

  const getStatusBadge = (status: string) => {
    const variantMap: Record<string, 'default' | 'secondary' | 'destructive' | 'outline'> = {
      Proposed: 'default',
      Approved: 'secondary',
      Rejected: 'destructive',
      Executed: 'outline',
      Failed: 'destructive',
      Expired: 'outline',
    }
    const labelMap: Record<string, string> = {
      Proposed: t('decisions:pending'),
      Approved: t('decisions:approved'),
      Rejected: t('decisions:rejected'),
      Executed: t('decisions:executed'),
      Failed: t('decisions:failed'),
      Expired: t('decisions:expired'),
    }
    return (
      <Badge variant={variantMap[status] || 'default'}>
        {labelMap[status] || status}
      </Badge>
    )
  }

  const getConfidenceColor = (confidence: number) => {
    if (confidence >= 0.8) return 'text-green-600'
    if (confidence >= 0.6) return 'text-yellow-600'
    return 'text-red-600'
  }

  const tabs = [
    { value: 'all' as DecisionFilter, label: t('decisions:all') },
    { value: 'proposed' as DecisionFilter, label: t('decisions:pending') },
    { value: 'executed' as DecisionFilter, label: t('decisions:executed') },
    { value: 'rejected' as DecisionFilter, label: t('decisions:rejected') },
  ]

  return (
    <PageLayout>
      <PageTabs
        tabs={tabs}
        activeTab={filter}
        onTabChange={(v) => setFilter(v as DecisionFilter)}
        actions={[
          { label: t('common:refresh'), icon: <RefreshCw className="h-4 w-4" />, onClick: refetch, variant: 'outline' },
        ]}
      >
        <PageTabsContent value={filter} activeTab={filter}>
          {loading ? (
            <LoadingState text={t('decisions:loading')} />
          ) : !decisions || decisions.length === 0 ? (
            <EmptyState
              icon={<Brain className="h-12 w-12 text-muted-foreground" />}
              title={t('decisions:noDecisions')}
              description={t('decisions:noDecisionsDesc')}
            />
          ) : (
            <div className="space-y-4">
              {decisions.map((decision) => (
                <Card
                  key={decision.id}
                  className={decision.status === 'Proposed' ? 'border-l-4 border-l-blue-500' : ''}
                >
                  <CardHeader className="pb-3">
                    <div className="flex items-start justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2 mb-1 flex-wrap">
                          <CardTitle className="text-base">{decision.title}</CardTitle>
                          {getStatusBadge(decision.status)}
                          <Badge variant="outline" className="text-xs">
                            {decision.decision_type}
                          </Badge>
                          <Badge
                            variant="outline"
                            className={`text-xs ${
                              decision.confidence >= 0.8
                                ? 'text-green-600 border-green-600'
                                : decision.confidence >= 0.6
                                ? 'text-yellow-600 border-yellow-600'
                                : 'text-red-600 border-red-600'
                            }`}
                          >
                            {t('decisions:confidence')} {(decision.confidence * 100).toFixed(0)}%
                          </Badge>
                        </div>
                        <CardDescription className="text-xs">
                          {decision.description}
                        </CardDescription>
                      </div>
                      <div className="flex gap-2">
                        <Button
                          onClick={() => setSelectedDecision(decision)}
                          variant="ghost"
                          size="sm"
                        >
                          <Eye className="h-3 w-3" />
                        </Button>
                        {decision.status === 'Proposed' && (
                          <>
                            <Button
                              onClick={() => handleExecute(decision.id)}
                              size="sm"
                              disabled={processingId === decision.id}
                            >
                              <Play className="h-3 w-3 mr-1" />
                              {t('decisions:execute')}
                            </Button>
                            <Button
                              onClick={() => handleApprove(decision.id)}
                              variant="outline"
                              size="sm"
                              disabled={processingId === decision.id}
                            >
                              <CheckCircle className="h-3 w-3 mr-1" />
                              {t('decisions:approve')}
                            </Button>
                            <Button
                              onClick={() => handleReject(decision.id)}
                              variant="outline"
                              size="sm"
                              disabled={processingId === decision.id}
                            >
                              <X className="h-3 w-3 mr-1" />
                              {t('decisions:reject')}
                            </Button>
                          </>
                        )}
                        {decision.status !== 'Proposed' && (
                          <Button
                            onClick={() => handleDelete(decision.id)}
                            variant="ghost"
                            size="sm"
                            disabled={processingId === decision.id}
                          >
                            <Trash2 className="h-3 w-3" />
                          </Button>
                        )}
                      </div>
                    </div>
                  </CardHeader>
                  <CardContent className="text-sm">
                    <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-muted-foreground mb-3">
                      <div>
                        <span className="font-medium">{t('decisions:priority')}:</span>{' '}
                        <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${
                          decision.priority === 'high' || decision.priority === 'critical'
                            ? 'bg-red-100 text-red-700'
                            : decision.priority === 'medium'
                            ? 'bg-yellow-100 text-yellow-700'
                            : 'bg-gray-100 text-gray-700'
                        }`}>
                          {decision.priority}
                        </span>
                      </div>
                      <div>
                        <span className="font-medium">{t('decisions:createdAt')}:</span>{' '}
                        {formatTimestamp(decision.created_at)}
                      </div>
                      {decision.executed_at && (
                        <div>
                          <span className="font-medium">{t('decisions:executedAt')}:</span>{' '}
                          {formatTimestamp(decision.executed_at)}
                        </div>
                      )}
                    </div>

                    {decision.reasoning && (
                      <details className="mb-3">
                        <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                          {t('decisions:reasoning')}
                        </summary>
                        <div className="mt-2 p-3 bg-muted rounded text-sm whitespace-pre-wrap max-h-40 overflow-y-auto">
                          {decision.reasoning}
                        </div>
                      </details>
                    )}

                    {decision.actions && decision.actions.length > 0 && (
                      <details>
                        <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                          {t('decisions:proposedActions')} ({decision.actions.length})
                        </summary>
                        <div className="mt-2 space-y-2">
                          {decision.actions.map((action, index) => (
                            <div key={action.id} className="p-2 bg-muted rounded">
                              <div className="flex items-center gap-2 mb-1 flex-wrap">
                                <Badge variant="outline" className="text-xs">
                                  {index + 1}
                                </Badge>
                                <span className="font-medium">{action.action_type}</span>
                                {action.required && (
                                  <Badge variant="destructive" className="text-xs">{t('decisions:required')}</Badge>
                                )}
                              </div>
                              <p className="text-xs text-muted-foreground">{action.description}</p>
                              {action.parameters && Object.keys(action.parameters).length > 0 && (
                                <pre className="mt-1 text-xs overflow-x-auto p-2 bg-background rounded">
                                  {JSON.stringify(action.parameters, null, 2)}
                                </pre>
                              )}
                            </div>
                          ))}
                        </div>
                      </details>
                    )}

                    {decision.execution_result && (
                      <div className="mt-3 p-3 bg-muted rounded">
                        <div className="flex items-center gap-2 mb-2">
                          <span className="font-medium">{t('decisions:executionResult')}:</span>{' '}
                          {decision.execution_result.success ? (
                            <Badge variant="outline" className="text-green-600">{t('decisions:success')}</Badge>
                          ) : (
                            <Badge variant="destructive">{t('decisions:failed')}</Badge>
                          )}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {t('decisions:actionsExecuted')}: {decision.execution_result.actions_executed} •
                          {t('decisions:successCount')}: {decision.execution_result.success_count} •
                          {t('decisions:failureCount')}: {decision.execution_result.failure_count}
                        </div>
                        {decision.execution_result.error && (
                          <div className="mt-1 text-xs text-red-600">
                            {t('decisions:error')}: {decision.execution_result.error}
                          </div>
                        )}
                      </div>
                    )}
                  </CardContent>
                </Card>
              ))}
            </div>
          )}
        </PageTabsContent>
      </PageTabs>

      {/* Decision Detail Dialog */}
      <Dialog open={!!selectedDecision} onOpenChange={() => setSelectedDecision(null)}>
        <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Brain className="h-5 w-5" />
              {selectedDecision?.title}
            </DialogTitle>
            <DialogDescription>
              {selectedDecision && getStatusBadge(selectedDecision.status)}
            </DialogDescription>
          </DialogHeader>
          {selectedDecision && (
            <div className="space-y-4">
              <div className="grid grid-cols-2 gap-4 text-sm">
                <div>
                  <span className="text-muted-foreground">{t('decisions:decisionType')}:</span>{' '}
                  <span className="font-medium">{selectedDecision.decision_type}</span>
                </div>
                <div>
                  <span className="text-muted-foreground">{t('decisions:priority')}:</span>{' '}
                  <span className="font-medium">{selectedDecision.priority}</span>
                </div>
                <div>
                  <span className="text-muted-foreground">{t('decisions:confidence')}:</span>{' '}
                  <span className={`font-medium ${getConfidenceColor(selectedDecision.confidence)}`}>
                    {(selectedDecision.confidence * 100).toFixed(1)}%
                  </span>
                </div>
                <div>
                  <span className="text-muted-foreground">{t('decisions:createdAt')}:</span>{' '}
                  <span className="font-medium">{formatTimestamp(selectedDecision.created_at)}</span>
                </div>
              </div>

              <div>
                <h4 className="font-medium mb-2">{t('decisions:description')}</h4>
                <p className="text-sm text-muted-foreground">{selectedDecision.description}</p>
              </div>

              {selectedDecision.reasoning && (
                <div>
                  <h4 className="font-medium mb-2">{t('decisions:reasoning')}</h4>
                  <div className="p-3 bg-muted rounded text-sm whitespace-pre-wrap max-h-48 overflow-y-auto">
                    {selectedDecision.reasoning}
                  </div>
                </div>
              )}

              {selectedDecision.actions && selectedDecision.actions.length > 0 && (
                <div>
                  <h4 className="font-medium mb-2">{t('decisions:proposedActions')}</h4>
                  <div className="space-y-2">
                    {selectedDecision.actions.map((action, index) => (
                      <div key={action.id} className="p-3 bg-muted rounded">
                        <div className="flex items-center gap-2 mb-1">
                          <Badge variant="outline">{index + 1}</Badge>
                          <span className="font-medium">{action.action_type}</span>
                        </div>
                        <p className="text-sm text-muted-foreground">{action.description}</p>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {selectedDecision.execution_result && (
                <div>
                  <h4 className="font-medium mb-2">{t('decisions:executionResult')}</h4>
                  <div className="p-3 bg-muted rounded">
                    <div className="flex items-center gap-2">
                      {selectedDecision.execution_result.success ? (
                        <CheckCircle className="h-4 w-4 text-green-600" />
                      ) : (
                        <AlertCircle className="h-4 w-4 text-red-600" />
                      )}
                      <span className="text-sm">
                        {selectedDecision.execution_result.success ? t('decisions:success') : t('decisions:failed')}
                      </span>
                    </div>
                    <div className="mt-2 text-xs text-muted-foreground">
                      {t('decisions:actionsExecuted')}: {selectedDecision.execution_result.actions_executed} •
                      {t('decisions:successCount')}: {selectedDecision.execution_result.success_count} •
                      {t('decisions:failureCount')}: {selectedDecision.execution_result.failure_count}
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setSelectedDecision(null)}>
              {t('common:close')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </PageLayout>
  )
}

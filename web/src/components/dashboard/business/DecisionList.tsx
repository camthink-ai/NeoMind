/**
 * Decision List Component
 *
 * Lists LLM decisions with their status and actions.
 * Shows pending, approved, and rejected decisions.
 */

import { useState, useMemo } from 'react'
import { Brain, CheckCircle, XCircle, Clock, Eye, ThumbsUp, ThumbsDown } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type DecisionStatus = 'pending' | 'approved' | 'rejected' | 'executed' | 'failed'

export interface Decision {
  id: string
  title: string
  description?: string
  status: DecisionStatus
  timestamp: number | string
  agentId?: string
  confidence?: number
  reasoning?: string
  approvedBy?: string
  executedAt?: number | string
  result?: string
}

export interface DecisionListProps {
  dataSource?: DataSource
  decisions?: Decision[]
  title?: string
  filter?: DecisionStatus[]
  showReasoning?: boolean
  showConfidence?: boolean
  maxDecisions?: number
  onApprove?: (decisionId: string) => void
  onReject?: (decisionId: string) => void
  onView?: (decisionId: string) => void
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const statusConfig: Record<DecisionStatus, { icon: typeof CheckCircle; color: string; bg: string; label: string }> = {
  pending: { icon: Clock, color: 'text-yellow-500', bg: 'bg-yellow-500/10', label: 'Pending' },
  approved: { icon: CheckCircle, color: 'text-green-500', bg: 'bg-green-500/10', label: 'Approved' },
  rejected: { icon: XCircle, color: 'text-red-500', bg: 'bg-red-500/10', label: 'Rejected' },
  executed: { icon: CheckCircle, color: 'text-blue-500', bg: 'bg-blue-500/10', label: 'Executed' },
  failed: { icon: XCircle, color: 'text-red-600', bg: 'bg-red-600/10', label: 'Failed' },
}

const sizeClasses = {
  sm: {
    card: 'p-2',
    title: 'text-sm',
    description: 'text-xs',
    badge: 'px-1.5 py-0.5 text-xs',
  },
  md: {
    card: 'p-3',
    title: 'text-sm',
    description: 'text-sm',
    badge: 'px-2 py-0.5 text-xs',
  },
  lg: {
    card: 'p-4',
    title: 'text-base',
    description: 'text-base',
    badge: 'px-2.5 py-0.5 text-sm',
  },
}

export function DecisionList({
  dataSource,
  decisions: propDecisions,
  title,
  filter,
  showReasoning = false,
  showConfidence = true,
  maxDecisions = 50,
  onApprove,
  onReject,
  onView,
  size = 'md',
  showCard = true,
  className,
}: DecisionListProps) {
  const [expandedDecisions, setExpandedDecisions] = useState<Set<string>>(new Set())

  // Get data from data source or use prop
  const { data } = useDataSource<Decision[]>(dataSource, { fallback: propDecisions || [] })
  const allDecisions = data || []

  const sizes = sizeClasses[size]

  // Filter and sort decisions
  const filteredDecisions = useMemo(() => {
    let filtered = allDecisions

    if (filter && filter.length > 0) {
      filtered = filtered.filter(d => filter.includes(d.status))
    }

    // Sort by timestamp (newest first)
    return [...filtered]
      .sort((a, b) => {
        const aTime = typeof a.timestamp === 'string' ? new Date(a.timestamp).getTime() : a.timestamp
        const bTime = typeof b.timestamp === 'string' ? new Date(b.timestamp).getTime() : b.timestamp
        return bTime - aTime
      })
      .slice(0, maxDecisions)
  }, [allDecisions, filter, maxDecisions])

  // Group by status
  const groupedDecisions = useMemo(() => {
    return filteredDecisions.reduce((acc, decision) => {
      if (!acc[decision.status]) acc[decision.status] = []
      acc[decision.status].push(decision)
      return acc
    }, {} as Record<DecisionStatus, Decision[]>)
  }, [filteredDecisions])

  const formatTimeAgo = (timestamp: number | string) => {
    const date = typeof timestamp === 'string' ? new Date(timestamp) : new Date(timestamp)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    if (diff < 60000) return 'Just now'
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`
    return date.toLocaleDateString()
  }

  const toggleExpand = (id: string) => {
    setExpandedDecisions(prev => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const getConfidenceColor = (confidence?: number) => {
    if (!confidence) return 'text-muted-foreground'
    if (confidence >= 80) return 'text-green-500'
    if (confidence >= 50) return 'text-yellow-500'
    return 'text-red-500'
  }

  const renderDecision = (decision: Decision) => {
    const config = statusConfig[decision.status]
    const Icon = config.icon
    const isExpanded = expandedDecisions.has(decision.id)

    return (
      <div
        key={decision.id}
        className={cn(
          'rounded-lg border border-border transition-all',
          'hover:shadow-md',
          decision.status === 'pending' ? 'border-l-4 border-l-yellow-500' : '',
          sizes.card
        )}
      >
        <div className="flex items-start gap-3">
          {/* Status icon */}
          <div className={cn('shrink-0 p-2 rounded-lg', config.bg)}>
            <Icon className={cn('h-4 w-4', config.color)} />
          </div>

          {/* Content */}
          <div className="flex-1 min-w-0">
            <div className="flex items-start gap-2">
              <div className={cn('font-medium', sizes.title)}>{decision.title}</div>
              {decision.status === 'pending' && (
                <span className="animate-pulse w-2 h-2 rounded-full bg-yellow-500 mt-1" />
              )}
            </div>

            {decision.description && !isExpanded && (
              <p className={cn('text-muted-foreground mt-1 line-clamp-1', sizes.description)}>
                {decision.description}
              </p>
            )}

            {/* Metadata */}
            <div className="flex items-center gap-3 mt-2 text-xs text-muted-foreground">
              <span>{formatTimeAgo(decision.timestamp)}</span>
              {decision.agentId && (
                <span>· Agent: {decision.agentId.slice(0, 8)}</span>
              )}
              {showConfidence && decision.confidence !== undefined && (
                <span className={cn('flex items-center gap-1', getConfidenceColor(decision.confidence))}>
                  <Brain className="h-3 w-3" />
                  {decision.confidence}% confidence
                </span>
              )}
            </div>

            {/* Expanded content */}
            {isExpanded && (
              <div className="mt-3 space-y-2">
                {decision.description && (
                  <p className={cn('text-foreground', sizes.description)}>
                    {decision.description}
                  </p>
                )}
                {showReasoning && decision.reasoning && (
                  <div className="p-2 bg-muted rounded text-sm">
                    <div className="text-xs text-muted-foreground mb-1">Reasoning:</div>
                    <p className="line-clamp-3">{decision.reasoning}</p>
                  </div>
                )}
                {decision.approvedBy && (
                  <div className="text-xs text-muted-foreground">
                    Approved by: {decision.approvedBy}
                  </div>
                )}
                {decision.result && (
                  <div className="p-2 bg-green-500/10 text-green-500 rounded text-sm">
                    Result: {decision.result}
                  </div>
                )}
              </div>
            )}
          </div>

          {/* Actions */}
          <div className="flex items-center gap-1 shrink-0">
            {/* View button */}
            {onView && (
              <button
                onClick={() => onView(decision.id)}
                className="p-1.5 hover:bg-accent rounded transition-colors"
                title="View details"
              >
                <Eye className="h-4 w-4 text-muted-foreground" />
              </button>
            )}

            {/* Approve/Reject for pending */}
            {decision.status === 'pending' && (
              <>
                {onApprove && (
                  <button
                    onClick={() => onApprove(decision.id)}
                    className="p-1.5 hover:bg-green-500/10 rounded transition-colors"
                    title="Approve"
                  >
                    <ThumbsUp className="h-4 w-4 text-green-500" />
                  </button>
                )}
                {onReject && (
                  <button
                    onClick={() => onReject(decision.id)}
                    className="p-1.5 hover:bg-red-500/10 rounded transition-colors"
                    title="Reject"
                  >
                    <ThumbsDown className="h-4 w-4 text-red-500" />
                  </button>
                )}
              </>
            )}

            {/* Expand button */}
            {(decision.description || showReasoning) && (
              <button
                onClick={() => toggleExpand(decision.id)}
                className="p-1.5 hover:bg-accent rounded transition-colors"
              >
                {isExpanded ? (
                  <span className="text-xs">▲</span>
                ) : (
                  <span className="text-xs">▼</span>
                )}
              </button>
            )}
          </div>

          {/* Status badge */}
          <span className={cn('px-2 py-1 rounded text-xs font-medium shrink-0', config.bg, config.color)}>
            {config.label}
          </span>
        </div>
      </div>
    )
  }

  const renderGroup = (status: DecisionStatus, decisions: Decision[]) => {
    const config = statusConfig[status]

    return (
      <div key={status}>
        <div className={cn('sticky top-0 bg-background/95 backdrop-blur z-10 px-2 py-1.5 text-xs font-medium text-muted-foreground border-b border-border mb-2')}>
          {config.label} ({decisions.length})
        </div>
        <div className="space-y-2">
          {decisions.map(renderDecision)}
        </div>
      </div>
    )
  }

  // Calculate stats
  const stats = {
    total: filteredDecisions.length,
    pending: filteredDecisions.filter(d => d.status === 'pending').length,
    approved: filteredDecisions.filter(d => d.status === 'approved' || d.status === 'executed').length,
    rejected: filteredDecisions.filter(d => d.status === 'rejected').length,
  }

  const content = (
    <div className={cn('space-y-4', className)}>
      {/* Summary stats */}
      <div className="grid grid-cols-4 gap-2">
        <div className="text-center p-2 bg-muted/30 rounded-lg">
          <div className={cn('font-bold tabular-nums', sizes.title)}>{stats.total}</div>
          <div className="text-xs text-muted-foreground">Total</div>
        </div>
        <div className="text-center p-2 bg-yellow-500/10 rounded-lg">
          <Clock className="h-3 w-3 mx-auto mb-1 text-yellow-500" />
          <div className={cn('font-bold tabular-nums text-yellow-500', sizes.title)}>{stats.pending}</div>
          <div className="text-xs text-muted-foreground">Pending</div>
        </div>
        <div className="text-center p-2 bg-green-500/10 rounded-lg">
          <CheckCircle className="h-3 w-3 mx-auto mb-1 text-green-500" />
          <div className={cn('font-bold tabular-nums text-green-500', sizes.title)}>{stats.approved}</div>
          <div className="text-xs text-muted-foreground">Approved</div>
        </div>
        <div className="text-center p-2 bg-red-500/10 rounded-lg">
          <XCircle className="h-3 w-3 mx-auto mb-1 text-red-500" />
          <div className={cn('font-bold tabular-nums text-red-500', sizes.title)}>{stats.rejected}</div>
          <div className="text-xs text-muted-foreground">Rejected</div>
        </div>
      </div>

      {/* Decision list */}
      {filteredDecisions.length === 0 ? (
        <div className="py-8 text-center text-muted-foreground text-sm">
          No decisions
        </div>
      ) : (
        <div className="space-y-4">
          {Object.entries(groupedDecisions).map(([status, decisions]) =>
            renderGroup(status as DecisionStatus, decisions)
          )}
        </div>
      )}
    </div>
  )

  if (showCard && title) {
    return (
      <DashboardComponentWrapper
        title={title}
        showCard={true}
        padding="md"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

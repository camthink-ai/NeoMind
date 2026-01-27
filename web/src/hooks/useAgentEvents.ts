// useAgentEvents Hook for Agent-specific real-time events
//
// Provides filtered event streaming for a specific agent.

import { useEffect, useState, useCallback, useRef } from 'react'
import type {
  NeoTalkEvent,
  AgentThinkingEvent,
  AgentDecisionEvent,
  AgentExecutionStartedEvent,
  AgentExecutionCompletedEvent,
  AgentMemoryUpdatedEvent,
} from '@/lib/events'
import { useEvents } from './useEvents'

export interface AgentThinkingStep {
  step_number: number
  step_type: 'data_collection' | 'analysis' | 'decision' | 'action'
  description: string
  details?: {
    source?: string
    data?: unknown
  }
  timestamp: number
  status: 'pending' | 'in_progress' | 'completed'
}

export interface AgentExecution {
  id: string
  agent_id: string
  trigger_type: string
  started_at: number
  completed_at?: number
  duration_ms?: number
  status: 'running' | 'completed' | 'failed'
  steps: AgentThinkingStep[]
  decisions: Array<{
    description: string
    rationale: string
    action: string
    confidence: number
    timestamp: number
  }>
}

export interface UseAgentEventsOptions {
  /**
   * Filter to specific event types
   */
  eventTypes?: Array<'AgentExecutionStarted' | 'AgentExecutionCompleted' | 'AgentThinking' | 'AgentDecision' | 'AgentMemoryUpdated'>

  /**
   * Whether to connect automatically
   */
  enabled?: boolean

  /**
   * Event handler callback
   */
  onEvent?: (event: NeoTalkEvent) => void

  /**
   * Execution started callback
   */
  onExecutionStarted?: (data: AgentExecutionStartedEvent['data']) => void

  /**
   * Execution completed callback
   */
  onExecutionCompleted?: (data: AgentExecutionCompletedEvent['data']) => void

  /**
   * Thinking step callback
   */
  onThinking?: (data: AgentThinkingEvent['data']) => void

  /**
   * Decision made callback
   */
  onDecision?: (data: AgentDecisionEvent['data']) => void

  /**
   * Memory updated callback
   */
  onMemoryUpdated?: (data: AgentMemoryUpdatedEvent['data']) => void
}

export interface UseAgentEventsResult {
  /**
   * Whether the connection is active
   */
  isConnected: boolean

  /**
   * Current active execution (if any)
   */
  currentExecution: AgentExecution | null

  /**
   * All events received for this agent
   */
  events: NeoTalkEvent[]

  /**
   * Thinking steps in current execution
   */
  thinkingSteps: AgentThinkingStep[]

  /**
   * Decisions made in current execution
   */
  decisions: Array<{
    description: string
    rationale: string
    action: string
    confidence: number
    timestamp: number
  }>

  /**
   * Clear events buffer
   */
  clearEvents: () => void

  /**
   * Manually reconnect
   */
  reconnect: () => void
}

const AGENT_EVENT_TYPES = [
  'AgentExecutionStarted',
  'AgentExecutionCompleted',
  'AgentThinking',
  'AgentDecision',
  'AgentMemoryUpdated',
] as const

/**
 * useAgentEvents - Hook for subscribing to agent-specific events
 *
 * Filters the global event stream to only include events for the specified agent.
 *
 * @example
 * ```tsx
 * function AgentMonitor({ agentId }: { agentId: string }) {
 *   const { currentExecution, thinkingSteps } = useAgentEvents({
 *     agentId,
 *     enabled: true,
 *     onThinking: (data) => console.log('Thinking:', data)
 *   })
 *
 *   return (
 *     <div>
 *       {currentExecution && (
 *         <div>Execution {currentExecution.id}</div>
 *       )}
 *       {thinkingSteps.map(step => (
 *         <div key={step.step_number}>{step.description}</div>
 *       ))}
 *     </div>
 *   )
 * }
 * ```
 */
export function useAgentEvents(
  agentId: string,
  options: UseAgentEventsOptions = {}
): UseAgentEventsResult {
  const {
    enabled = true,
    eventTypes,
    onEvent,
    onExecutionStarted,
    onExecutionCompleted,
    onThinking,
    onDecision,
    onMemoryUpdated,
  } = options

  // Use global events hook with agent filter
  const { isConnected, events: allEvents, clearEvents, reconnect } = useEvents({
    enabled,
    onEvent: (event) => {
      // Filter events for this agent
      const eventData = event.data as { agent_id?: string }
      if (eventData.agent_id !== agentId) return

      if (eventTypes && !eventTypes.includes(event.type as any)) return

      // Call specific handlers
      switch (event.type) {
        case 'AgentExecutionStarted':
          onExecutionStarted?.(event.data as AgentExecutionStartedEvent['data'])
          break
        case 'AgentExecutionCompleted':
          onExecutionCompleted?.(event.data as AgentExecutionCompletedEvent['data'])
          break
        case 'AgentThinking':
          onThinking?.(event.data as AgentThinkingEvent['data'])
          break
        case 'AgentDecision':
          onDecision?.(event.data as AgentDecisionEvent['data'])
          break
        case 'AgentMemoryUpdated':
          onMemoryUpdated?.(event.data as AgentMemoryUpdatedEvent['data'])
          break
      }

      onEvent?.(event)
    },
  })

  // Filter events for this agent
  const agentEvents = allEvents.filter((event) => {
    const eventData = event.data as { agent_id?: string }
    return eventData.agent_id === agentId
  })

  // Track current execution
  const [currentExecution, setCurrentExecution] = useState<AgentExecution | null>(null)

  // Process agent events to maintain execution state
  useEffect(() => {
    let execution: AgentExecution | null = null

    for (const event of agentEvents) {
      switch (event.type) {
        case 'AgentExecutionStarted': {
          const data = event.data as AgentExecutionStartedEvent['data']
          execution = {
            id: data.execution_id,
            agent_id: data.agent_id,
            trigger_type: data.trigger_type,
            started_at: event.timestamp,
            status: 'running',
            steps: [],
            decisions: [],
          }
          break
        }

        case 'AgentThinking': {
          const data = event.data as AgentThinkingEvent['data']
          if (execution && execution.id === data.execution_id) {
            const step: AgentThinkingStep = {
              step_number: data.step_number,
              step_type: data.step_type,
              description: data.description,
              details: data.details,
              timestamp: event.timestamp,
              status: 'completed',
            }
            execution.steps = [...execution.steps, step]
          }
          break
        }

        case 'AgentDecision': {
          const data = event.data as AgentDecisionEvent['data']
          if (execution && execution.id === data.execution_id) {
            const decision = {
              description: data.description,
              rationale: data.rationale,
              action: data.action,
              confidence: data.confidence,
              timestamp: event.timestamp,
            }
            execution.decisions = [...execution.decisions, decision]
          }
          break
        }

        case 'AgentExecutionCompleted': {
          const data = event.data as AgentExecutionCompletedEvent['data']
          if (execution && execution.id === data.execution_id) {
            execution.completed_at = event.timestamp
            execution.duration_ms = data.duration_ms
            execution.status = data.success ? 'completed' : 'failed'
          }
          break
        }
      }
    }

    if (execution) {
      setCurrentExecution(execution)
    }
  }, [agentEvents])

  // Extract thinking steps and decisions
  const thinkingSteps = currentExecution?.steps || []
  const decisions = currentExecution?.decisions || []

  return {
    isConnected,
    currentExecution,
    events: agentEvents,
    thinkingSteps,
    decisions,
    clearEvents,
    reconnect,
  }
}

/**
 * useAgentStatus - Hook for monitoring agent status via polling
 *
 * Falls back to polling when real-time events are not available.
 */
export function useAgentStatus(agentId: string, options: { enabled?: boolean; interval?: number } = {}) {
  const { enabled = true, interval = 5000 } = options
  const [status, setStatus] = useState<string | null>(null)
  const [currentExecutionId, setCurrentExecutionId] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (!enabled || !agentId) return

    let cancelled = false

    const fetchStatus = async () => {
      try {
        const response = await fetch(`/api/agents/${agentId}`)
        if (!response.ok) throw new Error('Failed to fetch agent status')
        const data = await response.json()
        if (!cancelled) {
          setStatus(data.status)
          // Check if there's an active execution
          if (data.status === 'Executing') {
            // Fetch executions to get current one
            const execResponse = await fetch(`/api/agents/${agentId}/executions?limit=1`)
            if (execResponse.ok) {
              const execData = await execResponse.json()
              if (execData.executions && execData.executions.length > 0) {
                const latest = execData.executions[0]
                if (latest.status === 'Running') {
                  setCurrentExecutionId(latest.id)
                }
              }
            }
          } else {
            setCurrentExecutionId(null)
          }
        }
      } catch (error) {
        console.error('Failed to fetch agent status:', error)
      } finally {
        setLoading(false)
      }
    }

    setLoading(true)
    fetchStatus()

    const intervalId = setInterval(fetchStatus, interval)

    return () => {
      cancelled = true
      clearInterval(intervalId)
    }
  }, [agentId, enabled, interval])

  return { status, currentExecutionId, loading }
}

// useAgentEvents Hook for Agent-specific real-time events
//
// Provides filtered event streaming for a specific agent.

import { useEffect, useState, useRef } from 'react'
import type {
  NeoMindEvent,
  AgentThinkingEvent,
  AgentDecisionEvent,
  AgentExecutionStartedEvent,
  AgentExecutionCompletedEvent,
  AgentMemoryUpdatedEvent,
} from '@/lib/events'
import { useEvents } from './useEvents'
import { api } from '@/lib/api'

export interface AgentThinkingStep {
  step_number: number
  step_type: string
  description: string
  details?: unknown
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
  onEvent?: (event: NeoMindEvent) => void

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
  events: NeoMindEvent[]

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

const _AGENT_EVENT_TYPES = [
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

  // Track current execution
  const [currentExecution, setCurrentExecution] = useState<AgentExecution | null>(null)
  const executionRef = useRef<AgentExecution | null>(null)

  // Keep ref in sync with state
  useEffect(() => {
    executionRef.current = currentExecution
  }, [currentExecution])

  // Use global events hook with agent filter
  const { isConnected, clearEvents, reconnect } = useEvents({
    enabled,
    onConnected: (connected) => {
      if (!connected && executionRef.current?.status === 'running') {
        // Connection lost - clear running execution state
        setCurrentExecution(null)
      }
    },
    onEvent: (event) => {
      // Filter events for this agent
      const eventData = event.data as { agent_id?: string }
      if (eventData.agent_id !== agentId) return

      if (eventTypes && !eventTypes.includes(event.type as any)) return

      // Incrementally update execution state
      switch (event.type) {
        case 'AgentExecutionStarted': {
          const data = event.data as AgentExecutionStartedEvent['data']
          const execution: AgentExecution = {
            id: data.execution_id,
            agent_id: data.agent_id,
            trigger_type: data.trigger_type,
            started_at: event.timestamp,
            status: 'running',
            steps: [],
            decisions: [],
          }
          setCurrentExecution(execution)
          onExecutionStarted?.(data)
          break
        }

        case 'AgentThinking': {
          const data = event.data as AgentThinkingEvent['data']
          setCurrentExecution(prev => {
            if (!prev || prev.id !== data.execution_id) return prev
            return {
              ...prev,
              steps: [...prev.steps, {
                step_number: data.step_number,
                step_type: data.step_type,
                description: data.description,
                details: data.details,
                timestamp: event.timestamp,
                status: 'completed',
              }],
            }
          })
          onThinking?.(data)
          break
        }

        case 'AgentDecision': {
          const data = event.data as AgentDecisionEvent['data']
          setCurrentExecution(prev => {
            if (!prev || prev.id !== data.execution_id) return prev
            return {
              ...prev,
              decisions: [...prev.decisions, {
                description: data.description,
                rationale: data.rationale,
                action: data.action,
                confidence: data.confidence,
                timestamp: event.timestamp,
              }],
            }
          })
          onDecision?.(data)
          break
        }

        case 'AgentExecutionCompleted': {
          const data = event.data as AgentExecutionCompletedEvent['data']
          setCurrentExecution(prev => {
            if (!prev || prev.id !== data.execution_id) return prev
            return {
              ...prev,
              completed_at: event.timestamp,
              duration_ms: data.duration_ms,
              status: data.success ? 'completed' : 'failed',
            }
          })
          onExecutionCompleted?.(data)
          break
        }

        case 'AgentMemoryUpdated':
          onMemoryUpdated?.(event.data as AgentMemoryUpdatedEvent['data'])
          break
      }

      onEvent?.(event)
    },
  })

  // Extract thinking steps and decisions
  const thinkingSteps = currentExecution?.steps || []
  const decisions = currentExecution?.decisions || []

  return {
    isConnected,
    currentExecution,
    events: [],
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

    const abortController = new AbortController()

    const fetchStatus = async () => {
      try {
        const data = await api.getAgent(agentId)
        if (abortController.signal.aborted) return

        setStatus(data.status || null)
        // Check if there's an active execution
        if (data.status === 'Executing') {
          try {
            const execData = await api.getAgentExecutions(agentId, 1)
            if (abortController.signal.aborted) return
            if (execData.executions && execData.executions.length > 0) {
              const latest = execData.executions[0]
              if (latest.status === 'Running') {
                setCurrentExecutionId(latest.id)
              }
            }
          } catch {
            // Ignore execution fetch errors
          }
        } else {
          setCurrentExecutionId(null)
        }
      } catch {
        if (!abortController.signal.aborted) {
          setStatus(null)
        }
      } finally {
        setLoading(false)
      }
    }

    setLoading(true)
    fetchStatus()

    const intervalId = setInterval(fetchStatus, interval)

    return () => {
      abortController.abort()
      clearInterval(intervalId)
    }
  }, [agentId, enabled, interval])

  return { status, currentExecutionId, loading }
}

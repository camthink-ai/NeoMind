/**
 * useChatStream — the shared chat stream state machine.
 *
 * One implementation of the WebSocket event → state interpretation
 * (Thinking / Content / ToolCallStart / ToolCallEnd / IntermediateEnd /
 * end / cancelled / Error / Warning) consumed by BOTH chat surfaces
 * (the full chat page and the side panel). Before this hook existed the
 * two views carried two hand-maintained copies that had already drifted
 * (the "frontend pair fix" tax: every stream bug had to be fixed twice,
 * and sometimes was only fixed on one side).
 *
 * Semantics are the chat page's (the superset):
 * - thinking accumulates ACROSS rounds (live view = completed rounds +
 *   current round), per-round values also exposed via streamingRoundThinking
 * - the final round's content becomes the message content; only intermediate
 *   rounds go into roundContents
 * - generationMs (wall time from first streamed event) rides the result
 *
 * View-specific behavior stays in the view via callbacks: persisting the
 * finished message (onEnd), error/warning rendering policy (onError keeps
 * the stream alive — the backend may send a fallback summary before end),
 * token-usage persistence (onTokenUsage), Progress/Plan events and
 * session_created/session_switched (onRawEvent).
 *
 * High-frequency Content/Thinking chunks are accumulated in refs and flushed
 * to state at most once per animation frame — per-token markdown re-parses
 * were a real jank source on weak edge hardware. Low-frequency events
 * (tool calls, round ends, end/cancelled) flush immediately. A stale flush
 * can never resurrect state after a reset: every reset bumps an epoch the
 * scheduled flush re-checks.
 */

import { useCallback, useEffect, useRef, useState } from "react"
import { generateId } from "@/lib/id"
import { ws } from "@/lib/websocket"
import type { ServerMessage } from "@/types"

export interface StreamTokenUsage {
  promptTokens: number
  systemPromptTokens?: number
  toolTokens?: number
}

/** What the view needs to persist the finished assistant turn. */
export interface StreamEndResult {
  /** Stable id — matches the id the streaming bubble rendered under, so the
   *  transition from streaming to saved message does not remount. */
  id: string
  content: string
  /** Cumulative thinking across all rounds (undefined when none). */
  thinking?: string
  toolCalls: any[]
  /** Intermediate rounds only — the final round's content is `content`. */
  roundContents?: Record<number, string>
  roundThinking?: Record<number, string>
  /** Wall time from the first streamed event to end, ms. */
  generationMs?: number
}

export interface UseChatStreamOptions {
  /** Session this surface is currently bound to. Events tagged with a
   *  DIFFERENT sessionId are dropped — the chat page and the side panel
   *  share one WebSocket, so without this filter a turn streaming in the
   *  panel also drives the page's machine (phantom bubbles) and vice
   *  versa. Undefined/null accepts everything (backward compatible). */
  sessionId?: string | null
  /** The server finalized the turn — the view persists the message. */
  onEnd: (result: StreamEndResult) => void
  /** Server error mid-stream. The stream is NOT auto-reset: the backend may
   *  still send a fallback summary followed by `end`. */
  onError?: (message: string) => void
  /** Non-blocking warning event. */
  onWarning?: (message: string) => void
  /** Token usage reported by the `end` event, when present. */
  onTokenUsage?: (usage: StreamTokenUsage, sessionId?: string) => void
  /** Server acknowledged __CANCEL__ (covers cross-tab cancels). State is
   *  already reset when this fires. */
  onCancelled?: () => void
  /** Events the machine does not interpret (Progress, Plan,
   *  session_created, session_switched, …). */
  onRawEvent?: (data: ServerMessage) => void
}

export function useChatStream(options: UseChatStreamOptions) {
  // Keep callbacks in refs so the ws subscription never resubscribes on a
  // view re-render (a resubscribe gap mid-stream drops events).
  const optionsRef = useRef(options)
  useEffect(() => { optionsRef.current = options })

  const [isStreaming, setIsStreaming] = useState(false)
  const [streamingMessageId, setStreamingMessageId] = useState<string | null>(null)
  // Flushed at most once per frame (see scheduleFlush)
  const [streamingContent, setStreamingContent] = useState("")
  const [streamingThinking, setStreamingThinking] = useState("")
  // Immediate — tool events are rare
  const [streamingToolCalls, setStreamingToolCalls] = useState<any[]>([])
  const [roundContents, setRoundContents] = useState<Record<number, string>>({})
  const [streamingRoundThinking, setStreamingRoundThinking] = useState<Record<number, string>>({})
  const [currentRound, setCurrentRound] = useState(1)

  // -- Synchronous accumulators (the truth; states are projections) --
  const accRef = useRef({ content: "", thinking: "", toolCalls: [] as any[] })
  const currentRoundRef = useRef(1)
  const roundContentsAccRef = useRef<Record<number, string>>({})
  const thinkingAccRef = useRef("")
  const roundThinkingAccRef = useRef<Record<number, string>>({})
  const streamStartRef = useRef<number | null>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  const isStreamingRef = useRef(false)

  // -- Throttled flush machinery --
  const epochRef = useRef(0)
  const flushScheduledRef = useRef(false)

  /** Completed-round thinking + current round's live thinking. */
  const liveThinking = () =>
    thinkingAccRef.current + accRef.current.thinking

  const flushNow = useCallback((epoch: number) => {
    if (epoch !== epochRef.current) return
    setStreamingContent(accRef.current.content)
    setStreamingThinking(liveThinking())
  }, [])

  const scheduleFlush = useCallback(() => {
    if (flushScheduledRef.current) return
    flushScheduledRef.current = true
    const epoch = epochRef.current
    const run = () => {
      flushScheduledRef.current = false
      flushNow(epoch)
    }
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(run)
    else setTimeout(run, 60)
  }, [flushNow])

  const hardReset = useCallback(() => {
    epochRef.current += 1 // kills any scheduled flush
    accRef.current = { content: "", thinking: "", toolCalls: [] }
    currentRoundRef.current = 1
    roundContentsAccRef.current = {}
    thinkingAccRef.current = ""
    roundThinkingAccRef.current = {}
    streamStartRef.current = null
    streamingMessageIdRef.current = null
    isStreamingRef.current = false
    setIsStreaming(false)
    setStreamingMessageId(null)
    setStreamingContent("")
    setStreamingThinking("")
    setStreamingToolCalls([])
    setRoundContents({})
    setStreamingRoundThinking({})
    setCurrentRound(1)
  }, [])

  /** Start a turn — call right before ws.sendMessage. Returns the id the
   *  finished message will keep (pass to nothing; it rides the result). */
  const beginTurn = useCallback((): string => {
    hardReset()
    const id = generateId()
    streamingMessageIdRef.current = id
    isStreamingRef.current = true
    setIsStreaming(true)
    setStreamingMessageId(id)
    return id
  }, [hardReset])

  /** Restore a detached turn after reconnect (pending-stream recovery). */
  const restore = useCallback((content: string, thinking: string) => {
    isStreamingRef.current = true
    setIsStreaming(true)
    accRef.current.content = content
    accRef.current.thinking = thinking
    streamStartRef.current = Date.now()
    flushNow(epochRef.current)
  }, [flushNow])

  const markStreamStart = () => {
    if (streamStartRef.current === null) streamStartRef.current = Date.now()
    if (!isStreamingRef.current) {
      isStreamingRef.current = true
      setIsStreaming(true)
    }
  }

  const handleEvent = useCallback((data: ServerMessage) => {
    // Session filter: one WS connection serves both chat surfaces; each
    // machine only processes events belonging to ITS session. Events without
    // a sessionId (legacy variants) always pass; session_created/switched are
    // exempt — they are control-plane events whose sessionId names the NEW
    // session to switch TO, i.e. the whole point is that it differs.
    const boundSession = optionsRef.current.sessionId
    const eventSession = (data as { sessionId?: string }).sessionId
    if (
      boundSession && eventSession && eventSession !== boundSession &&
      data.type !== "session_created" && data.type !== "session_switched"
    ) return

    switch (data.type) {
      case "Thinking":
        markStreamStart()
        accRef.current.thinking += data.content || ""
        scheduleFlush()
        break

      case "Content":
        markStreamStart()
        accRef.current.content += data.content || ""
        scheduleFlush()
        break

      case "ToolCallStart": {
        markStreamStart()
        const toolCall = {
          id: generateId(),
          name: data.tool,
          arguments: data.arguments,
          result: null,
          round: data.round ?? currentRoundRef.current,
        }
        accRef.current.toolCalls = [...accRef.current.toolCalls, toolCall]
        setStreamingToolCalls([...accRef.current.toolCalls])
        break
      }

      case "ToolCallEnd": {
        const updated = [...accRef.current.toolCalls]
        const idx = updated.findIndex(tc => tc.name === data.tool && tc.result === null)
        if (idx !== -1) {
          updated[idx] = { ...updated[idx], result: data.result }
          accRef.current.toolCalls = updated
          setStreamingToolCalls([...updated])
        }
        break
      }

      case "IntermediateEnd":
      case "intermediate_end": {
        if (accRef.current.content) {
          roundContentsAccRef.current[currentRoundRef.current] = accRef.current.content
        }
        if (accRef.current.thinking) {
          thinkingAccRef.current += accRef.current.thinking
          roundThinkingAccRef.current[currentRoundRef.current] = accRef.current.thinking
        }
        // Next round starts with a clean slate for content/thinking; the
        // live thinking view keeps showing completed rounds via the
        // accumulator (see liveThinking).
        accRef.current.content = ""
        accRef.current.thinking = ""
        currentRoundRef.current += 1
        setCurrentRound(currentRoundRef.current)
        setRoundContents({ ...roundContentsAccRef.current })
        setStreamingRoundThinking({ ...roundThinkingAccRef.current })
        flushNow(epochRef.current)
        break
      }

      case "end": {
        if (data.tokenUsage?.promptTokens) {
          optionsRef.current.onTokenUsage?.(
            {
              promptTokens: data.tokenUsage.promptTokens,
              systemPromptTokens: data.tokenUsage.systemPromptTokens,
              toolTokens: data.tokenUsage.toolTokens,
            },
            data.sessionId,
          )
        }
        const toolCalls = accRef.current.toolCalls
        const thinking = thinkingAccRef.current + accRef.current.thinking
        const content = accRef.current.content
        const hasRoundContents = Object.keys(roundContentsAccRef.current).length > 0
        const hasRoundThinking = Object.keys(roundThinkingAccRef.current).length > 0
        const generationMs = streamStartRef.current !== null
          ? Date.now() - streamStartRef.current
          : undefined
        const result: StreamEndResult = {
          id: streamingMessageIdRef.current || generateId(),
          content,
          thinking: thinking || undefined,
          toolCalls,
          roundContents: hasRoundContents ? { ...roundContentsAccRef.current } : undefined,
          roundThinking: hasRoundThinking ? { ...roundThinkingAccRef.current } : undefined,
          generationMs,
        }
        hardReset()
        if (result.content || result.thinking || result.toolCalls.length > 0) {
          optionsRef.current.onEnd(result)
        }
        break
      }

      case "cancelled":
        // No trailing 'end' is guaranteed on this path — reset here or the
        // composer stays locked and the bubble spins forever.
        hardReset()
        optionsRef.current.onCancelled?.()
        break

      case "Error":
        // Do NOT reset — the backend may send a fallback summary after the
        // error; `end` finalizes.
        optionsRef.current.onError?.(data.message || "An error occurred during processing")
        break

      case "Warning":
        optionsRef.current.onWarning?.(data.message || "Warning")
        break

      default:
        optionsRef.current.onRawEvent?.(data)
        break
    }
  }, [hardReset, scheduleFlush, flushNow])

  // Subscribe for the hook's lifetime; the handler is stable.
  useEffect(() => {
    const unsubscribe = ws.onMessage(handleEvent)
    return () => {
      epochRef.current += 1
      void unsubscribe()
    }
  }, [handleEvent])

  return {
    // state
    isStreaming,
    isStreamingRef,
    streamingMessageId,
    streamingContent,
    streamingThinking,
    streamingToolCalls,
    roundContents,
    streamingRoundThinking,
    currentRound,
    // actions
    beginTurn,
    hardReset,
    restore,
  }
}

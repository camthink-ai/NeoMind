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
import type { ServerMessage, ToolCall } from "@/types"

export interface StreamTokenUsage {
  promptTokens: number
  systemPromptTokens?: number
  toolTokens?: number
  /** Session model's real context window (meter denominator). */
  maxContextTokens?: number
}

/** Snapshot extras for a detached-turn restore: the pending stream's tool
 *  process and per-round narration. Without them a restored bubble shows
 *  only thinking+content and loses the tool-process display. */
export interface RestoreExtras {
  toolCalls?: ToolCall[]
  roundContents?: Record<number, string>
  /** Server-side round the snapshot is in (1-based) — adopted so a mid-round
   *  restore doesn't write round N narration into the round-1 slot. */
  currentRound?: number
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
  /** Runtime guard notes (incomplete action, budget spent) — rendered as
   *  system callouts, never part of `content`. */
  systemNotes?: string[]
  /** Wall time from the first streamed event to end, ms. */
  generationMs?: number
  /** True when the accumulator was reset mid-turn (session switch away and
   *  back, detach/restore) — the fields above are PARTIAL by construction.
   *  The view should finalize from server history instead of this result. */
  interrupted?: boolean
  /** The server dropped older history this turn to fit the context window. */
  historyTruncated?: boolean
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
  /** The turn ended without a terminal frame (lost end/cancelled, or an
   *  Error whose fallback+end never arrived — e.g. the server restarted
   *  mid-turn). The view should take server history as the truth. */
  onStalled?: (reason: string) => void
}

/** A turn that stops producing events for this long is dead (the server's
 *  own stream timeout is 1200s; a long tool runs silently between
 *  ToolCallStart and ToolCallEnd, so this must not undercut legitimate
 *  multi-minute tools). */
const STREAM_IDLE_MS = 10 * 60_000
/** Absolute turn cap — slightly above the backend's 1200s stream timeout. */
const STREAM_TOTAL_MS = 21 * 60_000
/** Error frames are followed by a fallback summary + end; give them this
 *  long before settling the view. */
const ERROR_GRACE_MS = 30_000

export function useChatStream(options: UseChatStreamOptions) {
  // Keep callbacks in refs so the ws subscription never resubscribes on a
  // view re-render (a resubscribe gap mid-stream drops events).
  const optionsRef = useRef(options)
  useEffect(() => { optionsRef.current = options })

  const [isStreaming, setIsStreaming] = useState(false)
  // True after the first server event for the active turn. The panel uses
  // this to distinguish local/session setup from waiting on the backend.
  const [hasReceivedEvent, setHasReceivedEvent] = useState(false)
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
  const systemNotesAccRef = useRef<string[]>([])
  const streamStartRef = useRef<number | null>(null)
  const streamingMessageIdRef = useRef<string | null>(null)
  const isStreamingRef = useRef(false)
  // Accumulator-continuity flag: set by every hardReset/restore, cleared by
  // beginTurn. When an `end` arrives with this set, the assembled result is
  // missing whatever streamed before the reset — the view must take the
  // server's persisted message instead.
  const interruptedRef = useRef(false)

  // -- Throttled flush machinery --
  const epochRef = useRef(0)
  const flushScheduledRef = useRef(false)

  // -- Terminal-frame watchdog --
  const watchdogRef = useRef<ReturnType<typeof setInterval> | null>(null)
  const lastEventAtRef = useRef(0)
  const turnStartAtRef = useRef(0)
  const errorAtRef = useRef<number | null>(null)

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
    interruptedRef.current = true
    accRef.current = { content: "", thinking: "", toolCalls: [] }
    currentRoundRef.current = 1
    roundContentsAccRef.current = {}
    thinkingAccRef.current = ""
    roundThinkingAccRef.current = {}
    systemNotesAccRef.current = []
    streamStartRef.current = null
    streamingMessageIdRef.current = null
    isStreamingRef.current = false
    setIsStreaming(false)
    setHasReceivedEvent(false)
    setStreamingMessageId(null)
    setStreamingContent("")
    setStreamingThinking("")
    setStreamingToolCalls([])
    setRoundContents({})
    setStreamingRoundThinking({})
    setCurrentRound(1)
  }, [])

  const stopWatchdog = useCallback(() => {
    if (watchdogRef.current !== null) {
      clearInterval(watchdogRef.current)
      watchdogRef.current = null
    }
    errorAtRef.current = null
  }, [])

  /** Start the terminal-frame watchdog for a turn. Every event refreshes
   *  lastEventAtRef; the interval settles the view if the server goes silent
   *  (restart mid-turn) or sends Error without ever following with end —
   *  the spinner that used to require a page refresh/session switch. */
  const startWatchdog = useCallback(() => {
    stopWatchdog()
    turnStartAtRef.current = Date.now()
    lastEventAtRef.current = Date.now()
    watchdogRef.current = setInterval(() => {
      if (!isStreamingRef.current) {
        stopWatchdog()
        return
      }
      const now = Date.now()
      if (errorAtRef.current !== null && now - errorAtRef.current > ERROR_GRACE_MS) {
        stopWatchdog()
        hardReset()
        optionsRef.current.onStalled?.("error without terminal event")
        return
      }
      if (now - lastEventAtRef.current > STREAM_IDLE_MS) {
        stopWatchdog()
        hardReset()
        optionsRef.current.onStalled?.("stream idle")
        return
      }
      if (now - turnStartAtRef.current > STREAM_TOTAL_MS) {
        stopWatchdog()
        hardReset()
        optionsRef.current.onStalled?.("stream total timeout")
      }
    }, 5000)
  }, [hardReset, stopWatchdog])

  /** Start a turn — call right before ws.sendMessage. Returns the id the
   *  finished message will keep (pass to nothing; it rides the result). */
  const beginTurn = useCallback((messageId?: string): string => {
    hardReset()
    interruptedRef.current = false // fresh turn — accumulator is continuous
    const id = messageId || generateId()
    streamingMessageIdRef.current = id
    isStreamingRef.current = true
    setIsStreaming(true)
    setStreamingMessageId(id)
    startWatchdog()
    return id
  }, [hardReset, startWatchdog])

  /** Restore a detached turn after reconnect (pending-stream recovery).
   *  `extras` carries the snapshot's tool calls and per-round narration —
   *  without them a restored bubble shows only thinking+content and the
   *  tool process (the dominant visual of a tool-loop turn) is missing. */
  const restore = useCallback((
    content: string,
    thinking: string,
    extras?: RestoreExtras,
  ) => {
    isStreamingRef.current = true
    setIsStreaming(true)
    setHasReceivedEvent(!!(content || thinking || extras?.toolCalls?.length))
    interruptedRef.current = true // reattached turn: the end event will be partial
    accRef.current.content = content
    // The snapshot's thinking is the WHOLE turn's accumulation (the server
    // never rotates it into rounds). Keep the invariant "effective thinking
    // = thinkingAcc + current-round" equal to the snapshot by clearing the
    // completed-rounds accumulator: the next intermediate_end moves it from
    // the current-round slot into the accumulator verbatim instead of
    // re-adding a full copy per round (the compounding-doubling bug).
    thinkingAccRef.current = ""
    accRef.current.thinking = thinking
    if (extras?.toolCalls) {
      accRef.current.toolCalls = [...extras.toolCalls]
      setStreamingToolCalls([...extras.toolCalls])
    }
    if (extras?.roundContents) {
      roundContentsAccRef.current = { ...extras.roundContents }
      setRoundContents({ ...extras.roundContents })
    }
    // Resume into the SERVER's round: without this the counter stayed at 1
    // and the next intermediate_end wrote round N's narration into the
    // round-1 slot, clobbering the recovered round 1.
    if (extras?.currentRound && extras.currentRound > 0) {
      currentRoundRef.current = extras.currentRound
      setCurrentRound(extras.currentRound)
    }
    streamStartRef.current = Date.now()
    flushNow(epochRef.current)
    // A restored detached turn needs the same terminal-frame guarantee.
    // Don't restart a running watchdog: restoreIfBehind re-adopts the
    // snapshot on every poll, and resetting the clocks would let a dead
    // turn poll forever.
    if (watchdogRef.current === null) startWatchdog()
  }, [flushNow, startWatchdog])

  /** Pending-stream poll helper: adopt the server snapshot only when it is
   *  AHEAD of the local accumulator. In the re-attached (live) case local
   *  events are fresher than the ≤2s-lagged server snapshot; in the
   *  detached case (hard refresh) local never grows and the snapshot is the
   *  only progress source. Length comparison is safe: both sides are plain
   *  delta concatenations that share the same prefix. Tool-call COUNT is
   *  part of "ahead": during a slow tool execution text does not grow, and
   *  a detached page's only signal that the turn is alive is a new call. */
  const restoreIfBehind = useCallback((
    content: string,
    thinking: string,
    extras?: RestoreExtras,
  ) => {
    const localCalls = accRef.current.toolCalls.length
    const snapCalls = extras?.toolCalls?.length ?? 0
    // Thinking must compare against the ACCUMULATED total (completed rounds
    // + current round): the server snapshot carries the whole turn's
    // thinking, and after a round boundary the current-round slot alone is
    // "" — comparing against it made every poll look "behind" and re-restore
    // the full snapshot, duplicating old rounds' thinking compounding.
    const localThinkingLen =
      thinkingAccRef.current.length + accRef.current.thinking.length
    if (
      content.length > accRef.current.content.length ||
      thinking.length > localThinkingLen ||
      snapCalls > localCalls
    ) {
      restore(content, thinking, extras)
    }
  }, [restore])

  const markStreamStart = () => {
    if (streamStartRef.current === null) streamStartRef.current = Date.now()
    setHasReceivedEvent(true)
    if (!isStreamingRef.current) {
      isStreamingRef.current = true
      setIsStreaming(true)
    }
  }

  // ── Cross-session streaming isolation ──
  // When sessionId changes (user switches session), the streaming state
  // belongs to the PREVIOUS session — reset it. Without this, the thinking
  // dots / partial content from session A bleed into session B's display.
  // The actual WS events are already filtered by sessionId (line ~195); this
  // clears the UI projection. If the user switches BACK to a still-streaming
  // session, the next event from the WS will rebuild the display state.
  const prevSessionRef = useRef(options.sessionId)
  useEffect(() => {
    const current = optionsRef.current.sessionId
    if (prevSessionRef.current !== current) {
      if (isStreamingRef.current || streamingMessageIdRef.current) {
        hardReset()
      }
      prevSessionRef.current = current
    }
    // hardReset is a stable useCallback (empty deps) — listing it satisfies
    // exhaustive-deps without changing when this effect fires.
  }, [options.sessionId, hardReset])

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

    lastEventAtRef.current = Date.now()
    if (data.type !== "Error") errorAtRef.current = null

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
          startedAt: Date.now(),
        }
        accRef.current.toolCalls = [...accRef.current.toolCalls, toolCall]
        setStreamingToolCalls([...accRef.current.toolCalls])
        break
      }

      case "ToolCallEnd": {
        const updated = [...accRef.current.toolCalls]
        const idx = updated.findIndex(tc => tc.name === data.tool && tc.result === null)
        if (idx !== -1) {
          const startedAt = updated[idx].startedAt
          updated[idx] = {
            ...updated[idx],
            result: data.result,
            success: data.success,
            durationMs: startedAt != null ? Date.now() - startedAt : undefined,
          }
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
        stopWatchdog()
        if (data.tokenUsage?.promptTokens) {
          optionsRef.current.onTokenUsage?.(
            {
              promptTokens: data.tokenUsage.promptTokens,
              systemPromptTokens: data.tokenUsage.systemPromptTokens,
              toolTokens: data.tokenUsage.toolTokens,
              maxContextTokens: data.tokenUsage.maxContextTokens,
            },
            data.sessionId,
          )
        }
        const toolCalls = accRef.current.toolCalls
        const thinking = thinkingAccRef.current + accRef.current.thinking
        const content = accRef.current.content
        const hasRoundContents = Object.keys(roundContentsAccRef.current).length > 0
        const hasRoundThinking = Object.keys(roundThinkingAccRef.current).length > 0
        const systemNotes = systemNotesAccRef.current.length > 0
          ? [...systemNotesAccRef.current]
          : undefined
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
          systemNotes,
          generationMs,
          historyTruncated: data.historyTruncated || undefined,
          interrupted: interruptedRef.current || undefined,
        }
        hardReset()
        // An interrupted turn finalizes even with an empty accumulator —
        // the view needs the end signal to reload the server's full message.
        if (result.interrupted || result.content || result.thinking || result.toolCalls.length > 0) {
          optionsRef.current.onEnd(result)
        }
        break
      }

      case "response": {
        // Non-streaming fallback (the streaming call failed — e.g. a context
        // overflow — and the backend answered via process_message_with_backend).
        // The reply is persisted server-side, but the stream machine used to
        // ignore this frame, so it only appeared after a session re-entry.
        stopWatchdog()
        const content = data.content || ""
        const result: StreamEndResult = {
          id: streamingMessageIdRef.current || generateId(),
          content,
          toolCalls: [],
          generationMs: typeof data.processingTimeMs === "number" ? data.processingTimeMs : undefined,
        }
        hardReset()
        if (content) optionsRef.current.onEnd(result)
        break
      }

      case "cancelled":
        // No trailing 'end' is guaranteed on this path — reset here or the
        // composer stays locked and the bubble spins forever.
        stopWatchdog()
        hardReset()
        optionsRef.current.onCancelled?.()
        break

      case "Error":
        // Do NOT reset — the backend may send a fallback summary after the
        // error; `end` finalizes. The watchdog gives that fallback a bounded
        // grace period so a missing end cannot spin forever.
        errorAtRef.current = Date.now()
        optionsRef.current.onError?.(data.message || "An error occurred during processing")
        break

      case "Warning":
        optionsRef.current.onWarning?.(data.message || "Warning")
        break

      case "SystemNote":
        // Guard note for the user — carried on the finished message (the
        // callout renders with the final bubble), not streamed as content.
        markStreamStart()
        systemNotesAccRef.current.push(data.note || "")
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
      stopWatchdog()
      void unsubscribe()
    }
  }, [handleEvent, stopWatchdog])

  return {
    // state
    isStreaming,
    hasReceivedEvent,
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
    restoreIfBehind,
  }
}

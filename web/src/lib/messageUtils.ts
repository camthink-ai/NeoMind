/**
 * Message Utilities - Shared logic for message handling
 *
 * This module contains utilities for working with messages,
 * including deduplication and merging of fragmented responses.
 */

import type { Message } from "@/types"
import { normalizeTimestampMs } from "@/lib/utils/format"

/**
 * Normalize a history batch to the store's canonical ms timestamps.
 *
 * Server history carries Unix SECONDS (AgentMessage.timestamp =
 * chrono::Utc::now().timestamp()); locally created messages carry ms.
 * Display helpers tolerate both, but arithmetic on message.timestamp does
 * not — `Date.now() - serverSecondsTs` produced a 497,289-hour generation
 * time (2026-10-15). Every history ingest path funnels through
 * mergeMessagesForDisplay, so normalizing here canonicalizes the store.
 */
function normalizeTimestamps(messages: Message[]): Message[] {
  return messages.map(m => {
    if (typeof m.timestamp !== 'number') return m
    const ms = normalizeTimestampMs(m.timestamp)
    // Already-ms timestamps (and unparseables) keep their value.
    return ms != null && ms !== m.timestamp ? { ...m, timestamp: ms } : m
  })
}

/**
 * Combine two content strings without duplicating when backend sent the same content twice.
 */
export function dedupeContentConcat(a: string, b: string): string {
  const x = (a || '').trim()
  const y = (b || '').trim()
  if (!y) return a || ''
  if (!x) return b || ''
  if (x === y) return a
  if (x.endsWith(y)) return a
  if (y.startsWith(x)) return b
  if (x.includes(y)) return a
  return (a || '') + (b || '')
}

/**
 * Merge fragmented assistant messages for display.
 *
 * Same logic as sessionSlice.mergeAssistantMessages:
 * Reconstruct ONE message per user turn, combining all tool calls across rounds,
 * building round_contents, and using the final content as message content.
 */
export function mergeMessagesForDisplay(rawMessages: Message[]): Message[] {
  const messages = normalizeTimestamps(rawMessages)
  const result: Message[] = []

  for (let i = 0; i < messages.length; i++) {
    const msg = messages[i]

    // Skip tool messages (internal use)
    if ((msg as any).role === "tool") continue

    // User messages and system messages are kept as-is
    if (msg.role !== "assistant") {
      result.push(msg)
      continue
    }

    // Collect all assistant messages in this turn
    const turnMessages: Message[] = [msg]
    let j = i + 1
    while (j < messages.length) {
      const nextMsg = messages[j]
      if ((nextMsg as any).role === "tool") { j++; continue }
      if (nextMsg.role !== "assistant") break
      turnMessages.push(nextMsg)
      j++
    }

    const merged: Message = { ...msg, content: '' }
    let allToolCalls: any[] = []
    let roundContents: Record<number, string> = {}
    let roundThinking: Record<number, string> = {}
    let systemNotes: string[] = []
    let roundCounter = 1
    let finalContent = ''
    let thinking: string | undefined

    // Detect backend storage pattern
    const hasTrailingContentMsg = turnMessages.some(m =>
      !(m.tool_calls && m.tool_calls.length > 0) && !!(m.content && m.content.trim())
    )

    let lastToolCallMsgIndex = -1
    for (let k = turnMessages.length - 1; k >= 0; k--) {
      if (turnMessages[k].tool_calls && turnMessages[k].tool_calls!.length > 0) {
        lastToolCallMsgIndex = k
        break
      }
    }

    for (let k = 0; k < turnMessages.length; k++) {
      const cur = turnMessages[k]
      const hasTools = !!(cur.tool_calls && cur.tool_calls.length > 0)
      const hasContent = !!(cur.content && cur.content.trim())

      if (cur.thinking) {
        // Accumulate thinking across all messages in this turn (interleaved thinking pattern)
        thinking = thinking ? thinking + '\n' + cur.thinking : cur.thinking
      }
      if (cur.round_contents) {
        roundContents = { ...roundContents, ...cur.round_contents }
      }
      if (cur.round_thinking) {
        roundThinking = { ...roundThinking, ...cur.round_thinking }
      }
      // Raw API messages carry the backend's snake_case field (Message's
      // other history fields keep snake_case; systemNotes is the camelCase
      // UI field).
      {
        const rawNotes = (cur as Message & { system_notes?: string[] }).system_notes
        if (rawNotes) {
          systemNotes = [...systemNotes, ...rawNotes]
        }
      }

      if (hasTools) {
        const roundNum = cur.tool_calls![0]?.round ?? roundCounter
        allToolCalls = [...allToolCalls, ...cur.tool_calls!.map(tc => ({ ...tc, round: tc.round ?? roundNum }))]
        roundCounter = roundNum + 1

        if (hasContent) {
          if (hasTrailingContentMsg) {
            roundContents[roundNum] = cur.content!
          } else if (k === lastToolCallMsgIndex) {
            finalContent = cur.content!
          } else {
            roundContents[roundNum] = cur.content!
          }
        }
      } else if (hasContent) {
        finalContent = dedupeContentConcat(finalContent, cur.content!)
      }
    }

    merged.content = finalContent
    merged.thinking = thinking || undefined
    merged.tool_calls = allToolCalls.length > 0 ? allToolCalls : undefined
    merged.round_contents = Object.keys(roundContents).length > 0 ? roundContents : undefined
    merged.round_thinking = Object.keys(roundThinking).length > 0 ? roundThinking : undefined
    merged.systemNotes = systemNotes.length > 0 ? systemNotes : undefined

    if (merged.content || merged.thinking || merged.tool_calls) {
      result.push(merged)
    }

    i = j - 1
  }

  return result
}

/**
 * Drop the in-flight turn's messages: everything AFTER the last user
 * message. While a turn is streaming (fresh or restored-after-reentry),
 * the history endpoint serves the LIVE agent history — the running turn's
 * fragments are already in it and would double-render next to the
 * streaming bubble (the "extra Thinking process" live-test report). The
 * streaming bubble owns the in-flight turn's display; the completed turn
 * replaces it on end/reload.
 */
export function dropInFlightTurn(messages: Message[]): Message[] {
  let lastUser = -1
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === "user") {
      lastUser = i
      break
    }
  }
  if (lastUser === -1) return messages
  return messages.slice(0, lastUser + 1)
}

/**
 * Check if a message is a partial stream message.
 * Partial messages should not be displayed in the message list.
 */
export function isPartialMessage(message: Message): boolean {
  return (message as any).isPartial === true
}

/**
 * Filter out partial messages from a message list.
 */
export function filterPartialMessages(messages: Message[]): Message[] {
  return messages.filter(msg => !isPartialMessage(msg))
}

/**
 * Get a preview of the last assistant message from a session.
 * Used for session list display.
 */
export function getLastAssistantMessage(messages: Message[]): string | null {
  const assistantMessages = messages.filter(m => m.role === "assistant" && !isPartialMessage(m))
  if (assistantMessages.length === 0) return null

  const lastMsg = assistantMessages[assistantMessages.length - 1]
  if (lastMsg.content) {
    // Return first 100 chars
    return lastMsg.content.slice(0, 100) + (lastMsg.content.length > 100 ? "..." : "")
  }
  return null
}

/**
 * Remove embedded tool call JSON from content text.
 * Some models output tool call JSON in the content field (e.g. `[{"name":"device","arguments":...}]`)
 * wrapped in markdown code blocks or bare. This strips them for clean display.
 */
export function cleanToolCallJson(content: string): string {
  if (!content) return content

  let cleaned = content

  // Remove ```json ... ``` blocks containing tool call JSON.
  // Requires "arguments" as well as "name": a fenced DATA array (a device
  // list quoted as ```json [{"id","name","online"}] ```) carries name
  // fields but no arguments, and the old name-only pattern deleted the
  // whole block from the reply (mirror of the backend cleanup fix).
  cleaned = cleaned.replace(/```(?:json)?\s*\n?\s*(\[\s*\{[\s\S]*?"name"\s*:[\s\S]*?"arguments"\s*:[\s\S]*?\}\s*\])\s*\n?\s*```/g, '')

  // Remove bare JSON arrays that look like tool calls: [{"name":"...", ...}]
  cleaned = cleaned.replace(/\[\s*\{\s*"name"\s*:\s*"[^"]*"\s*,\s*"arguments"\s*:\s*\{[^}]*\}\s*\}\s*\]/g, '')

  // Remove markdown-wrapped single object tool calls
  cleaned = cleaned.replace(/```(?:json)?\s*\n?\s*(\{\s*"name"\s*:\s*"[^"]*"\s*,\s*"arguments"\s*:[\s\S]*?\})\s*\n?\s*```/g, '')

  // Remove bare JSON objects that look like tool calls: {"name":"...", "arguments":{...}}
  cleaned = cleaned.replace(/\{\s*"name"\s*:\s*"[^"]*"\s*,\s*"arguments"\s*:\s*\{[^}]*\}\s*\}/g, '')

  // Clean up empty lines left behind
  cleaned = cleaned.replace(/\n{3,}/g, '\n\n').trim()

  return cleaned
}

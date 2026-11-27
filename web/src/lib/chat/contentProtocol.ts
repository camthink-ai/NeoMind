/**
 * Client helpers for the v1 ordered content-nodes chat protocol (plan iter C).
 * Caps mirror the backend (`neomind_api::handlers::chat_refs`).
 */
import type { ChatContentNode, ChatReference, ClientChatMessage } from "@/types"
import { CHAT_CONTENT_VERSION_V1 } from "@/types"
import {
  MAX_LABEL_CHARS,
  MAX_REFERENCE_NODES,
  MAX_TEXT_CHARS,
  type InlineDoc,
  toLegacyUniqueReferences,
  toPlainText,
} from "@/lib/chat/inlineDoc"

export const MAX_CONTENT_NODES = 64
export { MAX_REFERENCE_NODES, MAX_TEXT_CHARS, MAX_LABEL_CHARS }

export type CapCode =
  | "content_nodes_cap"
  | "content_ref_nodes_cap"
  | "content_unique_refs_cap"
  | "content_text_cap"
  | "content_label_cap"

export class ContentProtocolError extends Error {
  constructor(
    readonly code: CapCode,
    message: string,
    readonly limit: number,
    readonly actual: number,
  ) {
    super(message)
    this.name = "ContentProtocolError"
  }
}

export function inlineDocToContentNodes(doc: InlineDoc): ChatContentNode[] {
  return doc.nodes.map((n) => {
    if (n.kind === "text") return { kind: "text" as const, text: n.text }
    return {
      kind: "reference" as const,
      type: n.entityType,
      id: n.id,
      labelSnapshot: n.labelSnapshot.slice(0, MAX_LABEL_CHARS),
    }
  })
}

const REF_TYPES = new Set(["device", "rule", "transform", "agent", "dashboard"])

/**
 * Coerce API / optimistic / draft payloads into typed ChatContentNode[].
 * Accepts camelCase wire (`labelSnapshot`) and snake_case aliases.
 */
export function normalizeContentNodes(raw: unknown): ChatContentNode[] | undefined {
  if (!Array.isArray(raw) || raw.length === 0) return undefined
  const out: ChatContentNode[] = []
  for (const item of raw) {
    if (!item || typeof item !== "object") continue
    const n = item as Record<string, unknown>
    if (n.kind === "text" && typeof n.text === "string") {
      out.push({ kind: "text", text: n.text })
      continue
    }
    if (n.kind === "reference") {
      const type = n.type
      const id = n.id
      const labelRaw =
        (typeof n.labelSnapshot === "string" && n.labelSnapshot) ||
        (typeof n.label_snapshot === "string" && n.label_snapshot) ||
        (typeof n.label === "string" && n.label) ||
        ""
      if (typeof type === "string" && REF_TYPES.has(type) && typeof id === "string" && id) {
        out.push({
          kind: "reference",
          type: type as "device" | "rule" | "transform" | "agent" | "dashboard",
          id,
          labelSnapshot: labelRaw.slice(0, MAX_LABEL_CHARS),
        })
      }
    }
  }
  return out.length > 0 ? out : undefined
}

export function contentNodesHaveReferences(nodes: ChatContentNode[] | undefined): boolean {
  return !!nodes?.some((n) => n.kind === "reference")
}

/** Restore composer fields from persisted / history contentNodes (resend UX). */
export function contentNodesToComposerFields(nodes: ChatContentNode[]): {
  message: string
  references: ChatReference[]
  contentNodes: ChatContentNode[]
} {
  const parts: string[] = []
  const seen = new Set<string>()
  const references: ChatReference[] = []
  for (const n of nodes) {
    if (n.kind === "text") {
      parts.push(n.text)
    } else {
      parts.push(`@${n.labelSnapshot || n.id}`)
      const key = `${n.type}:${n.id}`
      if (!seen.has(key)) {
        seen.add(key)
        references.push({ type: n.type, id: n.id, label: n.labelSnapshot })
      }
    }
  }
  return {
    message: parts.join(""),
    references,
    contentNodes: nodes,
  }
}

export function validateContentNodes(nodes: ChatContentNode[]): void {
  if (nodes.length > MAX_CONTENT_NODES) {
    throw new ContentProtocolError(
      "content_nodes_cap",
      `contentNodes exceed the ${MAX_CONTENT_NODES}-node cap`,
      MAX_CONTENT_NODES,
      nodes.length,
    )
  }
  let textChars = 0
  let refNodes = 0
  const unique = new Set<string>()
  for (const n of nodes) {
    if (n.kind === "text") {
      textChars += n.text.length
    } else {
      refNodes += 1
      unique.add(`${n.type}:${n.id}`)
      if (n.labelSnapshot.length > MAX_LABEL_CHARS) {
        throw new ContentProtocolError(
          "content_label_cap",
          `labelSnapshot exceeds ${MAX_LABEL_CHARS} chars`,
          MAX_LABEL_CHARS,
          n.labelSnapshot.length,
        )
      }
    }
  }
  if (textChars > MAX_TEXT_CHARS) {
    throw new ContentProtocolError(
      "content_text_cap",
      `content text exceeds the ${MAX_TEXT_CHARS}-char cap`,
      MAX_TEXT_CHARS,
      textChars,
    )
  }
  if (refNodes > MAX_REFERENCE_NODES) {
    throw new ContentProtocolError(
      "content_ref_nodes_cap",
      `reference nodes exceed the ${MAX_REFERENCE_NODES}-reference cap`,
      MAX_REFERENCE_NODES,
      refNodes,
    )
  }
  if (unique.size > MAX_REFERENCE_NODES) {
    throw new ContentProtocolError(
      "content_unique_refs_cap",
      `unique references exceed the ${MAX_REFERENCE_NODES}-reference cap`,
      MAX_REFERENCE_NODES,
      unique.size,
    )
  }
}

/**
 * Pre-send cap check for the production composers: mirror the server's
 * content-node caps client-side so an over-cap draft (e.g. 21 total inline
 * mentions of one entity — in-sentence duplicates are legal, so the
 * unique-count insertion guard does not catch them) fails HERE, toast +
 * composer kept, instead of a hard 400 after the composer was cleared.
 * Returns the failing cap code, or null when the nodes are sendable.
 */
export function validateSendContent(nodes: ChatContentNode[]): CapCode | null {
  try {
    validateContentNodes(nodes)
    return null
  } catch (e) {
    if (e instanceof ContentProtocolError) return e.code
    throw e
  }
}

/** Build a WS/HTTP chat payload from an InlineDoc (prototype / future composer). */
export function buildChatRequestFromDoc(
  doc: InlineDoc,
  extras: Omit<ClientChatMessage, "message" | "contentNodes" | "contentVersion" | "references"> = {},
): ClientChatMessage {
  const nodes = inlineDocToContentNodes(doc)
  validateContentNodes(nodes)
  const legacy: ChatReference[] = toLegacyUniqueReferences(doc).map((r) => ({
    type: r.type,
    id: r.id,
    label: r.label,
  }))
  return {
    ...extras,
    message: toPlainText(doc),
    contentVersion: CHAT_CONTENT_VERSION_V1,
    contentNodes: nodes,
    // Keep legacy chips for older servers during rollout; new servers prefer nodes.
    references: legacy.length > 0 ? legacy : undefined,
  }
}

/**
 * Legacy chip composer → contentNodes (text + trailing reference nodes).
 * Production ChatComposer now prefers TipTap in-sentence nodes; keep this
 * helper for drafts / older send paths that only have chips + plain text.
 */
export function chipsAndTextToContentNodes(
  message: string,
  references: ChatReference[],
): ChatContentNode[] {
  const nodes: ChatContentNode[] = []
  if (message) {
    nodes.push({ kind: "text", text: message })
  }
  for (const r of references) {
    nodes.push({
      kind: "reference",
      type: r.type,
      id: r.id,
      labelSnapshot: (r.label ?? "").slice(0, MAX_LABEL_CHARS),
    })
  }
  return nodes
}

/** Build WS/HTTP payload from production chip + plain-text composer. */
export function buildChatRequestFromChips(
  message: string,
  references: ChatReference[],
  extras: Omit<ClientChatMessage, "message" | "contentNodes" | "contentVersion" | "references"> = {},
): ClientChatMessage {
  const nodes = chipsAndTextToContentNodes(message, references)
  if (nodes.length > 0) {
    validateContentNodes(nodes)
  }
  return {
    ...extras,
    message,
    contentVersion: nodes.length > 0 ? CHAT_CONTENT_VERSION_V1 : undefined,
    contentNodes: nodes.length > 0 ? nodes : undefined,
    references: references.length > 0 ? references : undefined,
  }
}

// ---- Per-session draft (refresh / switch / failed send recovery) ----

export interface SessionChatDraft {
  sessionId: string
  message: string
  references: ChatReference[]
  contentNodes?: ChatContentNode[]
  updatedAt: number
}

const DRAFT_PREFIX = "neomind:chat-draft:v1:"

export function draftKey(sessionId: string): string {
  return `${DRAFT_PREFIX}${sessionId || "new"}`
}

export function loadSessionDraft(sessionId: string): SessionChatDraft | null {
  if (typeof localStorage === "undefined") return null
  try {
    const raw = localStorage.getItem(draftKey(sessionId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as SessionChatDraft
    if (!parsed || parsed.sessionId !== (sessionId || "new")) return null
    return parsed
  } catch {
    return null
  }
}

export function saveSessionDraft(draft: SessionChatDraft): void {
  if (typeof localStorage === "undefined") return
  const payload: SessionChatDraft = {
    ...draft,
    sessionId: draft.sessionId || "new",
    updatedAt: Date.now(),
  }
  localStorage.setItem(draftKey(payload.sessionId), JSON.stringify(payload))
}

export function clearSessionDraft(sessionId: string): void {
  if (typeof localStorage === "undefined") return
  localStorage.removeItem(draftKey(sessionId))
}

/**
 * Bound draft growth: keep only the `keep` most recently updated draft keys.
 * Nothing deletes drafts for removed sessions, so without a cap the
 * `neomind:chat-draft:v1:*` namespace grows forever (each key ≤ ~8KB, but
 * unbounded in count). Best-effort: unreadable entries count as oldest.
 */
export function pruneSessionDrafts(keep = 30): void {
  if (typeof localStorage === "undefined") return
  const entries: { key: string; updatedAt: number }[] = []
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i)
    if (!key || !key.startsWith(DRAFT_PREFIX)) continue
    let updatedAt = 0
    try {
      const raw = localStorage.getItem(key)
      const parsed = raw ? (JSON.parse(raw) as SessionChatDraft) : null
      updatedAt = parsed?.updatedAt ?? 0
    } catch {
      updatedAt = 0
    }
    entries.push({ key, updatedAt })
  }
  if (entries.length <= keep) return
  entries.sort((a, b) => a.updatedAt - b.updatedAt)
  for (const e of entries.slice(0, entries.length - keep)) {
    localStorage.removeItem(e.key)
  }
}

/** Restore draft after a send failure (cap / network). Does not clear. */
export function markDraftForRetry(sessionId: string, patch: Partial<SessionChatDraft>): SessionChatDraft {
  const existing = loadSessionDraft(sessionId) ?? {
    sessionId: sessionId || "new",
    message: "",
    references: [],
    updatedAt: Date.now(),
  }
  const next = { ...existing, ...patch, sessionId: sessionId || "new", updatedAt: Date.now() }
  saveSessionDraft(next)
  return next
}

/** How long the composer waits for a server `MessageReceived` before restoring. */
export const DRAFT_ACK_TIMEOUT_MS = 20_000

export type PendingDraftStash = {
  message: string
  references: ChatReference[]
  contentNodes?: ChatContentNode[]
  images?: import("@/types").ChatImage[]
}

type PendingDraftEntry = {
  sessionId: string
  stash: PendingDraftStash
}

/** In-memory pending drafts keyed by clientRequestId (server receive-ack). */
const pendingDraftByRequestId = new Map<string, PendingDraftEntry>()

/**
 * Keep a draft until the server receive-ack for `requestId`.
 * Persists via markDraftForRetry so refresh mid-flight can recover.
 */
export function stashPendingDraft(
  requestId: string,
  sessionId: string,
  stash: PendingDraftStash,
): void {
  pendingDraftByRequestId.set(requestId, { sessionId, stash })
  markDraftForRetry(sessionId, {
    message: stash.message,
    references: stash.references,
    contentNodes: stash.contentNodes,
  })
}

/**
 * Resolve a pending draft after server ack / reject / timeout / queue drop.
 * `accepted` clears storage under the **original** session id.
 * Failure re-persists under that same session id and returns the stash —
 * callers must only hydrate the editor when {@link canHydrateDraftIntoEditor}.
 */
export function settlePendingDraft(
  requestId: string,
  outcome: "accepted" | "failed",
): PendingDraftStash & { sessionId: string } | null {
  const entry = pendingDraftByRequestId.get(requestId)
  pendingDraftByRequestId.delete(requestId)
  if (!entry) return null
  if (outcome === "accepted") {
    clearSessionDraft(entry.sessionId)
    return null
  }
  // Always target the original session's persisted draft — never the
  // session the user may have switched to while waiting for the ack.
  markDraftForRetry(entry.sessionId, {
    message: entry.stash.message,
    references: entry.stash.references,
    contentNodes: entry.stash.contentNodes,
  })
  return { sessionId: entry.sessionId, ...entry.stash }
}

/**
 * Whether a failed-ack draft may be written into the live composer.
 * Requires: still viewing the draft's original session, and the editor
 * has not been modified since the optimistic clear (still empty).
 */
export function canHydrateDraftIntoEditor(
  draftSessionId: string,
  currentSessionId: string | null | undefined,
  editor: {
    message: string
    references: ChatReference[]
    contentNodes?: ChatContentNode[]
    images?: import("@/types").ChatImage[]
  },
): boolean {
  const current = currentSessionId || "new"
  if (draftSessionId !== current) return false
  if (editor.message.trim()) return false
  if (editor.references.length > 0) return false
  if (editor.contentNodes && editor.contentNodes.length > 0) return false
  if (editor.images && editor.images.length > 0) return false
  return true
}

/**
 * After an await (reference preflight), refuse optimistic composer / message
 * writes when the live route session no longer matches the send-time key.
 */
export function sessionUnchangedAfterAwait(
  sendSessionId: string,
  currentSessionId: string | null | undefined,
): boolean {
  return (currentSessionId || "new") === sendSessionId
}

/**
 * Quick-action / canned-prompt text replace: always clear stale mention
 * chips and ordered content nodes so prior @-refs do not ride the new text.
 */
export function composerReplaceTextOnly(prompt: string): {
  message: string
  references: ChatReference[]
  contentNodes: ChatContentNode[]
} {
  return { message: prompt, references: [], contentNodes: [] }
}

/** Test / diagnostics: how many in-flight draft acks are waiting. */
export function pendingDraftCount(): number {
  return pendingDraftByRequestId.size
}

/** Test helper — drop all in-memory pending draft waiters. */
export function clearPendingDraftsForTests(): void {
  pendingDraftByRequestId.clear()
}

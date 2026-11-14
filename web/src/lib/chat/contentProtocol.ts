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
} from "@/prototypes/inline-at/model"

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

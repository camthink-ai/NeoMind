/**
 * Ordered text/reference document for chat inline @ (content-nodes protocol).
 *
 * Shared by production ChatComposer / contentProtocol and the /dev/inline-at
 * prototype. Caps are explicit (never silent drop); same entity may repeat.
 */

export type EntityType = "device" | "rule" | "transform" | "agent"

export interface TextNode {
  kind: "text"
  text: string
}

export interface ReferenceNode {
  kind: "reference"
  entityType: EntityType
  id: string
  labelSnapshot: string
}

export type InlineNode = TextNode | ReferenceNode

export interface InlineDoc {
  nodes: InlineNode[]
}

/** Hard caps shared with the existing MAX_REFERENCES chip composer. */
export const MAX_REFERENCE_NODES = 20
export const MAX_UNIQUE_REFERENCES = 20
export const MAX_LABEL_CHARS = 128
export const MAX_TEXT_CHARS = 8_000

export class CapError extends Error {
  constructor(
    readonly code: "ref_nodes" | "unique_refs" | "text_chars" | "label_chars",
    message: string,
  ) {
    super(message)
    this.name = "CapError"
  }
}

export function emptyDoc(): InlineDoc {
  return { nodes: [{ kind: "text", text: "" }] }
}

export function isReference(n: InlineNode): n is ReferenceNode {
  return n.kind === "reference"
}

export function normalizeDoc(doc: InlineDoc): InlineDoc {
  const nodes: InlineNode[] = []
  for (const n of doc.nodes) {
    if (n.kind === "text") {
      if (nodes.length > 0 && nodes[nodes.length - 1].kind === "text") {
        ;(nodes[nodes.length - 1] as TextNode).text += n.text
      } else {
        nodes.push({ kind: "text", text: n.text })
      }
    } else {
      nodes.push({ ...n, labelSnapshot: n.labelSnapshot.slice(0, MAX_LABEL_CHARS) })
    }
  }
  if (nodes.length === 0) nodes.push({ kind: "text", text: "" })
  if (nodes[0].kind !== "text") nodes.unshift({ kind: "text", text: "" })
  if (nodes[nodes.length - 1].kind !== "text") nodes.push({ kind: "text", text: "" })
  return { nodes }
}

export function totalTextChars(doc: InlineDoc): number {
  return doc.nodes.reduce((n, node) => n + (node.kind === "text" ? node.text.length : 0), 0)
}

export function referenceNodes(doc: InlineDoc): ReferenceNode[] {
  return doc.nodes.filter(isReference)
}

export function uniqueReferenceKeys(doc: InlineDoc): Set<string> {
  return new Set(referenceNodes(doc).map((r) => `${r.entityType}:${r.id}`))
}

function assertCaps(doc: InlineDoc): void {
  const refs = referenceNodes(doc)
  if (refs.length > MAX_REFERENCE_NODES) {
    throw new CapError("ref_nodes", `reference nodes exceed ${MAX_REFERENCE_NODES}`)
  }
  if (uniqueReferenceKeys(doc).size > MAX_UNIQUE_REFERENCES) {
    throw new CapError("unique_refs", `unique references exceed ${MAX_UNIQUE_REFERENCES}`)
  }
  if (totalTextChars(doc) > MAX_TEXT_CHARS) {
    throw new CapError("text_chars", `text exceeds ${MAX_TEXT_CHARS} chars`)
  }
  for (const r of refs) {
    if (r.labelSnapshot.length > MAX_LABEL_CHARS) {
      throw new CapError("label_chars", `label exceeds ${MAX_LABEL_CHARS} chars`)
    }
  }
}

export interface Caret {
  /** Index of a text node in doc.nodes */
  nodeIndex: number
  offset: number
}

export function clampCaret(doc: InlineDoc, caret: Caret): Caret {
  const idx = Math.max(0, Math.min(caret.nodeIndex, doc.nodes.length - 1))
  const node = doc.nodes[idx]
  if (node.kind !== "text") {
    // Snap to nearest text node before/after.
    for (let i = idx - 1; i >= 0; i--) {
      if (doc.nodes[i].kind === "text") {
        const t = doc.nodes[i] as TextNode
        return { nodeIndex: i, offset: t.text.length }
      }
    }
    for (let i = idx + 1; i < doc.nodes.length; i++) {
      if (doc.nodes[i].kind === "text") {
        return { nodeIndex: i, offset: 0 }
      }
    }
    return { nodeIndex: 0, offset: 0 }
  }
  return { nodeIndex: idx, offset: Math.max(0, Math.min(caret.offset, node.text.length)) }
}

/** Insert a reference at the caret; splits the text node. Same entity may repeat. */
export function insertReference(
  doc: InlineDoc,
  caret: Caret,
  ref: Omit<ReferenceNode, "kind">,
): { doc: InlineDoc; caret: Caret } {
  const c = clampCaret(doc, caret)
  const node = doc.nodes[c.nodeIndex] as TextNode
  const left = node.text.slice(0, c.offset)
  const right = node.text.slice(c.offset)
  const labelSnapshot = ref.labelSnapshot.slice(0, MAX_LABEL_CHARS)
  const nextNodes: InlineNode[] = [
    ...doc.nodes.slice(0, c.nodeIndex),
    { kind: "text", text: left },
    { kind: "reference", entityType: ref.entityType, id: ref.id, labelSnapshot },
    { kind: "text", text: right },
    ...doc.nodes.slice(c.nodeIndex + 1),
  ]
  const next = normalizeDoc({ nodes: nextNodes })
  assertCaps(next)
  // Caret lands in the text node after the inserted reference.
  const newRefIndex = c.nodeIndex + 1
  const afterTextIndex = newRefIndex + 1
  return { doc: next, caret: { nodeIndex: afterTextIndex, offset: 0 } }
}

/** Replace text inside one text node (typing). */
export function setTextNode(
  doc: InlineDoc,
  nodeIndex: number,
  text: string,
  caretOffset: number,
): { doc: InlineDoc; caret: Caret } {
  if (doc.nodes[nodeIndex]?.kind !== "text") {
    throw new Error("setTextNode requires a text node")
  }
  const nodes = doc.nodes.slice()
  nodes[nodeIndex] = { kind: "text", text }
  const next = normalizeDoc({ nodes })
  assertCaps(next)
  // After normalize, node indices may shift if empty merges — re-find by walking.
  // For the common case (edit in place without adjacent empty merge of refs),
  // clamp to the edited region.
  return { doc: next, caret: clampCaret(next, { nodeIndex, offset: caretOffset }) }
}

/** Delete the reference node at index; merge surrounding text. */
export function deleteReferenceAt(doc: InlineDoc, refIndex: number): { doc: InlineDoc; caret: Caret } {
  if (doc.nodes[refIndex]?.kind !== "reference") {
    throw new Error("deleteReferenceAt requires a reference node")
  }
  const before = doc.nodes[refIndex - 1]
  const after = doc.nodes[refIndex + 1]
  const leftText = before?.kind === "text" ? before.text : ""
  const rightText = after?.kind === "text" ? after.text : ""
  const start = before?.kind === "text" ? refIndex - 1 : refIndex
  const end = after?.kind === "text" ? refIndex + 1 : refIndex
  const merged: TextNode = { kind: "text", text: leftText + rightText }
  const nodes = [
    ...doc.nodes.slice(0, start),
    merged,
    ...doc.nodes.slice(end + 1),
  ]
  const next = normalizeDoc({ nodes })
  return { doc: next, caret: { nodeIndex: start, offset: leftText.length } }
}

/**
 * Backspace at caret: if offset==0 and previous node is a reference, delete it
 * atomically; otherwise delete one character.
 */
export function backspace(doc: InlineDoc, caret: Caret): { doc: InlineDoc; caret: Caret } | null {
  const c = clampCaret(doc, caret)
  if (c.offset > 0) {
    const node = doc.nodes[c.nodeIndex] as TextNode
    const text = node.text.slice(0, c.offset - 1) + node.text.slice(c.offset)
    return setTextNode(doc, c.nodeIndex, text, c.offset - 1)
  }
  // Look left for a reference.
  for (let i = c.nodeIndex - 1; i >= 0; i--) {
    if (doc.nodes[i].kind === "reference") {
      return deleteReferenceAt(doc, i)
    }
    if (doc.nodes[i].kind === "text" && (doc.nodes[i] as TextNode).text.length > 0) {
      break
    }
  }
  return null
}

/** Readable plaintext for copy / fallback send (`@Label` markers). */
export function toPlainText(doc: InlineDoc): string {
  return doc.nodes
    .map((n) => (n.kind === "text" ? n.text : `@${n.labelSnapshot}`))
    .join("")
}

/** Ordered nodes for the future HTTP/WS payload (iter C). */
export function toContentNodes(doc: InlineDoc): InlineNode[] {
  return normalizeDoc(doc).nodes
}

/**
 * Compatibility projection for today's attachment-chip API: unique refs only.
 * Prototype keeps in-sentence duplicates; this is the legacy bridge.
 */
export function toLegacyUniqueReferences(
  doc: InlineDoc,
): Array<{ type: EntityType; id: string; label: string }> {
  const seen = new Set<string>()
  const out: Array<{ type: EntityType; id: string; label: string }> = []
  for (const r of referenceNodes(doc)) {
    const key = `${r.entityType}:${r.id}`
    if (seen.has(key)) continue
    seen.add(key)
    out.push({ type: r.entityType, id: r.id, label: r.labelSnapshot })
  }
  return out
}

/** Detect an @-trigger at the end of the active text segment (IME-safe caller). */
export function detectAtTrigger(textBeforeCaret: string): { query: string; start: number } | null {
  // Same contract as ChatComposer: not email-like (word char before @).
  const m = /(^|[^@\w])@([^@\n]{0,40})$/.exec(textBeforeCaret)
  if (!m) return null
  return { query: m[2], start: textBeforeCaret.length - m[2].length - 1 }
}

/** Strip the `@query` trigger from a text node when a mention is chosen. */
export function consumeAtTrigger(
  doc: InlineDoc,
  caret: Caret,
  triggerStart: number,
): { doc: InlineDoc; caret: Caret } {
  const c = clampCaret(doc, caret)
  const node = doc.nodes[c.nodeIndex]
  if (node.kind !== "text") return { doc, caret: c }
  const text = node.text.slice(0, triggerStart) + node.text.slice(c.offset)
  return setTextNode(doc, c.nodeIndex, text, triggerStart)
}

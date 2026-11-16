/**
 * TipTap document ↔ InlineDoc / contentNodes bridge.
 */
import type { JSONContent } from "@tiptap/core"
import type { ChatContentNode, ChatReference } from "@/types"
import {
  emptyDoc,
  normalizeDoc,
  toLegacyUniqueReferences,
  toPlainText,
  type EntityType,
  type InlineDoc,
  type InlineNode,
} from "@/lib/chat/inlineDoc"
import { inlineDocToContentNodes } from "@/lib/chat/contentProtocol"

const ENTITY_TYPES: EntityType[] = ["device", "rule", "transform", "agent"]

/** Valid entity-type guard shared with the paste parser (entityMention). */
export function isEntityType(v: unknown): v is EntityType {
  return typeof v === "string" && (ENTITY_TYPES as string[]).includes(v)
}

/** Walk TipTap JSON into ordered InlineDoc nodes (paragraphs joined with \\n). */
export function tipTapJsonToInlineDoc(json: JSONContent | null | undefined): InlineDoc {
  if (!json) return emptyDoc()
  const nodes: InlineNode[] = []

  const pushText = (text: string) => {
    if (!text) return
    const last = nodes[nodes.length - 1]
    if (last?.kind === "text") last.text += text
    else nodes.push({ kind: "text", text })
  }

  const walkInline = (content: JSONContent[] | undefined) => {
    if (!content) return
    for (const n of content) {
      if (n.type === "text") {
        pushText(n.text ?? "")
      } else if (n.type === "hardBreak") {
        pushText("\n")
      } else if (n.type === "mention") {
        const id = typeof n.attrs?.id === "string" ? n.attrs.id : ""
        // An invalid/missing entityType must NOT silently coerce to a device
        // reference (pasted HTML with a bogus/absent data-entity-type) —
        // drop the mention node instead (E18).
        const entityType = n.attrs?.entityType
        if (id && isEntityType(entityType)) {
          const label =
            typeof n.attrs?.label === "string"
              ? n.attrs.label
              : id
          nodes.push({
            kind: "reference",
            entityType,
            id,
            labelSnapshot: label,
          })
        }
      } else if (n.content) {
        walkInline(n.content)
      }
    }
  }

  const blocks = json.content ?? []
  blocks.forEach((block, i) => {
    if (i > 0) pushText("\n")
    walkInline(block.content)
  })

  if (nodes.length === 0) return emptyDoc()
  return normalizeDoc({ nodes })
}

/** Build a TipTap doc JSON from an InlineDoc (single paragraph + hard breaks). */
export function inlineDocToTipTapJson(doc: InlineDoc): JSONContent {
  const normalized = normalizeDoc(doc)
  const inline: JSONContent[] = []
  for (const n of normalized.nodes) {
    if (n.kind === "text") {
      if (!n.text) continue
      const parts = n.text.split("\n")
      parts.forEach((part, i) => {
        if (i > 0) inline.push({ type: "hardBreak" })
        if (part) inline.push({ type: "text", text: part })
      })
    } else {
      inline.push({
        type: "mention",
        attrs: {
          id: n.id,
          label: n.labelSnapshot,
          entityType: n.entityType,
          mentionSuggestionChar: "@",
        },
      })
    }
  }
  return {
    type: "doc",
    content: [
      {
        type: "paragraph",
        content: inline.length > 0 ? inline : undefined,
      },
    ],
  }
}

export function inlineDocToChatPayload(doc: InlineDoc): {
  message: string
  references: ChatReference[]
  contentNodes: ChatContentNode[]
} {
  const nodes = inlineDocToContentNodes(doc)
  const hasRefs = nodes.some((n) => n.kind === "reference")
  return {
    message: toPlainText(doc),
    references: toLegacyUniqueReferences(doc).map((r) => ({
      type: r.type,
      id: r.id,
      label: r.label,
    })),
    contentNodes: hasRefs || nodes.some((n) => n.kind === "text" && n.text.trim())
      ? nodes
      : [],
  }
}

export function contentNodesToInlineDoc(nodes: ChatContentNode[] | undefined): InlineDoc {
  if (!nodes || nodes.length === 0) return emptyDoc()
  const out: InlineNode[] = []
  for (const n of nodes) {
    if (n.kind === "text") {
      out.push({ kind: "text", text: n.text })
    } else {
      out.push({
        kind: "reference",
        entityType: n.type,
        id: n.id,
        labelSnapshot: n.labelSnapshot,
      })
    }
  }
  return normalizeDoc({ nodes: out })
}

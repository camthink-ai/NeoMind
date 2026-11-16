/**
 * TipTap Mention node with NeoMind entity attrs (type + id + label).
 * Labels are display-only; type+id are authoritative.
 */
import Mention from "@tiptap/extension-mention"
import type { MentionOptions } from "@tiptap/extension-mention"
import { mergeAttributes } from "@tiptap/core"
import type { EntityType } from "@/prototypes/inline-at/model"
import { isEntityType } from "@/components/chat/inline-mention/docBridge"

export type EntityMentionAttrs = {
  id: string | null
  label: string | null
  entityType: EntityType | null
  /** Localized type word for display (falls back to entityType). */
  typeLabel: string | null
  mentionSuggestionChar?: string
}

export type EntityMentionItem = {
  id: string
  label: string
  entityType: EntityType
  typeLabel?: string
}

export const EntityMention = Mention.extend<
  MentionOptions<EntityMentionItem, EntityMentionAttrs>
>({
  name: "mention",

  addAttributes() {
    return {
      ...this.parent?.(),
      entityType: {
        default: null,
        parseHTML: (element) =>
          (element.getAttribute("data-entity-type") as EntityType | null) ?? null,
        renderHTML: (attributes) => {
          if (!attributes.entityType) return {}
          return { "data-entity-type": attributes.entityType }
        },
      },
      typeLabel: {
        default: null,
        parseHTML: (element) => element.getAttribute("data-type-label"),
        renderHTML: (attributes) => {
          if (!attributes.typeLabel) return {}
          return { "data-type-label": attributes.typeLabel }
        },
      },
    }
  },

  parseHTML() {
    return [
      {
        tag: 'span[data-type="mention"]',
        getAttrs: (el) => {
          // Reject pasted/foreign mentions with an unknown entity type:
          // converting them would either coerce a wrong type (the old
          // everything-is-a-device bug) or render a chip that is silently
          // dropped at send (visible-vs-sent divergence). Returning false
          // lets the span fall through as plain text instead.
          const t = (el as HTMLElement).getAttribute('data-entity-type') ?? ''
          return isEntityType(t) ? null : false
        },
      },
    ]
  },

  renderHTML({ node, HTMLAttributes }) {
    const label = node.attrs.label ?? node.attrs.id ?? ""
    const entityType = node.attrs.entityType ?? ""
    const typeLabel = (node.attrs.typeLabel as string | null) || entityType
    const typeClass = entityType ? ` nm-mention--${entityType}` : ""
    return [
      "span",
      mergeAttributes(HTMLAttributes, {
        "data-type": "mention",
        "data-id": node.attrs.id,
        "data-label": label,
        "data-entity-type": entityType,
        // Dense multi-mention: `@name` only; type lives in title + color.
        title: typeLabel ? `${typeLabel} · ${node.attrs.id ?? ""}` : undefined,
        class: `nm-mention${typeClass}`,
        contenteditable: "false",
      }),
      ["span", { class: "nm-mention-at", "aria-hidden": "true" }, "@"],
      ["span", { class: "nm-mention-label" }, label],
    ]
  },
})

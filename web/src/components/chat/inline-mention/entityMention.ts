/**
 * TipTap Mention node with NeoMind entity attrs (type + id + label).
 * Labels are display-only; type+id are authoritative.
 */
import Mention from "@tiptap/extension-mention"
import type { MentionOptions } from "@tiptap/extension-mention"
import { mergeAttributes } from "@tiptap/core"
import type { EntityType } from "@/prototypes/inline-at/model"

export type EntityMentionAttrs = {
  id: string | null
  label: string | null
  entityType: EntityType | null
  mentionSuggestionChar?: string
}

export type EntityMentionItem = {
  id: string
  label: string
  entityType: EntityType
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
    }
  },

  parseHTML() {
    return [{ tag: 'span[data-type="mention"]' }]
  },

  renderHTML({ node, HTMLAttributes }) {
    const label = node.attrs.label ?? node.attrs.id ?? ""
    const entityType = node.attrs.entityType ?? ""
    return [
      "span",
      mergeAttributes(HTMLAttributes, {
        "data-type": "mention",
        "data-id": node.attrs.id,
        "data-label": label,
        "data-entity-type": entityType,
        class: "nm-mention",
        contenteditable: "false",
      }),
      // Type badge + label (not "@label") — matches history chips and keeps
      // readable contrast via host CSS (never inherits bubble text color).
      entityType
        ? ["span", { class: `nm-mention-type nm-mention-type--${entityType}` }, entityType]
        : "",
      label,
    ]
  },
})

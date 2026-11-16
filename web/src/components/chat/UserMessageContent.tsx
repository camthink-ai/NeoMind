/**
 * User bubble body: when contentNodes include references, re-draw the same
 * in-sentence chips as the TipTap composer (history / optimistic echo).
 * Plain text (no refs) keeps MarkdownMessage so markdown still works.
 */

import { useTranslation } from "react-i18next"
import type { ChatContentNode } from "@/types"
import { contentNodesHaveReferences } from "@/lib/chat/contentProtocol"
import { MarkdownMessage } from "./MarkdownMessage"
import {
  mentionAtClass,
  mentionChipClass,
  mentionLabelClass,
} from "./mentionChipStyles"

interface UserMessageContentProps {
  content: string
  contentNodes?: ChatContentNode[]
}

export function UserMessageContent({ content, contentNodes }: UserMessageContentProps) {
  const { t } = useTranslation("chat")

  if (!contentNodesHaveReferences(contentNodes)) {
    if (!content) return null
    return <MarkdownMessage content={content} variant="user" />
  }

  return (
    <div
      className="whitespace-pre-wrap break-words text-sm leading-relaxed"
      data-user-content-nodes
    >
      {contentNodes!.map((n, i) =>
        n.kind === "text" ? (
          <span key={i}>{n.text}</span>
        ) : (
          <span
            key={i}
            className={mentionChipClass(n.type, "mx-0.5")}
            data-type={n.type}
            data-id={n.id}
            title={`${t(`mention.${n.type}`, n.type)} · ${n.id}`}
          >
            <span className={mentionAtClass()} aria-hidden="true">
              @
            </span>
            <span className={mentionLabelClass()}>
              {n.labelSnapshot || n.id}
            </span>
          </span>
        ),
      )}
    </div>
  )
}

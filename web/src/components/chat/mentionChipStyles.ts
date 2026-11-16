/**
 * Shared @-mention chip styles for TipTap composer + history bubbles.
 *
 * Dense multi-mention design:
 *   `@温室A` `@巡检代理`  — name only, whole token tinted by entity type.
 * Type text / middle-dots were dropped: repeating “设备 · …” in one sentence
 * reads as noise. Full type+id stays on the title tooltip.
 */
import { cn } from "@/lib/utils"
import type { MentionType } from "@/lib/api/mentions"

const TYPE_TEXT: Record<MentionType, string> = {
  device: "text-accent-cyan",
  rule: "text-accent-orange",
  transform: "text-accent-purple",
  agent: "text-accent-emerald",
}

/** Compact `@name` token — inherits body size, tinted by entity type. */
export function mentionChipClass(
  type: MentionType | string,
  extra?: string,
): string {
  const color = TYPE_TEXT[type as MentionType] ?? "text-primary"
  return cn(
    "nm-mention inline-flex items-baseline gap-0 align-baseline font-medium",
    color,
    extra,
  )
}

export function mentionAtClass(): string {
  return "nm-mention-at shrink-0"
}

export function mentionLabelClass(): string {
  return "nm-mention-label"
}

/** TipTap host descendant selectors (ProseMirror renders the node). */
export const mentionChipHostSelectors = cn(
  "[&_.nm-mention]:inline-flex [&_.nm-mention]:items-baseline [&_.nm-mention]:gap-0",
  "[&_.nm-mention]:align-baseline [&_.nm-mention]:font-medium [&_.nm-mention]:bg-transparent",
  "[&_.nm-mention--device]:text-accent-cyan",
  "[&_.nm-mention--rule]:text-accent-orange",
  "[&_.nm-mention--transform]:text-accent-purple",
  "[&_.nm-mention--agent]:text-accent-emerald",
  "[&_.nm-mention[data-invalid=true]]:text-error",
  "[&_.nm-mention[data-invalid=true]]:underline",
)

/**
 * Shared @-mention chip styles for TipTap composer + history bubbles.
 *
 * No fill / no nested badge — type is color-only; the name inherits the
 * surrounding text color (composer foreground or user-bubble text). That
 * avoids the old “muted chip + inherited bubble text” contrast bug without
 * painting a second background that reads as a tag.
 */
import { cn } from "@/lib/utils"
import type { MentionType } from "@/lib/api/mentions"

const TYPE_TEXT: Record<MentionType, string> = {
  device: "text-accent-cyan",
  rule: "text-accent-orange",
  transform: "text-accent-purple",
  agent: "text-accent-emerald",
}

/** Inline mention shell — transparent, same metrics as surrounding text. */
export function mentionChipClass(extra?: string): string {
  return cn(
    "nm-mention inline-flex items-center gap-1 rounded-sm",
    "px-0.5 text-xs font-medium leading-none align-middle",
    extra,
  )
}

/** Type label: same size as the name, accent color only. */
export function mentionTypeBadgeClass(type: MentionType | string): string {
  const color = TYPE_TEXT[type as MentionType] ?? "text-muted-foreground"
  return cn("shrink-0 leading-none", color)
}

/** TipTap host descendant selectors (ProseMirror renders the node). */
export const mentionChipHostSelectors = cn(
  "[&_.nm-mention]:inline-flex [&_.nm-mention]:items-center [&_.nm-mention]:gap-1",
  "[&_.nm-mention]:rounded-sm [&_.nm-mention]:px-0.5",
  "[&_.nm-mention]:text-xs [&_.nm-mention]:font-medium [&_.nm-mention]:leading-none",
  "[&_.nm-mention]:align-middle [&_.nm-mention]:bg-transparent",
  "[&_.nm-mention-type]:shrink-0 [&_.nm-mention-type]:leading-none",
  "[&_.nm-mention-type--device]:text-accent-cyan",
  "[&_.nm-mention-type--rule]:text-accent-orange",
  "[&_.nm-mention-type--transform]:text-accent-purple",
  "[&_.nm-mention-type--agent]:text-accent-emerald",
  "[&_.nm-mention[data-invalid=true]]:text-error",
  "[&_.nm-mention[data-invalid=true]]:underline",
)

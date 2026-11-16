/**
 * Shared @-mention chip styles for TipTap composer + history bubbles.
 *
 * Chips must set their own fg/bg — never inherit from the user bubble.
 * Light bubbles use white text on dark bg; dark bubbles invert. A muted chip
 * that inherits either side becomes unreadable in both themes.
 */
import { cn } from "@/lib/utils"
import type { MentionType } from "@/lib/api/mentions"

const TYPE_BADGE: Record<MentionType, string> = {
  device: "bg-accent-cyan-light text-accent-cyan",
  rule: "bg-accent-orange-light text-accent-orange",
  transform: "bg-accent-purple-light text-accent-purple",
  agent: "bg-accent-emerald-light text-accent-emerald",
}

/** Outer chip shell — solid card surface + explicit foreground. */
export function mentionChipClass(extra?: string): string {
  return cn(
    "nm-mention inline-flex items-center gap-1 rounded-full border border-border",
    "bg-card px-1.5 py-px text-xs font-medium text-foreground align-baseline",
    extra,
  )
}

export function mentionTypeBadgeClass(type: MentionType | string): string {
  const badge = TYPE_BADGE[type as MentionType] ?? "bg-muted text-muted-foreground"
  return cn("shrink-0 rounded px-1 py-px text-nano font-medium", badge)
}

/** TipTap host descendant selectors (ProseMirror renders the node). */
export const mentionChipHostSelectors = cn(
  "[&_.nm-mention]:inline-flex [&_.nm-mention]:items-center [&_.nm-mention]:gap-1",
  "[&_.nm-mention]:rounded-full [&_.nm-mention]:border [&_.nm-mention]:border-border",
  "[&_.nm-mention]:bg-card [&_.nm-mention]:px-1.5 [&_.nm-mention]:py-px",
  "[&_.nm-mention]:text-xs [&_.nm-mention]:font-medium [&_.nm-mention]:text-foreground",
  "[&_.nm-mention]:align-baseline",
  "[&_.nm-mention-type]:rounded [&_.nm-mention-type]:px-1 [&_.nm-mention-type]:py-px",
  "[&_.nm-mention-type]:text-nano [&_.nm-mention-type]:font-medium",
  "[&_.nm-mention-type--device]:bg-accent-cyan-light [&_.nm-mention-type--device]:text-accent-cyan",
  "[&_.nm-mention-type--rule]:bg-accent-orange-light [&_.nm-mention-type--rule]:text-accent-orange",
  "[&_.nm-mention-type--transform]:bg-accent-purple-light [&_.nm-mention-type--transform]:text-accent-purple",
  "[&_.nm-mention-type--agent]:bg-accent-emerald-light [&_.nm-mention-type--agent]:text-accent-emerald",
  "[&_.nm-mention[data-invalid=true]]:border-error",
  "[&_.nm-mention[data-invalid=true]]:bg-error-light",
  "[&_.nm-mention[data-invalid=true]]:text-error",
)

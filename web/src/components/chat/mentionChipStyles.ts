/**
 * Shared @-mention chip styles for TipTap composer + history bubbles.
 *
 * Chips must set their own fg/bg — never inherit from the user bubble.
 * Type is color-only (no nested badge fill): a second background + smaller
 * type size made the label look vertically offset in the line.
 */
import { cn } from "@/lib/utils"
import type { MentionType } from "@/lib/api/mentions"

const TYPE_TEXT: Record<MentionType, string> = {
  device: "text-accent-cyan",
  rule: "text-accent-orange",
  transform: "text-accent-purple",
  agent: "text-accent-emerald",
}

/** Outer chip shell — solid card surface + explicit foreground. */
export function mentionChipClass(extra?: string): string {
  return cn(
    "nm-mention inline-flex items-center gap-1 rounded-full border border-border",
    "bg-card px-1.5 py-0.5 text-xs font-medium leading-none text-foreground align-middle",
    extra,
  )
}

/** Type label: same size as the name, color only (no fill). */
export function mentionTypeBadgeClass(type: MentionType | string): string {
  const color = TYPE_TEXT[type as MentionType] ?? "text-muted-foreground"
  return cn("shrink-0 leading-none", color)
}

/** TipTap host descendant selectors (ProseMirror renders the node). */
export const mentionChipHostSelectors = cn(
  "[&_.nm-mention]:inline-flex [&_.nm-mention]:items-center [&_.nm-mention]:gap-1",
  "[&_.nm-mention]:rounded-full [&_.nm-mention]:border [&_.nm-mention]:border-border",
  "[&_.nm-mention]:bg-card [&_.nm-mention]:px-1.5 [&_.nm-mention]:py-0.5",
  "[&_.nm-mention]:text-xs [&_.nm-mention]:font-medium [&_.nm-mention]:leading-none",
  "[&_.nm-mention]:text-foreground [&_.nm-mention]:align-middle",
  "[&_.nm-mention-type]:shrink-0 [&_.nm-mention-type]:leading-none",
  "[&_.nm-mention-type--device]:text-accent-cyan",
  "[&_.nm-mention-type--rule]:text-accent-orange",
  "[&_.nm-mention-type--transform]:text-accent-purple",
  "[&_.nm-mention-type--agent]:text-accent-emerald",
  "[&_.nm-mention[data-invalid=true]]:border-error",
  "[&_.nm-mention[data-invalid=true]]:bg-error-light",
  "[&_.nm-mention[data-invalid=true]]:text-error",
)

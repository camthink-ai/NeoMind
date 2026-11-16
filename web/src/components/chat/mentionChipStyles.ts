/**
 * Shared @-mention chip styles for TipTap composer + history bubbles.
 *
 * Design:
 * - Same font size as surrounding prose (inherit) — never force text-xs.
 * - Distinguish type with accent color + a middle-dot separator, not size.
 * - No fill / border pill so it sits in the sentence without a “tag” box.
 */
import { cn } from "@/lib/utils"
import type { MentionType } from "@/lib/api/mentions"

const TYPE_TEXT: Record<MentionType, string> = {
  device: "text-accent-cyan",
  rule: "text-accent-orange",
  transform: "text-accent-purple",
  agent: "text-accent-emerald",
}

/** Inline mention shell — metrics inherit from composer / bubble text. */
export function mentionChipClass(extra?: string): string {
  return cn(
    "nm-mention inline-flex items-baseline gap-1 align-baseline font-medium",
    extra,
  )
}

/** Type word: accent color only, same size as the name. */
export function mentionTypeBadgeClass(type: MentionType | string): string {
  const color = TYPE_TEXT[type as MentionType] ?? "text-muted-foreground"
  return cn("nm-mention-type shrink-0", color)
}

export function mentionSepClass(): string {
  return "nm-mention-sep shrink-0 font-normal text-muted-foreground"
}

export function mentionLabelClass(): string {
  return "nm-mention-label"
}

/** TipTap host descendant selectors (ProseMirror renders the node). */
export const mentionChipHostSelectors = cn(
  "[&_.nm-mention]:inline-flex [&_.nm-mention]:items-baseline [&_.nm-mention]:gap-1",
  "[&_.nm-mention]:align-baseline [&_.nm-mention]:font-medium [&_.nm-mention]:bg-transparent",
  "[&_.nm-mention-type]:shrink-0",
  "[&_.nm-mention-type--device]:text-accent-cyan",
  "[&_.nm-mention-type--rule]:text-accent-orange",
  "[&_.nm-mention-type--transform]:text-accent-purple",
  "[&_.nm-mention-type--agent]:text-accent-emerald",
  "[&_.nm-mention-sep]:shrink-0 [&_.nm-mention-sep]:font-normal [&_.nm-mention-sep]:text-muted-foreground",
  "[&_.nm-mention[data-invalid=true]]:text-error",
  "[&_.nm-mention[data-invalid=true]_span]:text-error",
  "[&_.nm-mention[data-invalid=true]]:underline",
)

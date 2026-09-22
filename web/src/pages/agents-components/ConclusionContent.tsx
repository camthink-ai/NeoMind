/**
 * ConclusionContent — renders an execution conclusion.
 *
 * Structured (L0) agents conclude with a JSON object of their output
 * fields; reasoning agents conclude with prose. Rendering both through a
 * markdown renderer produced a raw JSON wall for the structured case, so
 * object-shaped conclusions render as labeled value rows and everything
 * else keeps its markdown.
 */

import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { cn } from '@/lib/utils'

interface ConclusionContentProps {
  content: string
  /** Tailwind classes for the JSON row container (spacing differs per host). */
  className?: string
}

/** Parse `content` into [field, value] pairs when it's a plain JSON object. */
function asFieldEntries(content: string): Array<[string, string]> | null {
  const trimmed = content.trim()
  if (!trimmed.startsWith('{') || !trimmed.endsWith('}')) return null
  try {
    const parsed: unknown = JSON.parse(trimmed)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    const entries = Object.entries(parsed as Record<string, unknown>)
    if (entries.length === 0) return null
    return entries.map(([k, v]) => [
      k,
      typeof v === 'string' ? v : JSON.stringify(v),
    ])
  } catch {
    return null
  }
}

export function ConclusionContent({ content, className }: ConclusionContentProps) {
  const fields = asFieldEntries(content)

  if (fields) {
    return (
      <div className={cn('grid gap-x-4 gap-y-1.5 sm:grid-cols-2', className)}>
        {fields.map(([name, value]) => (
          <div key={name} className="flex items-baseline justify-between gap-3 border-b border-border py-1 last:border-0">
            <span className="truncate font-mono text-xs text-muted-foreground">{name}</span>
            <span className="shrink-0 text-sm font-medium tabular-nums">{value}</span>
          </div>
        ))}
      </div>
    )
  }

  return (
    <div className={cn('prose prose-sm dark:prose-invert max-w-none prose-p:my-1', className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  )
}

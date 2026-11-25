/**
 * Shared section shell for AgentEditorV2.
 * Divider-based hierarchy inside one form surface — not stacked shadow cards.
 */
import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

export interface EditorSectionProps {
  title: ReactNode
  /** Optional muted count / status next to the title */
  meta?: ReactNode
  /** Optional trailing actions (e.g. Add) */
  actions?: ReactNode
  children: ReactNode
  className?: string
  /** data-section attribute for tests / a11y */
  'data-section'?: string
  /** First section in a FormSurface — no top rule */
  first?: boolean
}

export function EditorSection({
  title,
  meta,
  actions,
  children,
  className,
  'data-section': dataSection,
  first = false,
}: EditorSectionProps) {
  return (
    <section
      className={cn(
        'px-5 py-5 md:px-6',
        !first && 'border-t border-border',
        className,
      )}
      data-section={dataSection}
    >
      <header className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <h3 className="text-base font-semibold tracking-tight text-foreground">
              {title}
            </h3>
            {meta ? (
              <span className="text-xs text-muted-foreground">{meta}</span>
            ) : null}
          </div>
        </div>
        {actions ? <div className="shrink-0">{actions}</div> : null}
      </header>
      <div className="space-y-4">{children}</div>
    </section>
  )
}

/** Single bordered surface that groups EditorSections into one composition. */
export function FormSurface({
  children,
  className,
}: {
  children: ReactNode
  className?: string
}) {
  return (
    <div
      className={cn(
        'overflow-hidden rounded-xl border border-border bg-card',
        className,
      )}
    >
      {children}
    </div>
  )
}

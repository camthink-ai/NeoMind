/**
 * BuilderShell — unified split-workspace shell for automation builders.
 * Composes FullScreenDialog into: Header / config rail (Sidebar) / workspace (Main) / sticky Footer.
 * Mobile: config rail collapses into a <details> section atop the workspace.
 */
import { ReactNode } from 'react'
import { useIsMobile } from '@/hooks/useMobile'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogContent,
  FullScreenDialogSidebar,
  FullScreenDialogMain,
  FullScreenDialogFooter,
} from './FullScreenDialog'

export type BuilderAccent = 'indigo' | 'emerald'

const accentIcon: Record<BuilderAccent, { bg: string; text: string }> = {
  indigo: { bg: 'bg-accent-indigo-light', text: 'text-accent-indigo' },
  emerald: { bg: 'bg-accent-emerald-light', text: 'text-accent-emerald' },
}

export interface BuilderShellProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  accent: BuilderAccent
  title: string
  subtitle?: string
  /** Identity icon node (rendered in a rounded accent-bg badge). */
  icon: ReactNode
  /** Optional node rendered in the header actions slot (e.g. enable-status dot). */
  statusIndicator?: ReactNode
  /** Config rail content (metadata fields). */
  config: ReactNode
  /** Main workspace content (the builder's core canvas). */
  workspace: ReactNode
  /** Optional rightmost rail (e.g. live EffectivePlan preview). */
  aside?: ReactNode
  /** Footer actions. Container is justify-end. */
  footer: ReactNode
  /** Optional label for mobile config collapse (defaults to '配置'). */
  mobileConfigLabel?: string
}

export function BuilderShell({
  open,
  onOpenChange,
  accent,
  title,
  subtitle,
  icon,
  statusIndicator,
  config,
  workspace,
  aside,
  footer,
  mobileConfigLabel,
}: BuilderShellProps) {
  const isMobile = useIsMobile()
  const a = accentIcon[accent]
  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={icon}
        iconBg={a.bg}
        iconColor={a.text}
        title={title}
        subtitle={subtitle}
        onClose={() => onOpenChange(false)}
        actions={statusIndicator}
      />
      <FullScreenDialogContent>
        {isMobile ? (
          <FullScreenDialogMain className="p-4 md:p-5">
            <details className="mb-4 rounded-lg border border-border">
              <summary className="cursor-pointer px-4 py-2.5 text-sm font-medium text-foreground">
                {mobileConfigLabel ?? '配置'}
              </summary>
              <div className="space-y-3.5 p-4 pt-0">{config}</div>
            </details>
            {workspace}
            {aside && <div className="mt-5">{aside}</div>}
          </FullScreenDialogMain>
        ) : (
          <>
            <FullScreenDialogSidebar className="w-64 overflow-y-auto bg-[var(--sidebar-bg)] p-5 md:w-64 xl:w-72 md:bg-[var(--sidebar-bg)] md:dark:bg-[var(--sidebar-bg)]">
              {config}
            </FullScreenDialogSidebar>
            <FullScreenDialogMain className="bg-background p-6">{workspace}</FullScreenDialogMain>
            {aside && (
              <aside className="hidden w-72 shrink-0 overflow-y-auto border-l border-border bg-muted-20 p-5 lg:block xl:w-80">
                {aside}
              </aside>
            )}
          </>
        )}
      </FullScreenDialogContent>
      <FullScreenDialogFooter className="justify-end">{footer}</FullScreenDialogFooter>
    </FullScreenDialog>
  )
}
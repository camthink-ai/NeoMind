/**
 * BuilderShell — unified split-workspace shell for automation builders.
 * Composes FullScreenDialog into: Header / config rail (Sidebar) / workspace (Main) / sticky Footer.
 * Mobile: config rail collapses into a <details> section atop the workspace.
 */
import { ReactNode } from 'react'
import { useIsMobile } from '@/hooks/useMobile'
import { cn } from '@/lib/utils'
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
  /** Footer actions. Container is justify-between: put secondary actions first, primary last. */
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
          </FullScreenDialogMain>
        ) : (
          <>
            <FullScreenDialogSidebar className="w-[440px] md:w-[440px] overflow-y-auto p-4">
              {config}
            </FullScreenDialogSidebar>
            <FullScreenDialogMain className="p-5">{workspace}</FullScreenDialogMain>
          </>
        )}
      </FullScreenDialogContent>
      <FullScreenDialogFooter className="justify-between">{footer}</FullScreenDialogFooter>
    </FullScreenDialog>
  )
}
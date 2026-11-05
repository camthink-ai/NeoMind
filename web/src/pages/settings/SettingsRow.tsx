import { ReactNode } from "react"
import { Label } from "@/components/ui/label"
import { cn } from "@/lib/utils"

interface SettingsRowProps {
  /** Row label (string, or a node when an inline adornment is needed). */
  label: ReactNode
  /** Helper text rendered under the label. */
  description?: ReactNode
  /** Optional icon rendered before the label block (e.g. image-retention). */
  leadingIcon?: ReactNode
  /** Right-side control: Select / Switch / a wrapped node (e.g. loader + select). */
  children: ReactNode
  className?: string
  /** Keep a switch beside its label even on narrow screens. */
  inlineControl?: boolean
  controlClassName?: string
}

/**
 * SettingsRow — the canonical settings-page row: label + description on the
 * left, control right-aligned; stacks vertically on mobile.
 *
 * Used across the Preferences tab so every row shares identical spacing,
 * alignment and typography. Chosen over the generic design-system
 * `FormField horizontal` because that variant left-aligns the control and
 * drops its help text beneath it — fine in a narrow dialog, but the
 * label-left / control-right pattern reads better in a wide settings page.
 */
export const settingsSectionTitle = 'mb-3 text-sm font-semibold text-foreground'
export const settingsGroup = 'rounded-lg border border-border bg-card px-4 sm:px-5 [&>[data-settings-row]+[data-settings-row]]:border-t [&>[data-settings-row]]:border-border'
export const settingsFooter = 'border-t border-border py-4'

export function SettingsRow({ label, description, leadingIcon, className, children, inlineControl = false, controlClassName }: SettingsRowProps) {
  return (
    <div data-settings-row className={cn(
      "flex gap-x-8 gap-y-3 py-4",
      inlineControl ? "flex-row items-center justify-between" : "flex-col sm:flex-row sm:items-center sm:justify-between",
      className,
    )}>
      <div className="flex min-w-0 flex-1 items-start gap-2.5">
        {leadingIcon && <span className="mt-0.5 shrink-0">{leadingIcon}</span>}
        <div className="min-w-0">
          <Label className="text-sm font-medium leading-5">{label}</Label>
          {description && (
            <p className="mt-1 max-w-prose break-words text-xs leading-relaxed text-muted-foreground">{description}</p>
          )}
        </div>
      </div>
      <div className={cn(
        "flex min-w-0 items-center justify-end gap-2 sm:shrink-0",
        inlineControl ? "shrink-0" : "w-full sm:w-60",
        controlClassName,
      )}>
        {children}
      </div>
    </div>
  )
}

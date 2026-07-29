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
export function SettingsRow({ label, description, leadingIcon, className, children }: SettingsRowProps) {
  return (
    <div className={cn("flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2", className)}>
      <div className="flex items-center gap-2">
        {leadingIcon}
        <div>
          <Label className="text-sm font-medium">{label}</Label>
          {description && (
            <p className="text-xs text-muted-foreground mt-0.5">{description}</p>
          )}
        </div>
      </div>
      {children}
    </div>
  )
}

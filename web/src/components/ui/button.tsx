import * as React from "react"
import { Slot } from "@radix-ui/react-slot"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const buttonVariants = cva(
  // Focus indicator: a thin (1px) neutral ring via `ring-ring`. History: `--ring`
  // used to alias `--brand`, so `ring-ring` flashed an orange halo on keyboard
  // focus / WebKit tap-to-focus ("random orange edges" on ghost buttons). That
  // was patched with `ring-foreground/30`, but Tailwind can't apply the `/opacity`
  // modifier to CSS-var colors (`var(--foreground)`), so that class silently
  // failed to generate and the focus ring vanished entirely. `--ring` has since
  // been redefined to a neutral foreground@35% (`color-mix`), so `ring-ring` is
  // now safe AND actually renders. 1px + no offset = visible for keyboard a11y
  // without reading as a brand accent. Per-variant hover:bg-* + active:scale
  // handle pointer feedback.
  "inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium transition-colors duration-fast focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 disabled:saturate-50",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary-hover",
        destructive:
          "bg-destructive text-destructive-foreground hover:bg-destructive-hover",
        outline:
          "border border-input bg-card hover:bg-accent hover:text-accent-foreground",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary-hover",
        ghost: "hover:bg-accent hover:text-accent-foreground",
        link: "text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-9 rounded-md px-3",
        lg: "h-11 rounded-md px-8",
        icon: "h-10 w-10",
        // Extra-small: inline mini buttons (28px) — list rows, text-adjacent actions
        xs: "h-7 rounded-md px-2 text-xs",
        // Small icon button (32×32) — inside lists / cards
        "icon-sm": "h-8 w-8",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, style, ...props }, ref) => {
    const Comp = asChild ? Slot : "button"
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        style={{ touchAction: 'manipulation', ...style }}
        {...props}
      />
    )
  }
)
Button.displayName = "Button"

export { Button, buttonVariants }

// ─────────────────────────────────────────────────────────────────────────────
// IconButton — convenience wrapper for icon-only buttons.
//
// Use this instead of bare <button class="p-1 rounded-md hover:bg-muted">.
// Defaults: ghost variant, muted-foreground text, hover -> foreground + muted bg.
// Sizes: "sm" (h-8 w-8, 32px) for inline/list, "md" (h-10 w-10, 40px) for
// standalone primary icon actions.
// ─────────────────────────────────────────────────────────────────────────────

export interface IconButtonProps
  extends Omit<ButtonProps, "size"> {
  size?: "sm" | "md"
}

const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ className, size = "sm", variant = "ghost", ...props }, ref) => (
    <Button
      ref={ref}
      variant={variant}
      size={size === "sm" ? "icon-sm" : "icon"}
      className={cn(
        "shrink-0 text-muted-foreground hover:text-foreground hover:bg-muted",
        className,
      )}
      {...props}
    />
  ),
)
IconButton.displayName = "IconButton"

export { IconButton }

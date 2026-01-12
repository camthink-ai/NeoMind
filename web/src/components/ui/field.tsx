import * as React from "react"
import { cn } from "@/lib/utils"
import { Label } from "@/components/ui/label"

export interface FieldProps extends React.HTMLAttributes<HTMLDivElement> {
  children: React.ReactNode
}

export const Field = React.forwardRef<HTMLDivElement, FieldProps>(
  ({ className, children, ...props }, ref) => (
    <div ref={ref} className={cn("flex flex-col gap-1.5", className)} {...props}>
      {children}
    </div>
  )
)
Field.displayName = "Field"

export const FieldGroup = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => (
  <div
    ref={ref}
    className={cn("flex flex-col gap-4", className)}
    {...props}
  />
))
FieldGroup.displayName = "FieldGroup"

export const FieldLabel = React.forwardRef<
  HTMLLabelElement,
  React.LabelHTMLAttributes<HTMLLabelElement>
>(({ className, ...props }, ref) => (
  <Label
    ref={ref}
    className={cn("text-sm font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70", className)}
    {...props}
  />
))
FieldLabel.displayName = "FieldLabel"

export const FieldDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
))
FieldDescription.displayName = "FieldDescription"

export const FieldMessage = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    ref={ref}
    className={cn("text-sm font-medium text-destructive", className)}
    {...props}
  />
))
FieldMessage.displayName = "FieldMessage"

export const FieldSeparator = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, children, ...props }, ref) => (
  <div
    ref={ref}
    role="separator"
    className={cn("relative flex py-1 items-center", className)}
    {...props}
  >
    <div className="flex-grow border-t border-border"></div>
    {children ? (
      <>
        <span className="mx-4 text-xs text-muted-foreground uppercase">
          {children}
        </span>
        <div className="flex-grow border-t border-border"></div>
      </>
    ) : null}
  </div>
))
FieldSeparator.displayName = "FieldSeparator"

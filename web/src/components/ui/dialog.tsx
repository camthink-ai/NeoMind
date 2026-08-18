import * as React from "react"
import * as DialogPrimitive from "@radix-ui/react-dialog"
import { X } from "lucide-react"
import { cn } from "@/lib/utils"
import { getPortalRoot } from "@/lib/portal"
import { useIsMobile, useSafeAreaInsets } from "@/hooks/useMobile"

// Symbol to mark DialogHeader components for reliable detection
const DIALOG_HEADER_SYMBOL = Symbol('DialogHeader')

const Dialog = DialogPrimitive.Root

const DialogTrigger = DialogPrimitive.Trigger

const DialogPortal = (props: React.ComponentPropsWithoutRef<typeof DialogPrimitive.Portal>) => (
  <DialogPrimitive.Portal {...props} container={getPortalRoot()} />
)

const DialogClose = DialogPrimitive.Close

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      "fixed inset-0 bg-overlay-heavy duration-normal ease-out data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
      "z-50",
      className
    )}
    {...props}
  />
))
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName

/**
 * DialogContent - Main dialog component
 *
 * Close button is rendered in DialogHeader for both mobile and desktop
 * Dialogs without DialogHeader get a fallback close button
 */
const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & { fullScreenOnMobile?: boolean }
>(({ className, children, fullScreenOnMobile = true, ...props }, ref) => {
  const isMobile = useIsMobile()
  const insets = useSafeAreaInsets()

  // Check if DialogHeader exists in children
  const hasDialogHeader = React.Children.toArray(children).some((child) => {
    if (!React.isValidElement(child)) return false
    const type = child.type as any
    // Check by Symbol marker (most reliable)
    if (type?._DIALOG_HEADER_SYMBOL === DIALOG_HEADER_SYMBOL) return true
    // Check by displayName
    if (type?.displayName === "DialogHeader") return true
    // Check by type name (for named functions)
    if (type?.name === "DialogHeader") return true
    return false
  })

  // Extract z-index from className for nested dialog support — must happen for
  // BOTH branches: a dialog nested above a z-[100] fullscreen layer passes
  // className="z-[110]", and the overlay has to lift with the content or it
  // renders under the fullscreen layer (content visible, scrim gone,
  // outside-click dismissal broken).
  const zIndexMatch = className?.match(/z-\[?(\d+)\]?/)
  const overlayClassName = zIndexMatch ? `z-[${zIndexMatch[1]}]` : undefined

  // Mobile: full screen
  if (isMobile && fullScreenOnMobile) {
    return (
      <DialogPortal>
        <DialogPrimitive.Overlay
          className={cn(
            "fixed inset-0 z-50 bg-overlay-heavy",
            "data-[state=open]:animate-in data-[state=closed]:animate-out",
            "data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
            overlayClassName
          )}
        />
        <DialogPrimitive.Content
          ref={ref}
          className={cn(
            "fixed !left-0 !top-0 !right-0 !bottom-0 !w-full !h-full !max-w-full !translate-x-0 !translate-y-0 !rounded-none z-50 bg-popover flex flex-col",
            "data-[state=open]:animate-in data-[state=closed]:animate-out",
            "data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
            className
          )}
          style={{
            paddingTop: insets.top,
            paddingBottom: insets.bottom,
            paddingLeft: insets.left,
            paddingRight: insets.right,
          }}
          {...props}
        >
          {/* Fallback close button for dialogs without DialogHeader */}
          {!hasDialogHeader && (
            <div className="flex justify-end px-4 py-2">
              <DialogPrimitive.Close className="inline-flex items-center justify-center rounded-md p-2 opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none">
                <X className="h-5 w-5" />
                <span className="sr-only">Close</span>
              </DialogPrimitive.Close>
            </div>
          )}
          {children}
        </DialogPrimitive.Content>
      </DialogPortal>
    )
  }

  // Desktop: centered dialog
  return (
    <DialogPortal>
      <DialogOverlay className={overlayClassName} />
      <DialogPrimitive.Content
        ref={ref}
        className={cn(
          "fixed left-[50%] top-[50%] w-full max-w-lg -translate-x-1/2 -translate-y-1/2 border bg-popover shadow-lg duration-normal ease-spring-soft data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%] rounded-lg sm:rounded-xl",
          "z-50",
          "m-0 p-4 sm:p-6",
          !className?.includes("max-h-") && "max-h-[calc(100dvh-2rem)] sm:max-h-[85dvh]",
          !className?.includes("overflow-") && "overflow-y-auto",
          className
        )}
        {...props}
      >
        {children}
        {/* Fallback close button for dialogs without DialogHeader */}
        {!hasDialogHeader && (
          <DialogPrimitive.Close className="absolute right-4 top-4 inline-flex items-center justify-center rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none data-[state=open]:bg-accent data-[state=open]:text-muted-foreground">
            <X className="h-4 w-4" />
            <span className="sr-only">Close</span>
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Content>
    </DialogPortal>
  )
})
DialogContent.displayName = DialogPrimitive.Content.displayName

/**
 * DialogHeader - Contains title and close button
 *
 * The close button is rendered here (not in DialogContent) for unified layout
 */
const DialogHeader = function DialogHeader({
  className,
  children,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  const isMobile = useIsMobile()

  return (
    <div
      className={cn(
        // Header container with title and close button side by side
        "flex items-center justify-between gap-2",
        // Mobile styling
        isMobile && "px-4 py-4 border-b shrink-0 bg-popover",
        className
      )}
      {...props}
    >
      {/* Title content */}
      <div className={cn(
        "flex flex-col gap-1.5 flex-1 min-w-0",
        !isMobile && "text-left"
      )}>
        {children}
      </div>
      {/* Close button */}
      <DialogPrimitive.Close className={cn(
        "inline-flex items-center justify-center rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none data-[state=open]:bg-accent data-[state=open]:text-muted-foreground",
        // Mobile: larger button
        isMobile ? "p-2" : "p-0.5"
      )}>
        <X className={cn("h-4 w-4", isMobile && "h-5 w-5")} />
        <span className="sr-only">Close</span>
      </DialogPrimitive.Close>
    </div>
  )
}
DialogHeader.displayName = "DialogHeader"
// Add Symbol marker for reliable detection
;(DialogHeader as any)._DIALOG_HEADER_SYMBOL = DIALOG_HEADER_SYMBOL

const DialogFooter = ({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) => {
  const isMobile = useIsMobile()
  return (
    <div
      className={cn(
        "flex flex-row justify-end gap-2 sm:gap-3",
        "mt-4 sm:mt-0",
        isMobile && "px-4 py-3 border-t shrink-0 bg-popover sticky bottom-0 z-10",
        className
      )}
      {...props}
    />
  )
}
DialogFooter.displayName = "DialogFooter"

const DialogContentBody = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement>
>(({ className, ...props }, ref) => {
  const isMobile = useIsMobile()
  return (
    <div
      ref={ref}
      className={cn(
        "flex-1 overflow-y-auto",
        isMobile && "px-4",
        className
      )}
      {...props}
    />
  )
})
DialogContentBody.displayName = "DialogContentBody"

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn(
      "text-lg font-semibold leading-none",
      className
    )}
    {...props}
  />
))
DialogTitle.displayName = DialogPrimitive.Title.displayName

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
))
DialogDescription.displayName = DialogPrimitive.Description.displayName

export {
  Dialog,
  DialogPortal,
  DialogOverlay,
  DialogClose,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
  DialogContentBody,
}

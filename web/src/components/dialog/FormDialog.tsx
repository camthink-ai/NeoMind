import { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import * as DialogPrimitive from '@radix-ui/react-dialog'
import { X, Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface FormDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  description?: string
  width?: 'sm' | 'md' | 'lg' | 'xl' | '2xl' | '3xl'
  children: ReactNode
  footer?: ReactNode
  loading?: boolean
  className?: string
}

const widthClass = {
  sm: 'max-w-md',
  md: 'max-w-lg',
  lg: 'max-w-xl',
  xl: 'max-w-2xl',
  '2xl': 'max-w-3xl',
  '3xl': 'max-w-5xl',
}

/**
 * Standard form dialog with consistent styling
 *
 * Replaces the inconsistent Dialog patterns across the app.
 *
 * @example
 * <FormDialog
 *   open={open}
 *   onOpenChange={setOpen}
 *   title="Add Device"
 *   description="Configure a new device"
 *   width="xl"
 *   loading={saving}
 *   footer={<DialogFooter><Button>Save</Button></DialogFooter>}
 * >
 *   <Form>...</Form>
 * </FormDialog>
 */
export function FormDialog({
  open,
  onOpenChange,
  title,
  description,
  width = 'md',
  children,
  footer,
  loading = false,
  className,
}: FormDialogProps) {
  const { t } = useTranslation('common')
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay
          className={cn(
            'fixed inset-0 z-50',
            'bg-black/80 backdrop-blur-sm',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
            'duration-200'
          )}
        />
        <DialogPrimitive.Content
          className={cn(
            'fixed left-1/2 top-1/2 z-50',
            'grid w-full gap-4',
            'bg-background p-6',
            'shadow-lg',
            'duration-200',
            'data-[state=open]:animate-in data-[state=closed]:animate-out',
            'data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0',
            'data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95',
            'data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%]',
            'data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]',
            'sm:rounded-lg',
            widthClass[width],
            '-translate-x-1/2 -translate-y-1/2',
            className
          )}
          onPointerDownOutside={(e) => {
            if (loading) e.preventDefault()
          }}
          onEscapeKeyDown={(e) => {
            if (loading) e.preventDefault()
          }}
        >
          <div className="flex flex-col space-y-1.5 text-center sm:text-left">
            <div className="flex items-center justify-between">
              <DialogPrimitive.Title className="text-lg font-semibold leading-none tracking-tight">
                {title}
              </DialogPrimitive.Title>
              <DialogPrimitive.Close
                disabled={loading}
                className="rounded-sm opacity-70 ring-offset-background transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:pointer-events-none data-[state=open]:bg-accent data-[state=open]:text-muted-foreground"
              >
                <X className="h-4 w-4" />
                <span className="sr-only">{t('close')}</span>
              </DialogPrimitive.Close>
            </div>
            {description && (
              <DialogPrimitive.Description className="text-sm text-muted-foreground">
                {description}
              </DialogPrimitive.Description>
            )}
          </div>

          {children}

          {footer && (
            <div className={cn('flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2', loading && 'opacity-50 pointer-events-none')}>
              {footer}
            </div>
          )}

          {loading && (
            <div className="absolute inset-0 flex items-center justify-center bg-background/50 rounded-lg">
              <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
            </div>
          )}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

/**
 * DialogFooter helper component
 */
export function DialogFooter({ className, children }: { className?: string; children: ReactNode }) {
  return (
    <div className={cn('flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2', className)}>
      {children}
    </div>
  )
}

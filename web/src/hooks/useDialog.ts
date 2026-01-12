import { useState, useCallback } from 'react'

export interface UseDialogReturn<T> {
  open: boolean
  data: T | null
  openWith: (data?: T) => void
  close: () => void
  setOpen: (open: boolean) => void
  toggle: () => void
}

/**
 * Dialog state management hook
 *
 * Handles dialog open/close state and optional data.
 *
 * @example
 * const dialog = useDialog<Device>()
 *
 * <Dialog open={dialog.open} onOpenChange={dialog.setOpen}>
 *   {dialog.data && <DeviceDetail device={dialog.data} />}
 * </Dialog>
 *
 * // Open with data
 * dialog.openWith(device)
 * dialog.close()
 */
export function useDialog<T = any>(): UseDialogReturn<T> {
  const [open, setOpen] = useState(false)
  const [data, setData] = useState<T | null>(null)

  const openWith = useCallback((dialogData?: T) => {
    setData(dialogData ?? null)
    setOpen(true)
  }, [])

  const close = useCallback(() => {
    setOpen(false)
    // Clear data after animation
    setTimeout(() => setData(null), 200)
  }, [])

  const toggle = useCallback(() => {
    setOpen(prev => !prev)
    if (!open) setData(null)
  }, [open])

  return {
    open,
    data,
    openWith,
    close,
    setOpen,
    toggle,
  }
}

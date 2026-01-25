import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { useConfirm } from "@/components/ui/use-confirm"
import { useTranslation } from "react-i18next"

export function Confirmer() {
  const { dialogs, close } = useConfirm()
  const { t } = useTranslation()

  if (dialogs.length === 0) return null

  const dialog = dialogs[0]

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      dialog.resolve(false)
      close(dialog.id)
    }
  }

  const handleConfirm = () => {
    dialog.resolve(true)
    close(dialog.id)
  }

  const handleCancel = () => {
    dialog.resolve(false)
    close(dialog.id)
  }

  return (
    <AlertDialog open={true} onOpenChange={handleOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          {dialog.title && <AlertDialogTitle>{dialog.title}</AlertDialogTitle>}
          {dialog.description && (
            <AlertDialogDescription>{dialog.description}</AlertDialogDescription>
          )}
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel onClick={handleCancel}>
            {dialog.cancelText || t("common:cancel")}
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            className={
              dialog.variant === "destructive"
                ? "bg-destructive text-destructive-foreground hover:bg-destructive/90"
                : ""
            }
          >
            {dialog.confirmText || t("common:confirm")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

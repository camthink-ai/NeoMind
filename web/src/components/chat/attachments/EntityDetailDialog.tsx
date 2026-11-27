/**
 * EntityDetailDialog — responsive fallback host when no in-flow split pane is
 * present (i.e. desktop PanelChatView and all mobile screens):
 *
 * - Desktop (`!isMobile`): right-side overlay Sheet drawer (never obscures the
 *   main page on the left; dismiss via backdrop, X, or Escape).
 * - Mobile (`isMobile`): NeoMind's standard full-screen UnifiedFormDialog with
 *   safe-area back header and full-width bottom action buttons.
 */

import { RefreshCw } from "lucide-react"
import type { ChatAttachment, ChatReference } from "@/types"
import { cn } from "@/lib/utils"
import { useIsMobile } from "@/hooks/useMobile"
import { UnifiedFormDialog } from "@/components/dialog/UnifiedFormDialog"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { Button } from "@/components/ui/button"
import { useEntityInspector } from "./EntityInspectorContent"

interface EntityDetailDialogProps {
  attachment: ChatAttachment | null
  open: boolean
  onOpenChange: (open: boolean) => void
  onAttachReference?: (ref: ChatReference) => void
}

export function EntityDetailDialog({
  attachment,
  open,
  onOpenChange,
  onAttachReference,
}: EntityDetailDialogProps) {
  const isMobile = useIsMobile()
  const active = open && !!attachment && attachment.kind !== "image"

  const {
    loading,
    isZh,
    kindMeta,
    refresh,
    body,
    footerButtons,
    testDialog,
    testDialogOpen,
  } = useEntityInspector({
    attachment,
    active,
    keepOpenOnMention: false,
    onClose: () => onOpenChange(false),
    onAttachReference,
  })

  if (!attachment || attachment.kind === "image" || !kindMeta) return null

  const Icon = kindMeta.Icon

  // Mobile: native full-screen floating sheet via UnifiedFormDialog
  if (isMobile) {
    return (
      <>
        <UnifiedFormDialog
          open={open && !testDialogOpen}
          onOpenChange={onOpenChange}
          title={attachment.title}
          icon={<Icon className={cn("h-4 w-4 shrink-0", kindMeta.color)} aria-hidden="true" />}
          width="md"
          fullScreenOnMobile
          loading={loading}
          footer={footerButtons}
        >
          {body}
        </UnifiedFormDialog>
        {testDialog}
      </>
    )
  }

  // Desktop fallback (e.g. inside global right-docked PanelChatView):
  // slide-over right drawer so the main workspace on the left remains visible.
  return (
    <>
      <Sheet open={open && !testDialogOpen} onOpenChange={onOpenChange}>
        <SheetContent
          side="right"
          className="flex w-[440px] max-w-[92vw] flex-col gap-0 overflow-hidden p-0 sm:max-w-[460px]"
        >
          {/* Header — matched h-14 (56px) with footer */}
          <div className="flex h-14 flex-shrink-0 items-center justify-between gap-2 border-b border-glass-border pl-5 pr-12">
            <div className="flex min-w-0 flex-1 items-center gap-2.5">
              <div
                className={cn(
                  "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
                  kindMeta.iconBg
                )}
              >
                <Icon className={cn("h-4 w-4", kindMeta.color)} strokeWidth={1.75} aria-hidden="true" />
              </div>
              <SheetTitle className="truncate text-sm font-semibold leading-tight text-foreground">
                {attachment.title}
              </SheetTitle>
            </div>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              onClick={refresh}
              disabled={loading}
              aria-label={isZh ? "刷新" : "Refresh"}
              title={isZh ? "刷新数据" : "Refresh"}
              className="h-8 w-8 shrink-0 rounded-lg text-muted-foreground hover:text-foreground sm:h-8 sm:w-8"
            >
              <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} />
            </Button>
          </div>

          {/* Scrollable content */}
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-5 py-5">
            {body}
          </div>

          {/* Sticky footer — matched h-14 (56px) with header */}
          <div className="flex h-14 flex-shrink-0 items-center justify-end gap-2 border-t border-glass-border px-5">
            {footerButtons}
          </div>
        </SheetContent>
      </Sheet>
      {testDialog}
    </>
  )
}

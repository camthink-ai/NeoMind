/**
 * ZoomableImage - thumbnail that opens a full-screen preview.
 *
 * Used for both user attachments and markdown <img> in AI replies. The
 * clickable wrapper is a span with role="button" (not a <button>) so an
 * image nested inside a markdown link does not create invalid nested
 * interactive elements.
 */

import { useState, type MouseEvent } from "react"
import { useTranslation } from "react-i18next"
import { Maximize2 } from "lucide-react"
import { cn } from "@/lib/utils"
import {
  Dialog,
  DialogContent,
  DialogTitle,
} from "@/components/ui/dialog"

interface ZoomableImageProps {
  src: string
  alt?: string
  className?: string
  imgClassName?: string
  /** Accessible label for the preview trigger. */
  label?: string
}

export function ZoomableImage({
  src,
  alt,
  className,
  imgClassName,
  label,
}: ZoomableImageProps) {
  const { t } = useTranslation("chat")
  const [open, setOpen] = useState(false)
  const description = label || alt || t("image.view")

  const openPreview = (e: MouseEvent) => {
    // Images can be wrapped in markdown links; the preview should win.
    e.preventDefault()
    e.stopPropagation()
    setOpen(true)
  }

  return (
    <>
      <span
        role="button"
        tabIndex={0}
        onClick={openPreview}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault()
            setOpen(true)
          }
        }}
        aria-label={description}
        className={cn(
          "group relative my-2 inline-block max-w-full cursor-zoom-in overflow-hidden rounded-xl border border-border",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          className
        )}
      >
        <img
          src={src}
          alt={alt ?? ""}
          loading="lazy"
          decoding="async"
          className={cn(
            "max-h-[420px] w-auto max-w-full object-contain",
            imgClassName
          )}
        />
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 flex items-center justify-center bg-overlay-light opacity-0 transition-opacity duration-fast group-hover:opacity-100 group-focus-visible:opacity-100"
        >
          <Maximize2 className="h-4 w-4 text-white" />
        </span>
      </span>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent
          fullScreenOnMobile={false}
          className="max-w-[92vw] overflow-hidden border-none bg-transparent p-2 text-white shadow-none sm:max-w-[92vw] sm:p-2"
        >
          <DialogTitle className="sr-only">{description}</DialogTitle>
          <img
            src={src}
            alt={alt ?? ""}
            className="mx-auto max-h-[85vh] w-auto max-w-full rounded-lg object-contain"
          />
        </DialogContent>
      </Dialog>
    </>
  )
}

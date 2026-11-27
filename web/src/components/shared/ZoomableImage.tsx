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
  /**
   * Optional max bounding box (px) used to size the wrapper tightly to the
   * image's natural aspect ratio on load — prevents WebKit/WKWebView from
   * leaving empty horizontal whitespace inside the border when `max-height`
   * constrains width.
   */
  fitBox?: { maxWidth: number; maxHeight: number }
}

export function ZoomableImage({
  src,
  alt,
  className,
  imgClassName,
  label,
  fitBox,
}: ZoomableImageProps) {
  const { t } = useTranslation("chat")
  const [open, setOpen] = useState(false)
  const [naturalSize, setNaturalSize] = useState<{ w: number; h: number } | null>(null)
  const description = label || alt || t("image.view")

  const fittedStyle =
    fitBox && naturalSize && naturalSize.w > 0 && naturalSize.h > 0
      ? (() => {
          const scale = Math.min(
            1,
            fitBox.maxWidth / naturalSize.w,
            fitBox.maxHeight / naturalSize.h
          )
          return {
            width: Math.max(1, Math.round(naturalSize.w * scale)),
            maxWidth: "100%",
            aspectRatio: `${naturalSize.w} / ${naturalSize.h}`,
          }
        })()
      : undefined

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
        style={fittedStyle}
        className={cn(
          "group/zoom relative my-2 inline-block max-w-full cursor-zoom-in overflow-hidden rounded-xl border border-border",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          className
        )}
      >
        <img
          src={src}
          alt={alt ?? ""}
          loading="lazy"
          decoding="async"
          onLoad={(e) => {
            if (!fitBox) return
            const { naturalWidth: w, naturalHeight: h } = e.currentTarget
            if (w > 0 && h > 0) {
              setNaturalSize((prev) => (prev?.w === w && prev?.h === h ? prev : { w, h }))
            }
          }}
          className={cn(
            fittedStyle
              ? "block h-full w-full object-contain"
              : "block max-h-72 w-auto max-w-full object-contain",
            imgClassName
          )}
        />
        <span
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 flex items-center justify-center bg-overlay-light opacity-0 transition-opacity duration-fast group-hover/zoom:opacity-100 group-focus-visible/zoom:opacity-100"
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

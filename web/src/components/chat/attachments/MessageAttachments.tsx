/**
 * MessageAttachments — tiered structured attachment renderer below assistant prose.
 *
 * Uses NeoMind's native Tailwind v3 CSS-variable tokens (no invalid `/20`, `/70`
 * opacity modifiers on CSS-variable colors — see tailwind.config.js):
 * - Surfaces: `bg-card`, `bg-muted-20`, `bg-muted-30`, `bg-muted-50`
 * - Borders: `border-border`, `hover:border-border-hover`
 * - Category accents: `text-accent-*` + `bg-accent-*-light`
 *
 * Three distinct visual weights:
 * 1. `media` (image): fallback thumbnails for /api/images/... not already inlined.
 * 2. `card` (created/updated/executed): compact result cards (`@md:grid-cols-2`).
 * 3. `chip` (queried/deleted): ultra-light inline pill row.
 */

import { useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import {
  AtSign,
  Box,
  ChevronDown,
  Download,
  LayoutGrid,
  Sparkles,
  Workflow,
  Zap,
} from "lucide-react"
import type { ChatAttachment, ChatAttachmentKind, ChatReference } from "@/types"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { resolveImageSrc } from "@/lib/imageUtils"
import { ErrorBoundary } from "@/components/shared/ErrorBoundary"
import { ZoomableImage } from "@/components/shared/ZoomableImage"
import { EntityDetailDialog } from "./EntityDetailDialog"

const DEFAULT_VISIBLE_MEDIA = 4
const DEFAULT_VISIBLE_CARDS = 3
const DEFAULT_VISIBLE_CHIPS = 6

interface MessageAttachmentsProps {
  attachments: ChatAttachment[]
  /** Assistant markdown prose — used as client-side safety dedup for images. */
  content?: string
  onAttachReference?: (ref: ChatReference) => void
  /**
   * Optional host-level inspector handler (e.g. desktop /chat in-flow split
   * side pane). When omitted, clicks fall back to EntityDetailDialog (overlay
   * Sheet on desktop PanelChatView, full-screen UnifiedFormDialog on mobile).
   */
  onInspectAttachment?: (attachment: ChatAttachment) => void
  /** Currently inspected attachment ID (highlights the active card/chip). */
  activeAttachmentId?: string | null
}

const KIND_ACCENT: Record<ChatAttachmentKind, {
  text: string
  iconBg: string
  dot: string
  Icon: typeof Box
}> = {
  device: {
    text: "text-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: Box,
  },
  rule: {
    text: "text-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: Zap,
  },
  transform: {
    text: "text-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: Workflow,
  },
  agent: {
    text: "text-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: Sparkles,
  },
  dashboard: {
    text: "text-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: LayoutGrid,
  },
  image: {
    text: "text-muted-foreground",
    iconBg: "bg-muted-30 border border-border",
    dot: "bg-muted-foreground",
    Icon: Box,
  },
}

const STATUS_DOT: Record<string, string> = {
  ok: "bg-success",
  running: "bg-info motion-safe:animate-pulse",
  warn: "bg-warning",
  error: "bg-error",
  muted: "bg-muted-foreground",
}

function isImageAlreadyInMarkdown(url: string, content?: string): boolean {
  if (!content || !url) return false
  const imgRegex = /!\[[^\]]*\]\(\s*<?([^)\s>]+)>?/g
  let match: RegExpExecArray | null
  while ((match = imgRegex.exec(content)) !== null) {
    const rawTarget = match[1]
    const cleanTarget = rawTarget.split(/[?#]/)[0]
    if (cleanTarget === url || rawTarget === url || cleanTarget.endsWith(url)) {
      return true
    }
  }
  return false
}

function actionBadgeLabel(action: ChatAttachment["action"], isZh: boolean): string | null {
  switch (action) {
    case "created":
      return isZh ? "已创建" : "Created"
    case "updated":
      return isZh ? "已更新" : "Updated"
    case "executed":
      return isZh ? "已执行" : "Executed"
    case "deleted":
      return isZh ? "已删除" : "Deleted"
    default:
      return null
  }
}

function statusDisplayLabel(label: string, isZh: boolean): string {
  if (!isZh) return label
  switch (label.toLowerCase()) {
    case "online":
      return "在线"
    case "offline":
      return "离线"
    case "enabled":
      return "已启用"
    case "disabled":
      return "已停用"
    case "executing":
    case "running":
      return "运行中"
    case "idle":
    case "active":
    case "ready":
      return "就绪"
    case "paused":
      return "已暂停"
    case "deleted":
      return "已删除"
    default:
      return label
  }
}

function meaningfulImageCaption(title?: string): string | null {
  if (!title) return null
  const trimmed = title.trim()
  if (!trimmed) return null
  // Hide generic boilerplate or raw UUID filenames — let the image speak for itself.
  if (/^edited image$/i.test(trimmed)) return null
  if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.[a-z0-9]+$/i.test(trimmed)) {
    return null
  }
  return trimmed
}

function downloadFilenameFromUrl(url: string): string {
  const clean = url.split(/[?#]/)[0]
  return clean.split("/").pop() || "image.png"
}

function formatSubtitle(subtitle: string, isZh: boolean): string {
  if (!isZh) return subtitle
  const m = /^(\d+)\s+components?$/i.exec(subtitle.trim())
  if (m) return `${m[1]} 个组件`
  return subtitle
}

function MessageAttachmentsInner({
  attachments,
  content,
  onAttachReference,
  onInspectAttachment,
  activeAttachmentId,
}: MessageAttachmentsProps) {
  const { i18n } = useTranslation("chat")
  const isZh = i18n.language?.toLowerCase().startsWith("zh")

  const [mediaExpanded, setMediaExpanded] = useState(false)
  const [cardsExpanded, setCardsExpanded] = useState(false)
  const [chipsExpanded, setChipsExpanded] = useState(false)
  const [activeEntity, setActiveEntity] = useState<ChatAttachment | null>(null)

  const handleOpenEntity = (item: ChatAttachment) => {
    if (onInspectAttachment) {
      onInspectAttachment(item)
    } else {
      setActiveEntity(item)
    }
  }

  const { media, cards, chips } = useMemo(() => {
    const m: Array<{ item: ChatAttachment; src: string }> = []
    const ca: ChatAttachment[] = []
    const ch: ChatAttachment[] = []
    for (const item of attachments) {
      if (!item || typeof item !== "object") continue
      if (item.kind === "image" || item.display === "media") {
        if (item.imageUrl && !isImageAlreadyInMarkdown(item.imageUrl, content)) {
          const src = resolveImageSrc(item.imageUrl)
          if (src) m.push({ item, src })
        }
      } else if (item.display === "card") {
        ca.push(item)
      } else {
        ch.push(item)
      }
    }
    return { media: m, cards: ca, chips: ch }
  }, [attachments, content])

  if (media.length === 0 && cards.length === 0 && chips.length === 0) {
    return null
  }

  const visibleMedia = mediaExpanded ? media : media.slice(0, DEFAULT_VISIBLE_MEDIA)
  const hiddenMediaCount = media.length - visibleMedia.length

  const visibleCards = cardsExpanded ? cards : cards.slice(0, DEFAULT_VISIBLE_CARDS)
  const hiddenCardsCount = cards.length - visibleCards.length

  const visibleChips = chipsExpanded ? chips : chips.slice(0, DEFAULT_VISIBLE_CHIPS)
  const hiddenChipsCount = chips.length - visibleChips.length

  const emitMention = (e: React.MouseEvent, item: ChatAttachment) => {
    e.stopPropagation()
    if (!onAttachReference || !item.entityId || item.kind === "image") return
    onAttachReference({
      type: item.kind,
      id: item.entityId,
      label: item.title,
    })
  }

  return (
    <div
      className="mt-3 space-y-2.5"
      data-testid="message-attachments"
    >
      {/* 1. Fallback Image Media — frameless, aspect-ratio hugging to blend seamlessly into prose */}
      {media.length > 0 && (
        <div className="space-y-2" data-testid="attachments-media">
          {visibleMedia.length === 1 ? (
            (() => {
              const { item: img, src } = visibleMedia[0]
              const caption = meaningfulImageCaption(img.title)
              return (
                <div className="group/media relative inline-block max-w-full align-top">
                  <ZoomableImage
                    src={src}
                    alt={caption ?? (isZh ? "生成的图片" : "Image")}
                    fitBox={{ maxWidth: 420, maxHeight: 288 }}
                    className="my-0 block"
                  />
                  <a
                    href={src}
                    download={downloadFilenameFromUrl(img.imageUrl || src)}
                    onClick={(e) => e.stopPropagation()}
                    aria-label={isZh ? "下载图片" : "Download image"}
                    title={isZh ? "下载图片" : "Download image"}
                    className="absolute right-2 top-2 z-10 inline-flex h-7 w-7 items-center justify-center rounded-lg bg-black/55 text-white opacity-0 backdrop-blur-sm transition-opacity duration-fast hover:bg-black/75 focus-visible:opacity-100 group-hover/media:opacity-100"
                  >
                    <Download className="h-3.5 w-3.5" aria-hidden="true" />
                  </a>
                  {caption && (
                    <span className="pointer-events-none absolute bottom-2 left-2 z-10 max-w-[80%] truncate rounded-md bg-black/55 px-2 py-0.5 text-[11px] text-white opacity-0 backdrop-blur-sm transition-opacity duration-fast group-hover/media:opacity-100">
                      {caption}
                    </span>
                  )}
                </div>
              )
            })()
          ) : (
            <div className="grid max-w-md grid-cols-2 gap-2">
              {visibleMedia.map(({ item: img, src }) => {
                const caption = meaningfulImageCaption(img.title)
                return (
                  <div
                    key={img.id}
                    className="group/media relative overflow-hidden rounded-xl border border-border"
                  >
                    <ZoomableImage
                      src={src}
                      alt={caption ?? (isZh ? "图片" : "Image")}
                      className="my-0 block w-full rounded-none border-0"
                      imgClassName="block h-40 w-full object-cover"
                    />
                    <a
                      href={src}
                      download={downloadFilenameFromUrl(img.imageUrl || src)}
                      onClick={(e) => e.stopPropagation()}
                      aria-label={isZh ? "下载图片" : "Download image"}
                      title={isZh ? "下载图片" : "Download image"}
                      className="absolute right-2 top-2 z-10 inline-flex h-7 w-7 items-center justify-center rounded-lg bg-black/55 text-white opacity-0 backdrop-blur-sm transition-opacity duration-fast hover:bg-black/75 focus-visible:opacity-100 group-hover/media:opacity-100"
                    >
                      <Download className="h-3.5 w-3.5" aria-hidden="true" />
                    </a>
                    {caption && (
                      <span className="pointer-events-none absolute bottom-2 left-2 z-10 max-w-[80%] truncate rounded-md bg-black/55 px-2 py-0.5 text-[11px] text-white opacity-0 backdrop-blur-sm transition-opacity duration-fast group-hover/media:opacity-100">
                        {caption}
                      </span>
                    )}
                  </div>
                )
              })}
            </div>
          )}
          {hiddenMediaCount > 0 && (
            <button
              type="button"
              onClick={() => setMediaExpanded(true)}
              className="inline-flex h-6 min-h-0 min-w-0 items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
            >
              <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
              {isZh ? `展开其余 ${hiddenMediaCount} 张图片` : `Show ${hiddenMediaCount} more images`}
            </button>
          )}
        </div>
      )}

      {/* 2. Compact Result Cards (write / execute outcomes) */}
      {cards.length > 0 && (
        <div className="space-y-1.5" data-testid="attachments-cards">
          <div className="grid grid-cols-1 @md:grid-cols-2 gap-2">
            {visibleCards.map((card) => {
              const style = KIND_ACCENT[card.kind] ?? KIND_ACCENT.device
              const Icon = style.Icon
              const actionText = actionBadgeLabel(card.action, isZh)
              const statusDot = card.status ? (STATUS_DOT[card.status.tone] ?? STATUS_DOT.muted) : null

              const isInspected = activeAttachmentId === card.id

              return (
                <div
                  key={card.id}
                  className={cn(
                    "group/card flex min-w-0 items-center rounded-xl border bg-card shadow-sm transition-all",
                    isInspected
                      ? "border-primary bg-muted-20"
                      : "border-border hover:border-border-hover hover:bg-muted-20"
                  )}
                >
                  <button
                    type="button"
                    data-testid={`attachment-card-${card.kind}`}
                    onClick={() => handleOpenEntity(card)}
                    className="flex min-h-0 min-w-0 flex-1 items-center gap-2.5 rounded-xl px-3 py-2 text-left focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <div
                      className={cn(
                        "flex h-8 w-8 shrink-0 items-center justify-center rounded-lg",
                        style.iconBg
                      )}
                    >
                      <Icon className={cn("h-4 w-4", style.text)} aria-hidden="true" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 items-center gap-1.5">
                        <span className="truncate text-body font-medium text-foreground">
                          {card.title}
                        </span>
                        {actionText && (
                          <span
                            className={cn(
                              textNano,
                              "shrink-0 rounded bg-muted-30 px-1.5 py-0.5 font-medium text-muted-foreground"
                            )}
                          >
                            {actionText}
                          </span>
                        )}
                      </div>
                      <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                        {card.status && (
                          <span className="inline-flex shrink-0 items-center gap-1.5">
                            <span
                              className={cn("h-1.5 w-1.5 shrink-0 rounded-full", statusDot)}
                              aria-hidden="true"
                            />
                            <span>{statusDisplayLabel(card.status.label, isZh)}</span>
                          </span>
                        )}
                        {card.status && card.subtitle && (
                          <span aria-hidden="true" className="opacity-50">·</span>
                        )}
                        {card.subtitle && (
                          <span className="truncate">{formatSubtitle(card.subtitle, isZh)}</span>
                        )}
                      </div>
                    </div>
                  </button>
                  {onAttachReference && card.entityId && (
                    <button
                      type="button"
                      onClick={(e) => emitMention(e, card)}
                      aria-label={isZh ? `引用 ${card.title}` : `Mention ${card.title}`}
                      title={isZh ? "引用到输入框" : "Mention in chat"}
                      className="mr-2 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted-50 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <AtSign className="h-3.5 w-3.5" aria-hidden="true" />
                    </button>
                  )}
                </div>
              )
            })}
          </div>
          {hiddenCardsCount > 0 && (
            <button
              type="button"
              onClick={() => setCardsExpanded(true)}
              className="inline-flex h-6 min-h-0 min-w-0 items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
            >
              <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
              {isZh ? `展开其余 ${hiddenCardsCount} 项` : `Show ${hiddenCardsCount} more`}
            </button>
          )}
        </div>
      )}

      {/* 3. Lightweight Entity Chips (read-only queries) */}
      {chips.length > 0 && (
        <div
          className="flex flex-wrap items-center gap-1.5"
          data-testid="attachments-chips"
        >
          {visibleChips.map((chip) => {
            const style = KIND_ACCENT[chip.kind] ?? KIND_ACCENT.device
            const isInspected = activeAttachmentId === chip.id
            const canMention = Boolean(onAttachReference && chip.entityId && chip.action !== "deleted")
            return (
              <div
                key={chip.id}
                className={cn(
                  "group/chip inline-flex h-7 max-w-full items-center rounded-full border text-xs transition-colors",
                  isInspected
                    ? "border-primary bg-muted-30"
                    : "border-border bg-card hover:border-border-hover hover:bg-muted-30"
                )}
              >
                <button
                  type="button"
                  data-testid={`attachment-chip-${chip.kind}`}
                  onClick={() => handleOpenEntity(chip)}
                  className={cn(
                    "inline-flex h-full min-h-0 min-w-0 items-center gap-1.5 rounded-full text-left leading-none text-foreground transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
                    canMention ? "pl-2.5 pr-1.5" : "px-2.5"
                  )}
                >
                  <span
                    className={cn("h-1.5 w-1.5 shrink-0 rounded-full", style.dot)}
                    aria-hidden="true"
                  />
                  <span className="max-w-[14rem] truncate">{chip.title}</span>
                </button>
                {canMention && (
                  <button
                    type="button"
                    onClick={(e) => emitMention(e, chip)}
                    aria-label={isZh ? `引用 ${chip.title}` : `Mention ${chip.title}`}
                    title={isZh ? "引用到输入框" : "Mention in chat"}
                    className="mr-1 inline-flex h-5 w-5 min-h-0 min-w-0 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted-50 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <AtSign className="h-3 w-3" aria-hidden="true" />
                  </button>
                )}
              </div>
            )
          })}
          {hiddenChipsCount > 0 && (
            <button
              type="button"
              onClick={() => setChipsExpanded(true)}
              className="inline-flex h-7 min-h-0 min-w-0 items-center rounded-full border border-dashed border-border bg-card px-2.5 text-xs leading-none text-muted-foreground transition-colors hover:border-border-hover hover:bg-muted-30 hover:text-foreground"
            >
              +{hiddenChipsCount}
            </button>
          )}
        </div>
      )}

      {!onInspectAttachment && (
        <EntityDetailDialog
          attachment={activeEntity}
          open={!!activeEntity}
          onOpenChange={(next) => {
            if (!next) setActiveEntity(null)
          }}
          onAttachReference={onAttachReference}
        />
      )}
    </div>
  )
}

export function MessageAttachments(props: MessageAttachmentsProps) {
  if (!props.attachments || props.attachments.length === 0) return null
  return (
    <ErrorBoundary resetKey={props.attachments.map((a) => a?.id ?? "").join(",")}>
      <MessageAttachmentsInner {...props} />
    </ErrorBoundary>
  )
}

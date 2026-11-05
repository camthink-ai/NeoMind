/**
 * ChatComposer — the unified chat input shared by the chat page and the
 * global chat panel. One container: textarea on top, toolbar below
 * (image upload / model selector / context usage / send-or-cancel).
 *
 * The parent owns the text value, attachments list, streaming state, and
 * backend selection — this component owns the box's layout and the
 * file→compressed-image upload pipeline.
 */

import { useRef, useState, type RefObject } from "react"
import { useTranslation } from "react-i18next"
import { Archive, ArrowUp, Check, ChevronDown, Eye, Image as ImageIcon, Loader2, Sparkles, Trash2, Wrench, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useConfirm } from "@/components/ui/use-confirm"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { useToast } from "@/hooks/use-toast"
import { useIsMobile } from "@/hooks/useMobile"
import type { ChatImage, LlmBackendInstance } from "@/types"
import { cn } from "@/lib/utils"
import { textNano } from "@/design-system/tokens/typography"
import { compressImage, MAX_ORIGINAL_IMAGE_MB, COMPRESSED_IMAGE_MB } from "@/lib/image"

interface ChatComposerProps {
  value: string
  onChange: (value: string) => void
  onSend: () => void
  onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
  textareaRef?: RefObject<HTMLTextAreaElement>
  placeholder?: string
  /** Swaps in while streaming (e.g. "请等待..."). */
  streamingPlaceholder?: string
  isStreaming?: boolean
  /** Extra gate on top of isStreaming (e.g. panel disables input mid-stream). */
  disabled?: boolean
  /** Shown in place of Send while streaming. */
  onCancel?: () => void
  // Attachments — pass onAttachmentsChange to enable the upload button.
  attachments?: ChatImage[]
  onAttachmentsChange?: (images: ChatImage[]) => void
  supportsMultimodal?: boolean
  /** Image support comes from the vision companion, not the active model. */
  visionDelegate?: boolean
  // Model selector — hidden when no backends are passed.
  backends?: LlmBackendInstance[]
  activeBackendId?: string | null
  onActivateBackend?: (id: string) => void
  // Context usage indicator — null hides it. The breakdown powers the hover
  // card; parts are undefined while only a character estimate is available.
  contextUsage?: {
    pending?: boolean
    used: number
    max: number
    system?: number
    tools?: number
    history?: number
    estimated?: boolean
    messageCount?: number
  } | null
  // Manual context actions, shown in the usage card when provided (main
  // chat page wires them; hosts without a session id omit them).
  onCompact?: () => void
  onClearContext?: () => void
  compacting?: boolean
  /** Textarea max height in px (default 100, chat desktop uses 160). */
  maxHeight?: number
  /** Textarea resting height in px. Welcome hosts raise it so the input
      reads as the page's primary action; conversation keeps the compact
      single-line default (44). */
  minHeight?: number
}

export function ChatComposer({
  value,
  onChange,
  onSend,
  onKeyDown,
  textareaRef,
  placeholder,
  streamingPlaceholder,
  isStreaming = false,
  disabled = false,
  onCancel,
  attachments = [],
  onAttachmentsChange,
  supportsMultimodal = false,
  visionDelegate,
  backends,
  activeBackendId,
  onActivateBackend,
  contextUsage,
  onCompact,
  onClearContext,
  compacting = false,
  maxHeight = 100,
  minHeight = 44,
}: ChatComposerProps) {
  const { t } = useTranslation(["chat", "common"])
  const { toast } = useToast()
  const { confirm } = useConfirm()
  const isMobile = useIsMobile()
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isUploadingImage, setIsUploadingImage] = useState(false)

  const canAttach = !!onAttachmentsChange
  const inputDisabled = disabled || isStreaming

  // Both actions rewrite what the session remembers (compact swaps older
  // history for a summary, clear deletes it outright) — guard them with the
  // Confirmer so a misclick can't silently alter the conversation. Wrapped
  // here so every host (chat page, panel) gets the guard for free.
  const confirmCompact = async () => {
    if (!onCompact) return
    const yes = await confirm({
      title: t("chat:context.compactConfirmTitle", "Compact context?"),
      description: t("chat:context.compactConfirmDesc", "Earlier messages are replaced by a summary to free context space. History stays visible in the UI; only later requests are affected."),
      confirmText: t("chat:context.compact", "Compact"),
    })
    if (yes) onCompact()
  }

  const confirmClear = async () => {
    if (!onClearContext) return
    const yes = await confirm({
      title: t("chat:context.clearConfirmTitle", "Clear conversation history?"),
      description: t("chat:context.clearConfirmDesc", "All messages in this session will be deleted. This cannot be undone."),
      confirmText: t("chat:context.clear", "Clear"),
      variant: "destructive",
    })
    if (yes) onClearContext()
  }

  // Compress and append selected image files (mirrors the chat page pipeline).
  const handleImageSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return

    setIsUploadingImage(true)
    try {
      const newImages: ChatImage[] = []
      for (let i = 0; i < files.length; i++) {
        const file = files[i]
        if (!file.type.startsWith("image/")) continue

        if (file.size > MAX_ORIGINAL_IMAGE_MB * 1024 * 1024) {
          toast({ title: `Image ${file.name} is too large. Maximum size is ${MAX_ORIGINAL_IMAGE_MB}MB.`, variant: "destructive" })
          continue
        }

        const dataUrl = await compressImage(file, COMPRESSED_IMAGE_MB)
        newImages.push({
          data: dataUrl,
          mimeType: "image/jpeg", // Compressed images are always JPEG
        })
      }

      if (newImages.length > 0) {
        onAttachmentsChange?.([...attachments, ...newImages])
      }
    } catch {
      toast({ title: t("common:imageProcessFailed"), variant: "destructive" })
    } finally {
      setIsUploadingImage(false)
      if (fileInputRef.current) {
        fileInputRef.current.value = ""
      }
    }
  }

  const removeAttachment = (index: number) => {
    onAttachmentsChange?.(attachments.filter((_, i) => i !== index))
  }

  const hasAttachments = attachments.length > 0
  const canSend = (value.trim().length > 0 || hasAttachments) && !inputDisabled

  // Single root: hosts apply `space-y-*` to their direct children, which
  // would otherwise insert a gap between the preview strip and the box.
  return (
    <div>
      {/* Image previews */}
      {hasAttachments && (
        <div className="flex flex-wrap gap-1.5 mb-1">
          {attachments.map((image, index) => (
            <div key={index} className="relative group">
              <img
                src={image.data}
                alt={`Attached ${index + 1}`}
                className="h-8 w-8 sm:h-9 sm:w-9 object-cover rounded-md border border-border"
              />
              <button
                type="button"
                aria-label={`Remove attachment ${index + 1}`}
                className={cn(
                  // 10px hover-only was untouchable; mobile gets a always-on,
                  // thumb-sized target, desktop keeps the hover-reveal dot.
                  "absolute -top-1 -right-1 rounded-full bg-destructive text-destructive-foreground flex items-center justify-center transition-opacity p-0 ring-1 ring-background",
                  isMobile ? "h-4 w-4 opacity-100" : "h-3 w-3 opacity-0 group-hover:opacity-100"
                )}
                onClick={() => removeAttachment(index)}
              >
                <X className={isMobile ? "h-2.5 w-2.5" : "h-2 w-2"} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Single unified input box — everything inside one container */}
      <div className="rounded-lg border border-input bg-card shadow-sm transition-colors">
        {/* Textarea — fills the top, borderless */}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={isStreaming && streamingPlaceholder ? streamingPlaceholder : placeholder}
          rows={1}
          disabled={inputDisabled}
          className={cn(
            "w-full block px-4 pt-3 pb-1 resize-none text-sm leading-5 bg-transparent",
            "placeholder:text-muted-foreground",
            "focus:outline-none",
            "max-h-[100px] scroll-mb-32",
            "disabled:opacity-60"
          )}
          style={{ minHeight: `${minHeight}px` }}
          onInput={(e) => {
            const target = e.target as HTMLTextAreaElement
            target.style.height = "auto"
            target.style.height = Math.max(minHeight, Math.min(target.scrollHeight, maxHeight)) + "px"
          }}
        />

        {/* Bottom toolbar — left: image + model + context, right: send */}
        <div className="flex items-center gap-1 px-2 pb-2">
          {/* Image upload */}
          {canAttach && (
            <>
              <input
                ref={fileInputRef}
                type="file"
                accept="image/*"
                multiple
                className="hidden"
                onChange={handleImageSelect}
                disabled={inputDisabled || !supportsMultimodal}
              />
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => fileInputRef.current?.click()}
                disabled={inputDisabled || !supportsMultimodal}
                className={cn(
                  "rounded-lg flex-shrink-0 text-muted-foreground hover:text-foreground",
                  !supportsMultimodal && "opacity-50"
                )}
                title={supportsMultimodal ? (visionDelegate ? t("chat:model.visionViaCompanion") : t("chat:model.addImage")) : t("chat:model.notSupportImage")}
              >
                {isUploadingImage ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : hasAttachments ? (
                  <div className="relative">
                    <ImageIcon className="h-4 w-4" />
                    <span className="absolute -top-1 -right-1 bg-primary text-primary-foreground text-nano rounded-full h-4 w-4 flex items-center justify-center font-semibold tabular-nums">
                      {attachments.length}
                    </span>
                  </div>
                ) : (
                  <ImageIcon className="h-4 w-4" />
                )}
              </Button>
            </>
          )}

          {/* Model selector */}
          {backends && backends.length > 0 && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-8 px-2 rounded-lg text-muted-foreground hover:text-foreground text-xs gap-1 max-w-[120px] sm:max-w-[140px]"
                >
                  <span className="truncate">
                    {backends.find(b => b.id === activeBackendId)?.name ||
                     backends.find(b => b.id === activeBackendId)?.model ||
                     t("chat:input.selectModel")}
                  </span>
                  <ChevronDown className="h-3.5 w-3.5 shrink-0" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-[min(21rem,calc(100vw-2rem))] max-h-[min(24rem,60vh)] overflow-y-auto scrollbar-none rounded-xl p-1.5 shadow-md">
                <div className="sticky top-0 z-10 rounded-t-lg bg-popover px-2.5 py-2">
                  <span className="text-xs font-medium text-muted-foreground">{t("chat:input.selectLLMModel")}</span>
                </div>
                <div className="p-1">
                  {backends.map((backend) => {
                    const isActive = backend.id === activeBackendId
                    return (
                      <DropdownMenuItem
                        key={backend.id}
                        onClick={() => onActivateBackend?.(backend.id)}
                        className={cn("flex items-center gap-2.5 rounded-lg px-2.5 py-2", isActive && "bg-muted")}
                      >
                        <span className="flex h-4 w-4 shrink-0 items-center justify-center">
                          {isActive ? <Check className="h-4 w-4 text-foreground" aria-hidden="true" /> :
                            <span className={cn("h-1.5 w-1.5 rounded-full", backend.healthy ? "bg-success" : "bg-muted-foreground/50")} />}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="flex min-w-0 items-center">
                            <span className={cn("truncate text-sm text-foreground", isActive && "font-medium")}>{backend.name || backend.model}</span>
                          </div>
                          <div className="mt-0.5 flex min-w-0 items-center gap-1.5">
                            <span className="shrink-0 rounded bg-muted px-1 py-px text-[10px] leading-4 text-muted-foreground">{backend.backend_type}</span>
                            <span className="min-w-0 flex-1 truncate text-[11px] text-muted-foreground">{backend.model}</span>
                            <span className="flex shrink-0 items-center gap-1.5 text-muted-foreground" aria-label={[
                              backend.capabilities?.supports_multimodal && t("chat:capability.vision", { defaultValue: "Vision" }),
                              backend.capabilities?.supports_tools && t("chat:capability.tools", { defaultValue: "Tools" }),
                              backend.capabilities?.supports_thinking && t("chat:capability.thinking", { defaultValue: "Thinking" }),
                            ].filter(Boolean).join(', ')}>
                              {backend.capabilities?.supports_multimodal && <span title={t("chat:model.supportsVision")}><Eye className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                              {backend.capabilities?.supports_tools && <span title={t("chat:model.supportsTools")}><Wrench className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                              {backend.capabilities?.supports_thinking && <span title={t("chat:model.supportsThinking")}><Sparkles className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                            </span>
                          </div>
                        </div>
                      </DropdownMenuItem>
                    )
                  })}
                </div>
              </DropdownMenuContent>
            </DropdownMenu>
          )}

          {/* Context usage — progress ring; hover for the breakdown card */}
          {contextUsage && (() => {
            const ratio = Math.min(1, contextUsage.used / contextUsage.max)
            const color = ratio > 0.9 ? 'var(--error)' : ratio > 0.7 ? 'var(--warning)' : 'var(--muted-foreground)'
            const R = 7
            const C = 2 * Math.PI * R
            // One unit per card, decided by the window size — mixing raw
            // tokens and K-formatted values in the same card reads as noise.
            const useK = contextUsage.max >= 1000
            const fmt = (n: number) => {
              if (!useK) return String(Math.round(n))
              if (n === 0) return '0'
              const k = n / 1000
              return `${k >= 10 ? k.toFixed(1) : k.toFixed(2)}K`
            }
            const pct = Math.round(ratio * 100)
            const rows: Array<{ label: string; value?: number; color: string; suffix?: string }> = [
              { label: t('chat:context.systemPrompt', 'System prompt'), value: contextUsage.system, color: 'var(--primary)' },
              { label: t('chat:context.toolDefs', 'Tool definitions'), value: contextUsage.tools, color: 'var(--accent-cyan)' },
              { label: t('chat:context.history', 'Conversation history'), value: contextUsage.history, color: 'var(--muted-foreground)', suffix: contextUsage.messageCount != null ? t('chat:context.msgCount', { defaultValue: ' · {{count}} msgs', count: contextUsage.messageCount }) : undefined },
            ]
            const availableRows = rows.filter(r => r.value != null)
            return (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      className="shrink-0 h-8 flex items-center gap-1.5 text-muted-foreground hover:text-foreground transition-colors"
                      aria-label={`${t('chat:context.title', 'Context usage')}: ${contextUsage.pending ? t('chat:context.awaitingMeasurement', 'Waiting for request context measurement') : `${pct}%`}`}
                    >
                      <svg width="20" height="20" viewBox="0 0 20 20" className="-rotate-90">
                        <circle cx="10" cy="10" r={R} fill="none" stroke="var(--border)" strokeWidth="2.5" />
                        <circle
                          cx="10" cy="10" r={R} fill="none"
                          stroke={color} strokeWidth="2.5" strokeLinecap="round"
                          strokeDasharray={C} strokeDashoffset={C * (1 - ratio)}
                          className="transition-all duration-500"
                        />
                      </svg>
                      <span className={cn("text-xs tabular-nums", ratio > 0.9 ? "text-error" : ratio > 0.7 ? "text-warning" : "text-muted-foreground")}>
                        {contextUsage.pending ? "—" : fmt(contextUsage.used)}
                      </span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top" align="end" className="w-72 max-w-[calc(100vw-1rem)] rounded-xl border border-border p-4 shadow-lg">
                    <div className="mb-3 flex items-start justify-between gap-3">
                      <div>
                        <p className="text-xs font-semibold text-foreground">{t('chat:context.title', 'Context usage')}</p>
                        <p className="mt-1 text-lg font-semibold tabular-nums leading-none text-foreground">
                          {contextUsage.pending ? "—" : fmt(contextUsage.used)}
                          <span className="text-xs font-normal text-muted-foreground"> / {fmt(contextUsage.max)}</span>
                        </p>
                      </div>
                      <span className="rounded-full bg-muted px-2 py-0.5 text-xs font-medium tabular-nums text-foreground">
                        {contextUsage.pending ? "—" : pct}%
                      </span>
                    </div>
                    <div className="mb-3 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuenow={contextUsage.pending ? 0 : pct} aria-valuemin={0} aria-valuemax={100}>
                      <div className="h-full rounded-full transition-[width] duration-500" style={{ width: `${contextUsage.pending ? 0 : pct}%`, backgroundColor: color }} />
                    </div>
                    {availableRows.length > 0 ? <div className="space-y-2 pt-1">
                      {availableRows.map(r => (
                        <div key={r.label} className="flex items-center gap-2">
                          <span className="h-1.5 w-1.5 rounded-full shrink-0" style={{ backgroundColor: r.color }} />
                          <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{r.label}</span>
                          <span className="shrink-0 text-xs font-medium tabular-nums text-foreground">{(r.value != null ? fmt(r.value) : '—') + (r.suffix ?? '')}</span>
                        </div>
                      ))}
                    </div> : <p className="text-xs text-muted-foreground">{t('chat:context.breakdownUnavailable')}</p>}
                    {contextUsage.estimated && (
                      <p className="mt-3 text-nano leading-relaxed text-muted-foreground">
                        {contextUsage.pending
                          ? t('chat:context.awaitingMeasurement', 'Waiting for request context measurement')
                          : t('chat:context.estimatedHint', 'Character-based estimate — updates after the next reply')}
                      </p>
                    )}
                    {!contextUsage.estimated && (
                      <p className="mt-3 text-nano leading-relaxed text-muted-foreground">
                        {t('chat:context.measuredHint', 'Last measured model request; excludes accumulated execution output')}
                      </p>
                    )}
                    {(onCompact || onClearContext) && (
                      <div className="mt-3 flex items-center gap-2 border-t border-border pt-3">
                        {onCompact && (
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            onClick={confirmCompact}
                            disabled={compacting}
                            className="h-8 flex-1 gap-1.5 rounded-lg"
                          >
                            {compacting
                              ? <Loader2 className="h-3.5 w-3.5 animate-spin" />
                              : <Archive className="h-3.5 w-3.5" />}
                            {compacting
                              ? t('chat:context.compacting', 'Compacting…')
                              : t('chat:context.compact', 'Compact')}
                          </Button>
                        )}
                        {onClearContext && (
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            onClick={confirmClear}
                            className="h-8 flex-1 gap-1.5 rounded-lg text-muted-foreground hover:bg-destructive-light hover:text-destructive"
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                            {t('chat:context.clear', 'Clear')}
                          </Button>
                        )}
                      </div>
                    )}
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
            )
          })()}

          <div className="flex-1" />

          {/* Send or Cancel button */}
          {isStreaming ? (
            <Button
              type="button"
              onClick={onCancel}
              variant="outline"
              className="h-8 w-8 rounded-full flex-shrink-0 p-0 border-destructive text-destructive hover:bg-destructive-light"
              title={t("chat:input.cancel")}
              aria-label={t("chat:input.cancel")}
            >
              <Loader2 className="h-4 w-4 animate-spin" />
            </Button>
          ) : (
            <Button
              type="button"
              onClick={onSend}
              disabled={!canSend}
              className={cn(
                "h-8 w-8 rounded-full flex-shrink-0 p-0 transition-all",
                !canSend
                  ? "bg-muted text-muted-foreground"
                  : "bg-primary hover:bg-primary-hover text-primary-foreground"
              )}
            >
              <ArrowUp className="h-4 w-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

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
import { ArrowUp, Check, ChevronDown, Image as ImageIcon, Loader2, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { useToast } from "@/hooks/use-toast"
import type { ChatImage, LlmBackendInstance } from "@/types"
import { cn } from "@/lib/utils"
import { textNano, textMicro } from "@/design-system/tokens/typography"
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
  // Model selector — hidden when no backends are passed.
  backends?: LlmBackendInstance[]
  activeBackendId?: string | null
  onActivateBackend?: (id: string) => void
  // Context usage indicator — null hides it. The breakdown powers the hover
  // card; parts are undefined while only a character estimate is available.
  contextUsage?: {
    used: number
    max: number
    system?: number
    tools?: number
    history?: number
    estimated?: boolean
    messageCount?: number
  } | null
  /** Textarea max height in px (default 100, chat desktop uses 160). */
  maxHeight?: number
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
  backends,
  activeBackendId,
  onActivateBackend,
  contextUsage,
  maxHeight = 100,
}: ChatComposerProps) {
  const { t } = useTranslation(["chat", "common"])
  const { toast } = useToast()
  const fileInputRef = useRef<HTMLInputElement>(null)
  const [isUploadingImage, setIsUploadingImage] = useState(false)

  const canAttach = !!onAttachmentsChange
  const inputDisabled = disabled || isStreaming

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

  return (
    <>
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
                className="absolute -top-1 -right-1 h-2.5 w-2.5 rounded-full bg-destructive text-destructive-foreground flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity p-0"
                onClick={() => removeAttachment(index)}
              >
                <X className="h-2 w-2" />
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
          style={{ minHeight: "44px" }}
          onInput={(e) => {
            const target = e.target as HTMLTextAreaElement
            target.style.height = "auto"
            target.style.height = Math.max(44, Math.min(target.scrollHeight, maxHeight)) + "px"
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
                title={supportsMultimodal ? t("chat:model.addImage") : t("chat:model.notSupportImage")}
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
              <DropdownMenuContent align="start" className="w-72 max-h-[50vh] overflow-y-auto scrollbar-none p-0">
                <div className="sticky top-0 z-10 bg-popover px-3 py-2 border-b border-border">
                  <span className="font-semibold text-sm">{t("chat:input.selectLLMModel")}</span>
                </div>
                <div className="p-1">
                  {backends.map((backend) => {
                    const isActive = backend.id === activeBackendId
                    return (
                      <DropdownMenuItem
                        key={backend.id}
                        onClick={() => onActivateBackend?.(backend.id)}
                        className="gap-2.5 py-2"
                      >
                        {/* Left slot: active → Check, else health dot (symmetrical) */}
                        <div className="w-4 flex items-center justify-center shrink-0">
                          {isActive ? (
                            <Check className="h-4 w-4 text-primary" />
                          ) : (
                            <span className={cn(
                              "w-1.5 h-1.5 rounded-full",
                              backend.healthy ? "bg-success" : "bg-muted-foreground"
                            )} />
                          )}
                        </div>
                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-1.5">
                            <p className={cn("text-sm truncate", isActive && "font-medium")}>
                              {backend.name || backend.model}
                            </p>
                            <div className="flex items-center gap-1 shrink-0">
                              {backend.capabilities?.supports_multimodal && (
                                <span title={t("chat:model.supportsVision")} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-info-light text-info", textMicro)}>{t("chat:capability.vision", { defaultValue: "Vision" })}</span>
                              )}
                              {backend.capabilities?.supports_tools && (
                                <span title={t("chat:model.supportsTools")} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-accent-orange-light text-accent-orange", textMicro)}>{t("chat:capability.tools", { defaultValue: "Tools" })}</span>
                              )}
                              {backend.capabilities?.supports_thinking && (
                                <span title={t("chat:model.supportsThinking")} className={cn("inline-flex items-center px-1 h-4 rounded font-medium bg-accent-purple-light text-accent-purple", textMicro)}>{t("chat:capability.thinking", { defaultValue: "Thinking" })}</span>
                              )}
                            </div>
                          </div>
                          <p className={cn(textNano, "text-muted-foreground truncate mt-0.5")}>
                            {backend.backend_type} · {backend.model}
                          </p>
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
              { label: t('chat.context.systemPrompt', 'System prompt'), value: contextUsage.system, color: 'var(--primary)' },
              { label: t('chat.context.toolDefs', 'Tool definitions'), value: contextUsage.tools, color: 'var(--accent-cyan)' },
              { label: t('chat.context.history', 'Conversation history'), value: contextUsage.history, color: 'var(--muted-foreground)', suffix: contextUsage.messageCount != null ? t('chat.context.msgCount', { defaultValue: ' · {{count}} msgs', count: contextUsage.messageCount }) : undefined },
            ]
            return (
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      className="shrink-0 flex items-center gap-1.5 text-muted-foreground hover:text-foreground transition-colors"
                      aria-label={`${t('chat.context.title', 'Context usage')}: ${pct}%`}
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
                        {fmt(contextUsage.used)}
                      </span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top" align="end" className="w-60 p-3">
                    <p className="text-xs font-medium mb-2">
                      {t('chat.context.title', 'Context usage')}
                      <span className="ml-1.5 text-muted-foreground tabular-nums">
                        {fmt(contextUsage.used)} / {fmt(contextUsage.max)} · {pct}%
                      </span>
                    </p>
                    <div className="space-y-1.5">
                      {rows.map(r => (
                        <div key={r.label} className="flex items-center gap-2">
                          <span className="h-1.5 w-1.5 rounded-full shrink-0" style={{ backgroundColor: r.color }} />
                          <span className="text-xs text-muted-foreground flex-1 truncate">{r.label}</span>
                          <span className="text-xs tabular-nums">{(r.value != null ? fmt(r.value) : '—') + (r.suffix ?? '')}</span>
                        </div>
                      ))}
                    </div>
                    {contextUsage.estimated && (
                      <p className="mt-2 text-nano text-muted-foreground">
                        {t('chat.context.estimatedHint', 'Character-based estimate — updates after the next reply')}
                      </p>
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
              title="Cancel request"
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
    </>
  )
}

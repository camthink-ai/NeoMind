/**
 * ChatComposer — the unified chat input shared by the chat page and the
 * global chat panel. One container: textarea on top, toolbar below
 * (image upload / model selector / context usage / send-or-cancel).
 *
 * The parent owns the text value, attachments list, streaming state, and
 * backend selection — this component owns the box's layout and the
 * file→compressed-image upload pipeline.
 */

import { useEffect, useRef, useState, type RefObject } from "react"
import { useTranslation } from "react-i18next"
import {
  Archive,
  ArrowUp,
  AtSign,
  Bot,
  Check,
  Cpu,
  Eye,
  GitBranch,
  Image as ImageIcon,
  Loader2,
  Plus,
  Sparkles,
  Trash2,
  Wrench,
  X,
  Zap,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useConfirm } from "@/components/ui/use-confirm"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "@/components/ui/sheet"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { useToast } from "@/hooks/use-toast"
import { useIsMobile } from "@/hooks/useMobile"
import type { ChatContentNode, ChatImage, ChatReference, LlmBackendInstance } from "@/types"
import { cn } from "@/lib/utils"
import {
  MAX_REFERENCES,
  mentionKey,
  searchMentions,
  type MentionOption,
  type MentionType,
} from "@/lib/api/mentions"
import { compressImage, MAX_ORIGINAL_IMAGE_MB, COMPRESSED_IMAGE_MB } from "@/lib/image"
import {
  InlineMentionEditor,
  type MentionSuggestionApi,
} from "@/components/chat/inline-mention"

const MENTION_TYPE_META: Record<
  MentionType,
  { icon: typeof Cpu; badge: string }
> = {
  device: { icon: Cpu, badge: "bg-accent-cyan-light text-accent-cyan" },
  rule: { icon: Zap, badge: "bg-accent-orange-light text-accent-orange" },
  transform: { icon: GitBranch, badge: "bg-accent-purple-light text-accent-purple" },
  agent: { icon: Bot, badge: "bg-accent-emerald-light text-accent-emerald" },
}

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
  // Structured @-references — pass onReferencesChange to enable TipTap
  // inline mentions. Ordered contentNodes preserve in-sentence position;
  // references stay as the unique legacy projection for preflight.
  references?: ChatReference[]
  onReferencesChange?: (refs: ChatReference[]) => void
  contentNodes?: ChatContentNode[]
  onContentNodesChange?: (nodes: ChatContentNode[]) => void
  /** `type:id` → error detail for invalid/stale references (preflight). */
  invalidRefs?: Record<string, string>
  /**
   * Focus host for TipTap (contenteditable). When mentions are enabled the
   * classic textareaRef is unused; parents may still pass either.
   */
  editorHostRef?: RefObject<HTMLElement | null>
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
  references = [],
  onReferencesChange,
  contentNodes,
  onContentNodesChange,
  invalidRefs = {},
  editorHostRef,
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
  const internalTextareaRef = useRef<HTMLTextAreaElement>(null)
  const activeTextareaRef = textareaRef ?? internalTextareaRef
  const internalEditorHostRef = useRef<HTMLElement | null>(null)
  const activeEditorHostRef = editorHostRef ?? internalEditorHostRef
  const [isUploadingImage, setIsUploadingImage] = useState(false)
  const [modelSheetOpen, setModelSheetOpen] = useState(false)
  const [mobileExpanded, setMobileExpanded] = useState(false)

  // ---- @-mention picker (server search + pagination) ----
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  const [mentionTypeFilter, setMentionTypeFilter] = useState<MentionType | "all">("all")
  const [mentionOptions, setMentionOptions] = useState<MentionOption[]>([])
  const [mentionActive, setMentionActive] = useState(0)
  const [mentionLoading, setMentionLoading] = useState(false)
  const [mentionHasMore, setMentionHasMore] = useState(false)
  const [mentionNextOffset, setMentionNextOffset] = useState<number | null>(null)
  const [mentionTypeErrors, setMentionTypeErrors] = useState<string[]>([])
  const [mentionSearchFailed, setMentionSearchFailed] = useState(false)
  const [openMentionRequest, setOpenMentionRequest] = useState(0)
  const mentionCommandRef = useRef<MentionSuggestionApi["command"] | null>(null)
  /** Legacy textarea path only (hosts without onReferencesChange). */
  const mentionTrigger = useRef<{ start: number } | null>(null)
  /** Suppress TipTap onExit→close while an option pointerdown is selecting. */
  const suppressMentionExitClose = useRef(false)
  const canMention = !!onReferencesChange
  const canAttach = !!onAttachmentsChange
  const inputDisabled = disabled || isStreaming
  // IME composition (Chinese/Japanese input): intermediate text must not
  // open the picker — only post-composition changes count.
  const isComposing = useRef(false)
  const mentionOpenRef = useRef(false)
  const modelSheetOpenRef = useRef(false)
  /** Delayed mobile collapse — touch blurs often report null relatedTarget. */
  const mobileCollapseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    mentionOpenRef.current = mentionQuery !== null
  }, [mentionQuery])

  useEffect(() => {
    modelSheetOpenRef.current = modelSheetOpen
  }, [modelSheetOpen])

  useEffect(() => {
    return () => {
      if (mobileCollapseTimerRef.current) clearTimeout(mobileCollapseTimerRef.current)
    }
  }, [])

  useEffect(() => {
    if (mentionQuery === null) return
    const ac = new AbortController()
    setMentionLoading(true)
    setMentionSearchFailed(false)
    const timer = setTimeout(async () => {
      try {
        if (!canMention) {
          setMentionOptions([])
          setMentionHasMore(false)
          setMentionNextOffset(null)
          setMentionTypeErrors([])
          return
        }
        const result = await searchMentions({
          query: mentionQuery,
          types: mentionTypeFilter === "all" ? "all" : [mentionTypeFilter],
          limit: 20,
          offset: 0,
          signal: ac.signal,
        })
        setMentionOptions(result.items)
        setMentionActive(0)
        setMentionHasMore(result.hasMore)
        setMentionNextOffset(result.nextOffset)
        setMentionTypeErrors(
          Object.entries(result.typeErrors)
            .filter(([, msg]) => !!msg)
            .map(([type, msg]) => `${type}: ${msg}`),
        )
      } catch (err) {
        if (ac.signal.aborted) return
        setMentionOptions([])
        setMentionHasMore(false)
        setMentionNextOffset(null)
        setMentionSearchFailed(true)
      } finally {
        if (!ac.signal.aborted) setMentionLoading(false)
      }
    }, 180)
    return () => {
      ac.abort()
      clearTimeout(timer)
    }
  }, [mentionQuery, canMention, mentionTypeFilter])

  const loadMoreMentions = async () => {
    if (mentionQuery === null || mentionNextOffset == null || mentionLoading) return
    setMentionLoading(true)
    try {
      const result = await searchMentions({
        query: mentionQuery,
        types: mentionTypeFilter === "all" ? "all" : [mentionTypeFilter],
        limit: 20,
        offset: mentionNextOffset,
      })
      setMentionOptions((prev) => {
        const seen = new Set(prev.map((o) => mentionKey(o.type, o.id)))
        const merged = [...prev]
        for (const item of result.items) {
          const k = mentionKey(item.type, item.id)
          if (!seen.has(k)) {
            seen.add(k)
            merged.push(item)
          }
        }
        return merged
      })
      setMentionHasMore(result.hasMore)
      setMentionNextOffset(result.nextOffset)
    } catch {
      // keep existing page
    } finally {
      setMentionLoading(false)
    }
  }

  const closeMention = () => {
    mentionOpenRef.current = false
    mentionTrigger.current = null
    mentionCommandRef.current = null
    setMentionQuery(null)
    setMentionOptions([])
    setMentionHasMore(false)
    setMentionNextOffset(null)
    setMentionTypeErrors([])
    setMentionSearchFailed(false)
  }

  const handleSuggestionStart = (api: MentionSuggestionApi) => {
    if (inputDisabled) return
    if (references.length >= MAX_REFERENCES) return
    mentionCommandRef.current = api.command
    // Eager flag — Sheet/blur races used to read a stale false before useEffect.
    mentionOpenRef.current = true
    setMentionQuery(api.query)
  }

  const handleSuggestionUpdate = (api: MentionSuggestionApi) => {
    if (inputDisabled) {
      closeMention()
      return
    }
    mentionCommandRef.current = api.command
    mentionOpenRef.current = true
    setMentionQuery(api.query)
  }

  const handleTipTapPayload = (payload: {
    message: string
    references: ChatReference[]
    contentNodes: ChatContentNode[]
  }) => {
    onChange(payload.message)
    onReferencesChange?.(payload.references)
    onContentNodesChange?.(payload.contentNodes)
  }

  /** Scan caret-left text for an @ trigger — legacy textarea hosts only. */
  const scanMentionTrigger = (next: string, caretOverride?: number) => {
    if (!canMention || inputDisabled) return
    if (references.length >= MAX_REFERENCES) return
    const ta = activeTextareaRef.current
    const caret =
      typeof caretOverride === "number"
        ? caretOverride
        : ta && typeof ta.selectionStart === "number"
          ? ta.selectionStart
          : next.length
    const before = next.slice(0, caret)
    const m = /(^|[^@\w])@([^@\n]{0,40})$/.exec(before)
    if (m) {
      mentionTrigger.current = { start: caret - m[2].length - 1 }
      mentionOpenRef.current = true
      setMentionQuery(m[2])
    } else {
      closeMention()
    }
  }

  const handleChange = (next: string) => {
    onChange(next)
    if (isComposing.current) return
    scanMentionTrigger(next)
  }

  const handleSuggestionExit = (_meta?: { editorFocused: boolean }) => {
    if (suppressMentionExitClose.current) return
    // Anchored picker (desktop + mobile) keeps editor focus, so TipTap exit
    // means the `@` match is gone — close the host menu.
    closeMention()
  }

  const selectMention = (opt: MentionOption) => {
    // TipTap path: insert an in-sentence mention atom (duplicates allowed).
    if (mentionCommandRef.current) {
      if (references.length >= MAX_REFERENCES) {
        closeMention()
        return
      }
      suppressMentionExitClose.current = true
      try {
        mentionCommandRef.current({
          id: opt.id,
          label: opt.label,
          entityType: opt.type,
          typeLabel: t(`chat:mention.${opt.type}`, opt.type),
        })
      } finally {
        suppressMentionExitClose.current = false
      }
      closeMention()
      return
    }
    // Legacy textarea + chip path (no TipTap).
    const trig = mentionTrigger.current
    const ta = activeTextareaRef.current
    if (!trig) return
    const caret = ta ? ta.selectionStart : value.length
    const next = value.slice(0, trig.start) + value.slice(caret)
    onChange(next)
    if (references.some((r) => r.type === opt.type && r.id === opt.id)) {
      closeMention()
      requestAnimationFrame(() => {
        ta?.focus()
        try {
          ta?.setSelectionRange(trig.start, trig.start)
        } catch {
          // selection restore is best-effort
        }
      })
      return
    }
    if (references.length >= MAX_REFERENCES) {
      closeMention()
      return
    }
    onReferencesChange?.([
      ...references,
      { type: opt.type, id: opt.id, label: opt.label },
    ])
    closeMention()
    requestAnimationFrame(() => {
      ta?.focus()
      try {
        ta?.setSelectionRange(trig.start, trig.start)
      } catch {
        // selection restore is best-effort
      }
    })
  }

  const removeReference = (index: number) => {
    onReferencesChange?.(references.filter((_, i) => i !== index))
  }

  const handleMentionNavKeys = (e: {
    key: string
    preventDefault: () => void
  }): boolean => {
    if (mentionQuery === null) return false
    if (e.key === "ArrowDown") {
      e.preventDefault()
      setMentionActive((i) => Math.min(i + 1, Math.max(mentionOptions.length - 1, 0)))
      return true
    }
    if (e.key === "ArrowUp") {
      e.preventDefault()
      setMentionActive((i) => Math.max(i - 1, 0))
      return true
    }
    if ((e.key === "Enter" || e.key === "Tab") && mentionOptions[mentionActive]) {
      e.preventDefault()
      selectMention(mentionOptions[mentionActive])
      return true
    }
    if (e.key === "Escape") {
      e.preventDefault()
      closeMention()
      return true
    }
    return false
  }

  const handleKeyDownInternal = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const ime =
      isComposing.current ||
      e.nativeEvent.isComposing ||
      (e.nativeEvent as KeyboardEvent).keyCode === 229
    if (ime) {
      return
    }
    if (handleMentionNavKeys(e)) return
    if (
      e.key === "Backspace" &&
      value === "" &&
      references.length > 0 &&
      canMention
    ) {
      e.preventDefault()
      removeReference(references.length - 1)
      return
    }
    onKeyDown?.(e)
  }

  /** TipTap Enter/IME: claim keys so ProseMirror does not insert a paragraph. */
  const handleTipTapKeyDown = (event: KeyboardEvent): boolean => {
    const ime =
      isComposing.current ||
      event.isComposing ||
      event.keyCode === 229
    // While the picker is open, IME Enter must not select a mention or send.
    if (ime) {
      if (
        mentionOpenRef.current &&
        (event.key === "Enter" || event.key === "Tab")
      ) {
        event.preventDefault()
        return true
      }
      return false
    }
    if (handleMentionNavKeys(event)) return true
    if (event.key === "Enter" && !event.shiftKey) {
      // Let the host send handler run via a synthetic React key path.
      const synthetic = {
        key: "Enter",
        shiftKey: false,
        preventDefault: () => event.preventDefault(),
        nativeEvent: event,
        stopPropagation: () => event.stopPropagation(),
      } as unknown as React.KeyboardEvent<HTMLTextAreaElement>
      onKeyDown?.(synthetic)
      event.preventDefault()
      return true
    }
    return false
  }

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

  const pickMentionOption = (opt: MentionOption) => {
    // pointerdown path: select before blur/TipTap exit can clear the command.
    selectMention(opt)
  }

  const renderMentionPickerBody = () => (
    <>
      <div
        className={cn(
          "flex flex-wrap gap-1.5 border-b border-border",
          isMobile ? "px-4 py-3" : "px-3 py-2.5",
        )}
        role="tablist"
        aria-label={t("chat:mention.filterTypes", "按类型筛选")}
      >
        {(["all", "device", "rule", "transform", "agent"] as const).map((tab) => {
          const selected = mentionTypeFilter === tab
          const meta = tab === "all" ? null : MENTION_TYPE_META[tab]
          const Icon = meta?.icon
          return (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={selected}
              className={cn(
                "inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-nano transition-colors",
                selected
                  ? "bg-primary text-primary-foreground"
                  : "bg-muted-30 text-muted-foreground hover:bg-muted hover:text-foreground",
              )}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setMentionTypeFilter(tab)}
            >
              {Icon && <Icon className="h-3 w-3" aria-hidden="true" />}
              {tab === "all"
                ? t("chat:mention.filterAll", "全部")
                : t(`chat:mention.${tab}`, tab)}
            </button>
          )
        })}
      </div>
      {mentionTypeErrors.length > 0 && (
        <div className="border-b border-border px-3 py-2 text-nano text-warning" role="status">
          {t("chat:mention.partialFailure", "部分类型加载失败")}
          {": "}
          {mentionTypeErrors.join("; ")}
        </div>
      )}
      <div className={cn("overflow-y-auto", isMobile ? "max-h-[50vh]" : "max-h-64")}>
        {mentionLoading && mentionOptions.length === 0 ? (
          <div className="flex items-center gap-2 px-3 py-3 text-xs text-muted-foreground" role="status">
            <Loader2 className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            {t("chat:mention.searching", "搜索中…")}
          </div>
        ) : mentionSearchFailed ? (
          <div className="px-3 py-3 text-xs text-error" role="alert">
            {t("chat:mention.searchFailed", "搜索失败，请重试")}
          </div>
        ) : mentionOptions.length === 0 ? (
          <div className="px-3 py-3 text-xs text-muted-foreground" role="status">
            {t("chat:mention.noResults", "没有匹配的对象")}
          </div>
        ) : (
          <>
            {mentionOptions.map((opt, i) => {
              const meta = MENTION_TYPE_META[opt.type]
              const TypeIcon = meta.icon
              const active = i === mentionActive
              return (
                <button
                  key={mentionKey(opt.type, opt.id)}
                  id={`chat-mention-opt-${i}`}
                  type="button"
                  role="option"
                  aria-selected={active}
                  aria-label={[
                    t(`chat:mention.${opt.type}`, opt.type),
                    opt.label,
                    opt.duplicateName ? opt.shortId || opt.id.slice(0, 8) : "",
                    opt.hint || "",
                  ]
                    .filter(Boolean)
                    .join(" ")}
                  onMouseEnter={() => setMentionActive(i)}
                  onPointerDown={(e) => {
                    // Keep editor focus / run select before TipTap suggestion exits.
                    e.preventDefault()
                    setMentionActive(i)
                    pickMentionOption(opt)
                  }}
                  className={cn(
                    "flex w-full items-start gap-2.5 px-3 text-left text-sm transition-colors",
                    isMobile ? "min-h-12 py-3" : "py-2.5",
                    active ? "bg-muted" : "hover:bg-muted-30",
                  )}
                >
                  <span
                    className={cn(
                      "mt-0.5 inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-nano font-medium",
                      meta.badge,
                    )}
                  >
                    <TypeIcon className="h-3 w-3" aria-hidden="true" />
                    {t(`chat:mention.${opt.type}`, opt.type)}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium text-foreground">{opt.label}</span>
                    {(opt.duplicateName || opt.hint) && (
                      <span className="mt-0.5 block truncate text-mini text-muted-foreground">
                        {[opt.shortId || opt.id.slice(0, 8), opt.hint].filter(Boolean).join(" · ")}
                      </span>
                    )}
                  </span>
                  {!opt.duplicateName && !opt.hint && (
                    <span className="max-w-32 shrink-0 truncate text-mini text-muted-foreground">
                      {opt.shortId || opt.id}
                    </span>
                  )}
                </button>
              )
            })}
            {mentionHasMore && (
              <button
                type="button"
                className="flex w-full items-center justify-center gap-1 border-t border-border px-3 py-2.5 text-xs text-muted-foreground hover:bg-muted-30"
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => void loadMoreMentions()}
                disabled={mentionLoading}
              >
                {mentionLoading && (
                  <Loader2 className="h-3 w-3 animate-spin" aria-hidden="true" />
                )}
                {t("chat:mention.loadMore", "加载更多")}
              </button>
            )}
          </>
        )}
      </div>
    </>
  )

  // Single root: hosts apply `space-y-*` to their direct children, which
  // would otherwise insert a gap between the preview strip and the box.
  return (
    <div data-chat-composer className="relative">
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

      {/* Single unified input box — everything inside one container.
          Mobile: radius + shadow live on this same node; avoid overflow:hidden
          here (it squares the soft shadow). Prefer an upward-biased shadow so
          ancestors that clip the bottom edge don't leave hard corner ledges. */}
      {/* @-mention picker — anchored above the input on desktop AND mobile.
          Do NOT use a focus-trapping Sheet here: on phones it dismisses the
          soft keyboard and Radix often closes on the same gesture that typed
          `@`, so the composer flickers with no visible menu. */}
      {mentionQuery !== null && (
        <div
          role="listbox"
          id="chat-mention-listbox"
          aria-label={t("chat:mention.title", "选择要引用的对象")}
          aria-busy={mentionLoading || undefined}
          aria-activedescendant={
            mentionOptions[mentionActive]
              ? `chat-mention-opt-${mentionActive}`
              : undefined
          }
          data-mention-menu
          className={cn(
            "absolute bottom-full left-0 right-0 z-[200] mb-2 overflow-hidden rounded-xl border border-border bg-popover shadow-lg",
            isMobile ? "max-h-[min(45vh,18rem)]" : "max-h-80",
          )}
        >
          {isMobile && (
            <div className="flex items-center gap-2 border-b border-border px-3 py-2">
              <AtSign className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
              <span className="text-sm font-medium text-foreground">
                {t("chat:mention.title", "选择要引用的对象")}
              </span>
            </div>
          )}
          {renderMentionPickerBody()}
        </div>
      )}
      <div className={cn(
        "rounded-lg border border-input bg-card shadow-sm transition-colors",
        isMobile && "rounded-[1.4rem] border-glass-border bg-card p-1.5 shadow-[0_-1px_2px_oklch(0_0_0/4%),0_2px_8px_-2px_oklch(0_0_0/10%)]",
        isMobile && mobileExpanded ? "flex flex-col" : isMobile ? "flex items-center gap-0.5" : "",
      )}
      data-chat-composer-surface
      onClick={(event) => {
        if (!isMobile || inputDisabled) return
        const target = event.target as HTMLElement
        if (target.closest('button, input, textarea, [role="button"], .ProseMirror')) return
        if (canMention) {
          activeEditorHostRef.current
            ?.querySelector<HTMLElement>(".ProseMirror")
            ?.focus({ preventScroll: true })
        } else {
          activeTextareaRef.current?.focus({ preventScroll: true })
        }
      }}
      onBlur={(event) => {
        if (!isMobile) return
        // Keep expanded while the mention menu / model sheet is open —
        // otherwise a tap collapses the composer mid-gesture and drops the click.
        if (mentionOpenRef.current || modelSheetOpenRef.current) return
        const next = event.relatedTarget as Node | null
        if (next && event.currentTarget.contains(next)) return
        // Touch taps on the action row often blur with relatedTarget=null
        // before click. Collapse only after the gesture finishes so the
        // toolbar does not reflow out from under the finger.
        const surface = event.currentTarget
        if (mobileCollapseTimerRef.current) clearTimeout(mobileCollapseTimerRef.current)
        mobileCollapseTimerRef.current = setTimeout(() => {
          mobileCollapseTimerRef.current = null
          if (mentionOpenRef.current || modelSheetOpenRef.current) return
          if (surface.contains(document.activeElement)) return
          setMobileExpanded(false)
        }, 0)
      }}>
        {canMention ? (
          <InlineMentionEditor
            value={value}
            contentNodes={contentNodes}
            placeholder={isStreaming && streamingPlaceholder ? streamingPlaceholder : placeholder}
            disabled={inputDisabled}
            minHeight={isMobile ? 44 : minHeight}
            maxHeight={maxHeight}
            listboxId="chat-mention-listbox"
            mentionOpen={mentionQuery !== null}
            invalidRefs={invalidRefs}
            editorHostRef={activeEditorHostRef}
            openMentionRequest={openMentionRequest}
            onPayloadChange={handleTipTapPayload}
            onSuggestionStart={handleSuggestionStart}
            onSuggestionUpdate={handleSuggestionUpdate}
            onSuggestionExit={handleSuggestionExit}
            onSuggestionKeyDown={(props) => handleMentionNavKeys(props.event)}
            onKeyDown={handleTipTapKeyDown}
            onCompositionChange={(composing) => {
              isComposing.current = composing
            }}
            onFocus={() => {
              if (!isMobile) return
              if (mobileCollapseTimerRef.current) {
                clearTimeout(mobileCollapseTimerRef.current)
                mobileCollapseTimerRef.current = null
              }
              setMobileExpanded(true)
            }}
            className={cn(
              isMobile && mobileExpanded && "min-w-0 px-3 pb-1.5 pt-2.5",
              isMobile && !mobileExpanded && "min-w-0 flex-1 px-2 py-3",
            )}
          />
        ) : (
          <textarea
            ref={activeTextareaRef}
            value={value}
            onChange={(e) => handleChange(e.target.value)}
            onCompositionStart={() => { isComposing.current = true }}
            onCompositionEnd={(e) => {
              isComposing.current = false
              const el = e.currentTarget
              const caret =
                typeof el.selectionStart === "number" && el.selectionStart > 0
                  ? el.selectionStart
                  : el.value.length
              scanMentionTrigger(el.value, caret)
            }}
            onFocus={() => {
              if (!isMobile) return
              if (mobileCollapseTimerRef.current) {
                clearTimeout(mobileCollapseTimerRef.current)
                mobileCollapseTimerRef.current = null
              }
              setMobileExpanded(true)
            }}
            onKeyDown={handleKeyDownInternal}
            role="textbox"
            placeholder={isStreaming && streamingPlaceholder ? streamingPlaceholder : placeholder}
            rows={1}
            disabled={inputDisabled}
            className={cn(
              "block w-full resize-none bg-transparent px-4 pb-1 pt-3 text-base leading-5 sm:text-sm",
              isMobile && mobileExpanded && "min-w-0 px-3 pb-1.5 pt-2.5",
              isMobile && !mobileExpanded && "min-w-0 flex-1 px-2 py-3",
              "placeholder:text-muted-foreground",
              "cursor-text caret-foreground focus:outline-none",
              "max-h-[100px] scroll-mb-32",
              "disabled:opacity-60"
            )}
            style={{ minHeight: `${isMobile ? 44 : minHeight}px` }}
            onInput={(e) => {
              const target = e.target as HTMLTextAreaElement
              target.style.height = "auto"
              target.style.height = Math.max(minHeight, Math.min(target.scrollHeight, maxHeight)) + "px"
            }}
          />
        )}

        {/* Bottom toolbar — left: image + model + context, right: send */}
        <div
          className={isMobile
            ? (mobileExpanded ? "flex w-full items-center gap-0.5 px-0.5 pb-0.5" : "contents")
            : "flex items-center gap-1 px-2 pb-2"
          }
          onPointerDown={(event) => {
            // Keep the editor focused while tapping actions so the expanded
            // layout does not collapse between pointerdown and click.
            if (!isMobile || !mobileExpanded) return
            const target = event.target as HTMLElement
            if (target.closest("button, [role='button'], input, label")) {
              event.preventDefault()
            }
          }}
        >
          {/* @ mention toolbar — TipTap inserts trigger at caret */}
          {canMention && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              disabled={inputDisabled || references.length >= MAX_REFERENCES}
              className={cn(
                "rounded-lg flex-shrink-0 text-muted-foreground hover:text-foreground",
                isMobile && "h-10 w-10 rounded-full p-0",
              )}
              title={t("chat:mention.insert", "插入引用")}
              aria-label={t("chat:mention.insert", "插入引用")}
              onClick={() => setOpenMentionRequest((n) => n + 1)}
            >
              <AtSign className="h-4 w-4" aria-hidden="true" />
            </Button>
          )}
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
                  isMobile && "order-first h-10 w-10 rounded-full p-0",
                  !supportsMultimodal && "opacity-50"
                )}
                title={supportsMultimodal ? (visionDelegate ? t("chat:model.visionViaCompanion") : t("chat:model.addImage")) : t("chat:model.notSupportImage", "sm:h-10 sm:w-10")}
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
                  <Plus className="h-5 w-5" />
                )}
              </Button>
            </>
          )}

          {/* Model selector */}
          {backends && backends.length > 0 && (!isMobile || backends.length > 1) && (isMobile ? (
            <>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                onClick={() => setModelSheetOpen(true)}
                className="relative h-10 w-10 shrink-0 rounded-full p-0 text-muted-foreground sm:h-10 sm:w-10"
                aria-label={`${t("chat:input.selectLLMModel")}: ${
                  backends.find(b => b.id === activeBackendId)?.name ||
                  backends.find(b => b.id === activeBackendId)?.model ||
                  t("chat:input.selectModel")
                }`}
                title={
                  backends.find(b => b.id === activeBackendId)?.name ||
                  backends.find(b => b.id === activeBackendId)?.model ||
                  t("chat:input.selectModel")
                }
              >
                <Sparkles className="h-[18px] w-[18px]" aria-hidden="true" />
              </Button>
              <Sheet open={modelSheetOpen} onOpenChange={setModelSheetOpen}>
                <SheetContent
                  side="bottom"
                  className="max-h-[70dvh] rounded-t-[1.75rem] border-t border-border px-0 pb-[calc(1rem+env(safe-area-inset-bottom,0px))] pt-3 [&>button]:hidden"
                >
                  <div className="mx-auto mb-4 h-1 w-10 rounded-full bg-border" aria-hidden="true" />
                  <div className="px-5 pb-3">
                    <SheetTitle className="text-left text-xl font-semibold">
                      {t("chat:input.selectLLMModel")}
                    </SheetTitle>
                    <SheetDescription className="mt-1 text-left text-sm text-muted-foreground">
                      {t("chat:model.forConversation", "本次对话使用的模型")}
                    </SheetDescription>
                  </div>
                  <div className="overflow-y-auto px-5">
                    {backends.map((backend) => {
                      const active = backend.id === activeBackendId
                      const capability = [
                        backend.backend_type,
                        backend.capabilities?.supports_tools ? t("chat:capability.tools", "工具调用") : null,
                        backend.capabilities?.supports_multimodal ? t("chat:capability.vision", "视觉") : null,
                      ].filter(Boolean).join(" · ")
                      return (
                        <button
                          key={backend.id}
                          type="button"
                          onClick={() => {
                            onActivateBackend?.(backend.id)
                            setModelSheetOpen(false)
                          }}
                          className="flex min-h-16 w-full items-center gap-3 border-b border-border py-3 text-left last:border-b-0"
                        >
                          <span className="min-w-0 flex-1">
                            <span className={cn("block truncate text-base", active && "font-semibold")}>
                              {backend.name || backend.model}
                            </span>
                            <span className="mt-0.5 block truncate text-xs text-muted-foreground">{capability}</span>
                          </span>
                          {active && <Check className="h-5 w-5 shrink-0" aria-hidden="true" />}
                        </button>
                      )
                    })}
                  </div>
                </SheetContent>
              </Sheet>
            </>
          ) : (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-8 px-2 rounded-lg text-muted-foreground hover:text-foreground text-xs gap-1 max-w-[120px] sm:max-w-[140px] sm:h-8"
                >
                  <span className="truncate">
                    {backends.find(b => b.id === activeBackendId)?.name ||
                     backends.find(b => b.id === activeBackendId)?.model ||
                     t("chat:input.selectModel")}
                  </span>
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
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span className={cn("truncate text-sm text-foreground", isActive && "font-medium")}>{backend.name || backend.model}</span>
                            {/* Capability hints sit top-right, on the name line — bottom-right crowded the model/protocol line. */}
                            <span className="ml-auto flex shrink-0 items-center gap-1.5 text-muted-foreground" aria-label={[
                              backend.capabilities?.supports_multimodal && t("chat:capability.vision", { defaultValue: "Vision" }),
                              backend.capabilities?.supports_tools && t("chat:capability.tools", { defaultValue: "Tools" }),
                              backend.capabilities?.supports_thinking && t("chat:capability.thinking", { defaultValue: "Thinking" }),
                            ].filter(Boolean).join(', ')}>
                              {backend.capabilities?.supports_multimodal && <span title={t("chat:model.supportsVision")}><Eye className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                              {backend.capabilities?.supports_tools && <span title={t("chat:model.supportsTools")}><Wrench className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                              {backend.capabilities?.supports_thinking && <span title={t("chat:model.supportsThinking")}><Sparkles className="h-3.5 w-3.5" aria-hidden="true" /></span>}
                            </span>
                          </div>
                          <div className="mt-0.5 flex min-w-0 items-center gap-1.5">
                            <span className="shrink-0 rounded bg-muted px-1 py-px text-nano leading-4 text-muted-foreground">{backend.backend_type}</span>
                            <span className="min-w-0 flex-1 truncate text-mini text-muted-foreground">{backend.model}</span>
                          </div>
                        </div>
                      </DropdownMenuItem>
                    )
                  })}
                </div>
              </DropdownMenuContent>
            </DropdownMenu>
          ))}

          {/* Context usage — progress ring; hover for the breakdown card */}
          {contextUsage && (() => {
            const pending = !!contextUsage.pending
            // Pending must not look like an empty/broken ring — show a soft
            // indeterminate arc. Measured overrun clamps to a full ring.
            const ratio = pending ? 0.28 : Math.min(1, contextUsage.used / Math.max(contextUsage.max, 1))
            const color = pending
              ? 'var(--muted-foreground)'
              : ratio > 0.9 ? 'var(--error)' : ratio > 0.7 ? 'var(--warning)' : 'var(--muted-foreground)'
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
            const pct = pending ? null : Math.round(Math.min(1, contextUsage.used / Math.max(contextUsage.max, 1)) * 100)
            const rows: Array<{ label: string; value?: number; color: string; suffix?: string }> = [
              { label: t('chat:context.systemPrompt', 'System prompt'), value: contextUsage.system, color: 'var(--primary)' },
              { label: t('chat:context.toolDefs', 'Tool definitions'), value: contextUsage.tools, color: 'var(--accent-cyan)' },
              { label: t('chat:context.history', 'Conversation history'), value: contextUsage.history, color: 'var(--muted-foreground)', suffix: contextUsage.messageCount != null ? t('chat:context.msgCount', { defaultValue: ' · {{count}} msgs', count: contextUsage.messageCount }) : undefined },
            ]
            const availableRows = rows.filter(r => r.value != null)
            return (
              <div className="max-sm:hidden">
              <TooltipProvider delayDuration={150}>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <button
                      type="button"
                      className="shrink-0 h-8 flex items-center gap-1.5 text-muted-foreground hover:text-foreground transition-colors"
                      aria-label={`${t('chat:context.title', 'Context usage')}: ${pending ? t('chat:context.awaitingMeasurement', 'Waiting for request context measurement') : `${pct}%`}`}
                    >
                      <svg width="20" height="20" viewBox="0 0 20 20" className={cn("-rotate-90", pending && "animate-pulse")}>
                        <circle cx="10" cy="10" r={R} fill="none" stroke="var(--border)" strokeWidth="2.5" />
                        <circle
                          cx="10" cy="10" r={R} fill="none"
                          stroke={color} strokeWidth="2.5" strokeLinecap="round"
                          strokeDasharray={pending ? `${C * 0.2} ${C * 0.8}` : C}
                          strokeDashoffset={pending ? C * 0.05 : C * (1 - ratio)}
                          className="transition-all duration-500"
                        />
                      </svg>
                      <span className={cn(
                        "text-xs tabular-nums",
                        !pending && pct != null && pct > 90 ? "text-error" : !pending && pct != null && pct > 70 ? "text-warning" : "text-muted-foreground",
                      )}>
                        {pending ? "…" : fmt(contextUsage.used)}
                      </span>
                    </button>
                  </TooltipTrigger>
                  <TooltipContent side="top" align="end" className="w-72 max-w-[calc(100vw-1rem)] rounded-xl border border-border p-4 shadow-lg">
                    <div className="mb-3 flex items-start justify-between gap-3">
                      <div>
                        <p className="text-xs font-semibold text-foreground">{t('chat:context.title', 'Context usage')}</p>
                        <p className="mt-1 text-lg font-semibold tabular-nums leading-none text-foreground">
                          {pending ? "…" : fmt(contextUsage.used)}
                          <span className="text-xs font-normal text-muted-foreground"> / {fmt(contextUsage.max)}</span>
                        </p>
                      </div>
                      <span className="rounded-full bg-muted px-2 py-0.5 text-xs font-medium tabular-nums text-foreground">
                        {pending ? "…" : `${pct}%`}
                      </span>
                    </div>
                    <div className="mb-3 h-1.5 overflow-hidden rounded-full bg-muted" role="progressbar" aria-valuenow={pending ? 0 : (pct ?? 0)} aria-valuemin={0} aria-valuemax={100}>
                      <div className="h-full rounded-full transition-[width] duration-500" style={{ width: `${pending ? 28 : (pct ?? 0)}%`, backgroundColor: color }} />
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
              </div>
            )
          })()}

          <div className={cn("flex-1", isMobile && !mobileExpanded && "hidden")} />

          {/* Send or Cancel button */}
          {isStreaming ? (
            <Button
              type="button"
              onClick={onCancel}
              variant="outline"
              className="h-10 w-10 rounded-full flex-shrink-0 p-0 border-destructive text-destructive hover:bg-destructive-light"
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
                "h-10 w-10 rounded-full flex-shrink-0 p-0 transition-all",
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

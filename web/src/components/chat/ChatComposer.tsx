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
  GitBranch,
  Image as ImageIcon,
  Loader2,
  Plus,
  Sparkles,
  Trash2,
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
  { icon: typeof Cpu }
> = {
  device: { icon: Cpu },
  rule: { icon: Zap },
  transform: { icon: GitBranch },
  agent: { icon: Bot },
}

/** Shared model row for desktop dropdown + mobile sheet.
 *  Left check slot, name first, provider/model secondary, low-contrast
 *  capability chips. Selected chrome lives on the parent row (bg-muted). */
function ModelOptionBody({
  backend,
  active,
}: {
  backend: LlmBackendInstance
  active: boolean
}) {
  const { t } = useTranslation()
  const provider = t(`common:llm.providers.${backend.backend_type}`, {
    defaultValue: backend.backend_type,
  })
  const capabilities = [
    backend.capabilities?.supports_tools
      ? t("chat:capability.tools", { defaultValue: "Tools" })
      : null,
    backend.capabilities?.supports_multimodal
      ? t("chat:capability.vision", { defaultValue: "Vision" })
      : null,
    backend.capabilities?.supports_thinking
      ? t("chat:capability.thinking", { defaultValue: "Thinking" })
      : null,
  ].filter((label): label is string => Boolean(label))
  const source = [provider, backend.model].filter(Boolean).join(" · ")

  return (
    <>
      <span
        className="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center"
        aria-hidden="true"
      >
        {active ? <Check className="h-4 w-4 text-foreground" /> : null}
      </span>
      {backend.healthy != null && (
        <span
          title={backend.healthy ? t("common:online") : t("common:offline")}
          className={cn(
            "mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full",
            backend.healthy ? "bg-success" : "bg-error",
          )}
          aria-hidden="true"
        />
      )}
      <span className="min-w-0 flex-1">
        <span
          className={cn(
            "block truncate text-sm leading-5 text-foreground",
            active && "font-medium",
          )}
        >
          {backend.name || backend.model}
        </span>
        <span className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-0.5">
          {source && (
            <span className="min-w-0 truncate text-mini text-muted-foreground">
              {source}
            </span>
          )}
          {capabilities.map((label) => (
            <span
              key={label}
              className="shrink-0 rounded-md bg-muted-30 px-1.5 py-px text-mini text-muted-foreground"
            >
              {label}
            </span>
          ))}
        </span>
      </span>
    </>
  )
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
  /**
   * Mobile scroll-vs-tap: pointerdown on an option used to preventDefault +
   * select immediately, so any scroll gesture that started on a row became a
   * mistaken pick. Track movement; only select on pointerup within slop.
   */
  const mentionGestureRef = useRef<{
    pointerId: number
    x: number
    y: number
    opt: MentionOption
    index: number
    moved: boolean
  } | null>(null)
  const canMention = !!onReferencesChange
  const canAttach = !!onAttachmentsChange
  const inputDisabled = disabled || isStreaming

  // Auto-grow: desktop always; mobile while expanded (collapsed stays one row).
  // Cap ~4–6 lines then the field scrolls internally (TipTap host / textarea).
  const growMinHeight = isMobile ? 44 : minHeight
  const growMaxHeight = isMobile
    ? (mobileExpanded ? maxHeight : growMinHeight)
    : maxHeight

  const syncTextareaHeight = () => {
    const el = activeTextareaRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${Math.max(growMinHeight, Math.min(el.scrollHeight, growMaxHeight))}px`
  }

  useEffect(() => {
    if (canMention) return
    syncTextareaHeight()
    // eslint-disable-next-line react-hooks/exhaustive-deps -- grow bounds + value
  }, [value, growMinHeight, growMaxHeight, mobileExpanded, canMention])

  // IME composition (Chinese/Japanese input): intermediate text must not
  // open the picker — only post-composition changes count.
  const isComposing = useRef(false)
  const mentionOpenRef = useRef(false)
  const modelSheetOpenRef = useRef(false)
  /** Delayed mobile collapse — touch blurs often report null relatedTarget. */
  const mobileCollapseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const MENTION_TAP_SLOP_PX = 12

  useEffect(() => {
    mentionOpenRef.current = mentionQuery !== null
  }, [mentionQuery])

  /** Latest mention search context (query + type filter). loadMoreMentions
   *  snapshots it at request time and discards its own response when the
   *  context moved on (retyped query / switched filter / closed picker) — a
   *  stale page must not merge into the new list or stomp its pagination /
   *  loading state (E17). */
  const mentionSearchCtxRef = useRef<{ query: string | null; filter: MentionType | "all" }>({
    query: null,
    filter: "all",
  })
  useEffect(() => {
    mentionSearchCtxRef.current = { query: mentionQuery, filter: mentionTypeFilter }
  }, [mentionQuery, mentionTypeFilter])

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
    // Snapshot the search context at request time — the response must land
    // in the list it was requested for, never in a newer query's (E17).
    const ctx = { query: mentionQuery, filter: mentionTypeFilter }
    const isStale = () =>
      mentionSearchCtxRef.current.query !== ctx.query ||
      mentionSearchCtxRef.current.filter !== ctx.filter
    setMentionLoading(true)
    try {
      const result = await searchMentions({
        query: ctx.query,
        types: ctx.filter === "all" ? "all" : [ctx.filter],
        limit: 20,
        offset: mentionNextOffset,
      })
      if (isStale()) return
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
      // A newer context owns the loading flag (its own effect set it); only
      // clear it when still current, or when the picker closed and nobody
      // else will.
      if (!isStale() || mentionSearchCtxRef.current.query === null) setMentionLoading(false)
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
    // Same cap as handleSuggestionStart: without it the picker opens at the
    // reference cap and a pick silently closes with no feedback.
    if (references.length >= MAX_REFERENCES) {
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

  const handleSuggestionExit = (meta?: { editorFocused: boolean }) => {
    if (suppressMentionExitClose.current) return
    // Mobile option taps blur the editor; TipTap reports editorFocused=false
    // while the host menu is still open for a deferred pointerup select.
    // Keep the stored command alive until the gesture finishes / the user
    // dismisses the menu another way.
    if (meta && meta.editorFocused === false && mentionQuery !== null) {
      return
    }
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
    // TipTap path lost its command (exit raced a deferred mobile select) —
    // close the host menu instead of silently no-op'ing while still visible.
    if (mentionQuery !== null && !mentionTrigger.current) {
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
    // Select while the stored TipTap command is still valid (may outlive blur
    // when suppressMentionExitClose kept the host menu open).
    selectMention(opt)
  }

  const releaseMentionSuppress = () => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        suppressMentionExitClose.current = false
      })
    })
  }

  const clearMentionGesture = (pointerId?: number) => {
    const g = mentionGestureRef.current
    if (!g) return
    if (pointerId != null && g.pointerId !== pointerId) return
    mentionGestureRef.current = null
    releaseMentionSuppress()
  }

  const finishMentionGesture = (pointerId: number) => {
    const g = mentionGestureRef.current
    if (!g || g.pointerId !== pointerId) return
    const shouldSelect = !g.moved
    const { opt, index } = g
    mentionGestureRef.current = null
    if (shouldSelect) {
      setMentionActive(index)
      pickMentionOption(opt)
    }
    releaseMentionSuppress()
  }

  const startMobileMentionGesture = (
    e: { pointerId: number; clientX: number; clientY: number },
    opt: MentionOption,
    index: number,
  ) => {
    // jsdom's fireEvent may omit pointerId (MouseEvent fallback); default to 1.
    const pointerId = typeof e.pointerId === "number" ? e.pointerId : 1
    suppressMentionExitClose.current = true
    mentionGestureRef.current = {
      pointerId,
      x: e.clientX,
      y: e.clientY,
      opt,
      index,
      moved: false,
    }
    // Window listeners survive when the finger leaves the row while scrolling.
    const onMove = (ev: PointerEvent) => {
      const g = mentionGestureRef.current
      if (!g || g.pointerId !== pointerId || g.moved) return
      const x = typeof ev.clientX === "number" ? ev.clientX : 0
      const y = typeof ev.clientY === "number" ? ev.clientY : 0
      if (Math.hypot(x - g.x, y - g.y) > MENTION_TAP_SLOP_PX) {
        g.moved = true
      }
    }
    const onEnd = (ev: PointerEvent) => {
      const endId = typeof ev.pointerId === "number" ? ev.pointerId : pointerId
      if (endId !== pointerId) return
      window.removeEventListener("pointermove", onMove)
      window.removeEventListener("pointerup", onEnd)
      window.removeEventListener("pointercancel", onEnd)
      if (ev.type === "pointercancel") clearMentionGesture(pointerId)
      else finishMentionGesture(pointerId)
    }
    window.addEventListener("pointermove", onMove)
    window.addEventListener("pointerup", onEnd)
    window.addEventListener("pointercancel", onEnd)
  }

  const renderMentionPickerBody = () => (
    <>
      <div
        className={cn(
          "flex shrink-0 flex-wrap gap-1 border-b border-border",
          isMobile ? "px-3 py-2" : "px-2.5 py-2",
        )}
        role="tablist"
        aria-label={t("chat:mention.filterTypes", "按类型筛选")}
      >
        {(["all", "device", "rule", "transform", "agent"] as const).map((tab) => {
          const selected = mentionTypeFilter === tab
          return (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={selected}
              className={cn(
                // Text-only chips: tighter, uniform height; selected = muted
                // block (no icon clutter). Filter behavior unchanged.
                "inline-flex h-7 items-center rounded-md px-2 text-nano transition-colors",
                selected
                  ? "bg-muted font-medium text-foreground"
                  : "text-muted-foreground hover:bg-muted-30 hover:text-foreground",
              )}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => setMentionTypeFilter(tab)}
            >
              {tab === "all"
                ? t("chat:mention.filterAll", "全部")
                : t(`chat:mention.${tab}`, tab)}
            </button>
          )
        })}
      </div>
      {mentionTypeErrors.length > 0 && (
        <div className="shrink-0 border-b border-border px-3 py-2 text-nano text-warning" role="status">
          {t("chat:mention.partialFailure", "部分类型加载失败")}
          {": "}
          {mentionTypeErrors.join("; ")}
        </div>
      )}
      <div
        className={cn(
          "min-h-0 flex-1 overflow-y-auto overscroll-contain",
          isMobile ? "touch-pan-y" : "max-h-64",
        )}
        data-mention-scroll
      >
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
              const typeLabel = t(`chat:mention.${opt.type}`, opt.type)
              const disambiguation = [
                opt.duplicateName ? opt.shortId || opt.id.slice(0, 8) : null,
                opt.hint || null,
                !opt.duplicateName && !opt.hint ? opt.shortId || opt.id.slice(0, 8) : null,
              ].filter(Boolean)
              return (
                <button
                  key={mentionKey(opt.type, opt.id)}
                  id={`chat-mention-opt-${i}`}
                  type="button"
                  role="option"
                  aria-selected={active}
                  aria-label={[typeLabel, opt.label, ...disambiguation]
                    .filter(Boolean)
                    .join(" ")}
                  onMouseEnter={() => {
                    if (!isMobile) setMentionActive(i)
                  }}
                  onPointerDown={(e) => {
                    if (isMobile) {
                      // Do NOT preventDefault — that blocks native list scrolling.
                      startMobileMentionGesture(e, opt, i)
                      return
                    }
                    // Desktop: keep editor focus and select immediately.
                    suppressMentionExitClose.current = true
                    e.preventDefault()
                    setMentionActive(i)
                    pickMentionOption(opt)
                  }}
                  className={cn(
                    "flex w-full items-start gap-2.5 px-3 text-left text-sm transition-colors",
                    isMobile ? "min-h-11 py-2.5" : "py-2",
                    active ? "bg-muted" : "hover:bg-muted-30",
                  )}
                >
                  <TypeIcon
                    className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground"
                    aria-hidden="true"
                  />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium text-foreground">
                      {opt.label}
                    </span>
                    <span className="mt-0.5 flex min-w-0 items-center gap-1.5 text-mini text-muted-foreground">
                      <span className="shrink-0">{typeLabel}</span>
                      {disambiguation.length > 0 && (
                        <span className="min-w-0 truncate">
                          · {disambiguation.join(" · ")}
                        </span>
                      )}
                    </span>
                  </span>
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
          onPointerDown={() => {
            // Touching the menu blurs the editor; keep the TipTap suggestion
            // command alive so a later tap can still insert. Self-release on
            // the next frames — a stuck-true flag would permanently block
            // handleSuggestionExit (desktop picks bubble into this handler
            // AFTER selectMention's finally has already reset the flag, so
            // without the release the menu can never close again).
            //
            // Mobile options start a gesture here (select deferred to
            // pointerup); releasing suppress while the finger is still down
            // re-opens the exit race and drops the pick. Keep suppress armed
            // for the whole gesture.
            suppressMentionExitClose.current = true
            if (!mentionGestureRef.current) {
              releaseMentionSuppress()
            }
          }}
          className={cn(
            // Keep ~3–4 option rows visible so the menu does not crowd the
            // composer (esp. mobile + soft keyboard). Long lists still scroll.
            "absolute bottom-full left-0 right-0 z-[200] mb-2 flex flex-col overflow-hidden rounded-xl border border-border bg-popover shadow-lg",
            isMobile ? "max-h-[min(34dvh,12rem)]" : "max-h-52",
          )}
        >
          {isMobile && (
            <div className="flex shrink-0 items-center gap-2 border-b border-border px-3 py-2">
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
            minHeight={growMinHeight}
            maxHeight={growMaxHeight}
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
              "scroll-mb-32",
              "disabled:opacity-60"
            )}
            style={{
              minHeight: `${growMinHeight}px`,
              maxHeight: `${growMaxHeight}px`,
              overflowY: "auto",
            }}
            onInput={() => syncTextareaHeight()}
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
              title={t("chat:mention.insert")}
              aria-label={t("chat:mention.insert")}
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
                  <div className="space-y-1 overflow-y-auto px-3 pb-1">
                    {backends.map((backend) => {
                      const active = backend.id === activeBackendId
                      return (
                        <button
                          key={backend.id}
                          type="button"
                          onClick={() => {
                            onActivateBackend?.(backend.id)
                            setModelSheetOpen(false)
                          }}
                          className={cn(
                            "flex min-h-11 w-full items-start gap-2.5 rounded-xl px-3 py-2.5 text-left transition-colors",
                            active ? "bg-muted" : "active:bg-muted-30",
                          )}
                        >
                          <ModelOptionBody backend={backend} active={active} />
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
              <DropdownMenuContent
                align="start"
                className="w-[min(22rem,calc(100vw-2rem))] max-h-[min(18rem,50vh)] overflow-y-auto p-1 scrollbar-none"
              >
                {backends.map((backend) => {
                  const isActive = backend.id === activeBackendId
                  return (
                    <DropdownMenuItem
                      key={backend.id}
                      onClick={() => onActivateBackend?.(backend.id)}
                      className={cn(
                        "my-0 min-h-0 items-start gap-2.5 rounded-lg px-2.5 py-1.5 focus:bg-muted focus:text-foreground sm:min-h-0 sm:py-1.5",
                        isActive && "bg-muted",
                      )}
                    >
                      <ModelOptionBody backend={backend} active={isActive} />
                    </DropdownMenuItem>
                  )
                })}
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
            const availableRows = rows.filter(
              (r): r is { label: string; value: number; color: string; suffix?: string } => r.value != null,
            )
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
                      {/* No lone "—" / "…" when measurement is missing — ring alone is enough. */}
                      {!pending && (
                        <span className={cn(
                          "text-xs tabular-nums",
                          pct != null && pct > 90 ? "text-error" : pct != null && pct > 70 ? "text-warning" : "text-muted-foreground",
                        )}>
                          {fmt(contextUsage.used)}
                        </span>
                      )}
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
                          <span className="shrink-0 text-xs font-medium tabular-nums text-foreground">{fmt(r.value) + (r.suffix ?? '')}</span>
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

          {/* Send or Cancel button — size=icon-sm so default px/sm:h don't ovalize h-8 w-8 */}
          {isStreaming ? (
            <Button
              type="button"
              onClick={onCancel}
              variant="outline"
              size="icon-sm"
              className="h-8 w-8 sm:h-8 sm:w-8 shrink-0 rounded-full p-0 border-border text-muted-foreground hover:bg-muted-30 hover:text-foreground"
              title={t("chat:input.cancel")}
              aria-label={t("chat:input.cancel")}
            >
              <Loader2 className="h-3.5 w-3.5 animate-spin text-primary" />
            </Button>
          ) : (
            <Button
              type="button"
              onClick={onSend}
              disabled={!canSend}
              size="icon-sm"
              className={cn(
                "h-8 w-8 sm:h-8 sm:w-8 shrink-0 rounded-full p-0 transition-all",
                !canSend
                  ? "bg-muted text-muted-foreground"
                  : "bg-primary hover:bg-primary-hover text-primary-foreground"
              )}
            >
              <ArrowUp className="h-3.5 w-3.5" />
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
